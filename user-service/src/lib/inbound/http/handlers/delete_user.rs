use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;

use crate::domain::user::models::UserId;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::router::AppState;
use crate::user::errors::UserError;
use crate::user::ports::UserServicePort;

pub async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiSuccess<()>, ApiError> {
    // Parse user ID
    let user_id = UserId::from_string(&id).map_err(|e| UserError::from(e))?;

    state
        .user_service
        .delete_user(&user_id)
        .await
        .map_err(|e| ApiError::from(e))
        .map(|_| ApiSuccess::new(StatusCode::NO_CONTENT, ()))
}
