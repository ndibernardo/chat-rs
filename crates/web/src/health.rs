use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use serde::Serialize;
use sqlx::PgPool;

/// A dependency this process needs in order to serve traffic correctly.
///
/// `/readyz` reports 503 if any check fails; `/livez` never consults these —
/// liveness answers "is the process alive", not "are its dependencies up",
/// so a flaky dependency must not trigger a restart loop.
#[async_trait]
pub trait ReadyCheck: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self) -> Result<(), String>;
}

/// Shared state for the `/livez` + `/readyz` router.
#[derive(Clone)]
pub struct HealthState {
    draining: Arc<AtomicBool>,
    checks: Arc<Vec<Arc<dyn ReadyCheck>>>,
}

impl HealthState {
    pub fn new(checks: Vec<Arc<dyn ReadyCheck>>) -> Self {
        Self {
            draining: Arc::new(AtomicBool::new(false)),
            checks: Arc::new(checks),
        }
    }

    /// A handle a shutdown sequence can flip before draining connections, so
    /// `/readyz` starts failing ahead of the actual drain.
    pub fn draining_flag(&self) -> Arc<AtomicBool> {
        self.draining.clone()
    }
}

#[derive(Serialize)]
struct ReadyFailure {
    check: String,
    error: String,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: &'static str,
    failures: Vec<ReadyFailure>,
}

async fn livez() -> StatusCode {
    StatusCode::OK
}

async fn readyz(State(state): State<HealthState>) -> Response {
    if state.draining.load(Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "draining",
                failures: Vec::new(),
            }),
        )
            .into_response();
    }

    let mut failures = Vec::new();
    for check in state.checks.iter() {
        if let Err(error) = check.check().await {
            failures.push(ReadyFailure {
                check: check.name().to_string(),
                error,
            });
        }
    }

    if failures.is_empty() {
        (
            StatusCode::OK,
            Json(ReadyResponse {
                status: "ready",
                failures,
            }),
        )
            .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "not_ready",
                failures,
            }),
        )
            .into_response()
    }
}

/// `/livez` (always 200 while the process is up) and `/readyz` (503 while
/// draining, or if any check fails, with the failing checks as JSON).
/// `/health` is kept as a `/livez` alias so existing compose healthchecks
/// keep working unchanged.
pub fn health_router(state: HealthState) -> Router {
    Router::new()
        .route("/livez", get(livez))
        .route("/health", get(livez))
        .route("/readyz", get(readyz))
        .with_state(state)
}

/// Readiness check for a Postgres connection pool: `SELECT 1`.
pub struct PgReadyCheck {
    pool: PgPool,
}

impl PgReadyCheck {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReadyCheck for PgReadyCheck {
    fn name(&self) -> &str {
        "postgres"
    }

    async fn check(&self) -> Result<(), String> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
