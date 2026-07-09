use std::sync::Arc;

use axum::Json;
use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::{self};
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response;
use serde_json::json;

/// Authenticated caller's identity, attached to request extensions by [`authenticate`].
///
/// `user_id` is a raw UUID rather than a service's own domain `UserId`
/// newtype: this crate sits below both services and cannot depend on either
/// one's domain layer. Handlers convert it to their own domain type at the
/// point of use.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: uuid::Uuid,
    pub username: String,
}

/// Axum middleware that validates a bearer JWT and attaches an
/// [`AuthenticatedUser`] to the request's extensions for downstream handlers
/// to extract via `Extension<AuthenticatedUser>`.
pub async fn authenticate(
    State(authenticator): State<Arc<auth::Authenticator>>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let token = extract_token_from_header(&req)?;

    let claims: auth::Claims = authenticator.validate_token(token).map_err(|e| {
        tracing::warn!("JWT validation failed: {}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid or expired token"})),
        )
            .into_response()
    })?;

    let user_id_str = claims.sub.as_ref().ok_or_else(|| {
        tracing::error!("Missing 'sub' claim in token");
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid token format"})),
        )
            .into_response()
    })?;

    let user_id = uuid::Uuid::parse_str(user_id_str).map_err(|e| {
        tracing::error!("Failed to parse user ID from token: {}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid token format"})),
        )
            .into_response()
    })?;

    let username = claims.username().unwrap_or_else(|| "unknown".to_string());

    req.extensions_mut()
        .insert(AuthenticatedUser { user_id, username });

    Ok(next.run(req).await)
}

// The `Response` error carries the exact rejection body/status for the
// caller to return as-is; boxing it would just move the size complaint to
// every call site's `?` conversion for no benefit on this cold path.
#[allow(clippy::result_large_err)]
fn extract_token_from_header(req: &Request) -> Result<&str, Response> {
    let auth_header = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Missing Authorization header"
                })),
            )
                .into_response()
        })?;

    let auth_str = auth_header.to_str().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid Authorization header"
            })),
        )
            .into_response()
    })?;

    if !auth_str.starts_with("Bearer ") {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid Authorization header format. Expected: Bearer <token>"
            })),
        )
            .into_response());
    }

    Ok(auth_str.trim_start_matches("Bearer "))
}
