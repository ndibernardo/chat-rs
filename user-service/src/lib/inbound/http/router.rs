use std::sync::Arc;
use std::time::Duration;

use auth::Authenticator;
use axum::body::Body;
use axum::http::Request;
use axum::http::Response;
use axum::middleware;
use axum::routing::delete;
use axum::routing::get;
use axum::routing::patch;
use axum::routing::post;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::Span;

use super::handlers::authenticate::authenticate;
use super::handlers::create_user::create_user;
use super::handlers::delete_user::delete_user;
use super::handlers::get_user::get_user;
use super::handlers::update_user::update_user;
use super::middleware::authenticate as auth_middleware;
use crate::domain::user::service::UserService;
use crate::outbound::events::KafkaEventProducer;
use crate::outbound::repositories::user::PostgresUserRepository;

#[derive(Clone)]
pub struct AppState {
    pub user_service: Arc<UserService<PostgresUserRepository, KafkaEventProducer>>,
    pub authenticator: Arc<Authenticator>,
    pub jwt_expiration_hours: i64,
}

pub fn create_router(
    user_service: Arc<UserService<PostgresUserRepository, KafkaEventProducer>>,
    authenticator: Arc<Authenticator>,
    jwt_expiration_hours: i64,
) -> Router {
    let state = AppState {
        user_service,
        authenticator,
        jwt_expiration_hours,
    };

    let public_routes = Router::new()
        .route("/api/auth/login", post(authenticate))
        .route("/api/users", post(create_user));

    let protected_routes = Router::new()
        .route("/api/users/:user_id", get(get_user))
        .route("/api/users/:user_id", patch(update_user))
        .route("/api/users/:user_id", delete(delete_user))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<Body>| {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
                headers = ?request.headers(),
            )
        })
        .on_request(|request: &Request<Body>, _span: &Span| {
            tracing::info!(
                method = %request.method(),
                uri = %request.uri(),
                "Request started"
            );
        })
        .on_response(
            |response: &Response<Body>, latency: Duration, _span: &Span| {
                tracing::info!(
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis(),
                    "Request completed"
                );
            },
        );

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(trace_layer)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
