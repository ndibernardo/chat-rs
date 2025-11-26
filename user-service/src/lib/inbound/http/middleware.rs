use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::{self};
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde_json::json;

use crate::domain::user::models::UserId;
use crate::inbound::http::router::AppState;

/// Extension type to store authenticated user ID in request extensions
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: UserId,
    pub username: String,
}

/// Middleware that validates JWT tokens and adds user info to request extensions
pub async fn authenticate(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    // Extract token from Authorization header
    let token = extract_token_from_header(&req)?;

    // Validate token and extract claims (from auth library)
    let claims: auth::Claims = state.authenticator.validate_token(token).map_err(|e| {
        tracing::warn!("JWT validation failed: {}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid or expired token"
            })),
        )
            .into_response()
    })?;

    // Extract user ID from claims
    let user_id_str = claims.sub.as_ref().ok_or_else(|| {
        tracing::error!("Missing 'sub' claim in token");
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid token format"
            })),
        )
            .into_response()
    })?;

    let user_id = UserId::from_string(user_id_str).map_err(|e| {
        tracing::error!("Failed to parse user ID from token: {}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid token format"
            })),
        )
            .into_response()
    })?;

    // Extract username from claims
    let username = claims.username().unwrap_or_else(|| "unknown".to_string());

    // Add authenticated user info to request extensions
    req.extensions_mut()
        .insert(AuthenticatedUser { user_id, username });

    Ok(next.run(req).await)
}

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
