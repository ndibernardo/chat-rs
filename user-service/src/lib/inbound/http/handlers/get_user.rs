use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;

use super::ApiError;
use super::ApiSuccess;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::ports::UserServicePort;
use crate::inbound::http::router::AppState;

pub async fn get_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<ApiSuccess<GetUserResponseData>, ApiError> {
    let user_id = UserId::from_string(&user_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    state
        .user_service
        .get_user(&user_id)
        .await
        .map_err(ApiError::from)
        .map(|ref user| ApiSuccess::new(StatusCode::OK, user.into()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GetUserResponseData {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl From<&User> for GetUserResponseData {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            created_at: user.created_at,
        }
    }
}
