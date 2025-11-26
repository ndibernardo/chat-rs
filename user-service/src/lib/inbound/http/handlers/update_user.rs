use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use crate::domain::user::models::EmailAddress;
use crate::domain::user::models::UpdateUserCommand;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::router::AppState;
use crate::user::errors::UserError;
use crate::user::ports::UserServicePort;

/// HTTP request body for updating a user (raw JSON)
#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
}

impl UpdateUserRequest {
    fn try_into_command(self) -> Result<UpdateUserCommand, UserError> {
        // Validation happens here - errors are automatically converted via #[from]
        let username = self.username.map(Username::new).transpose()?;

        let email = self.email.map(EmailAddress::new).transpose()?;

        Ok(UpdateUserCommand {
            username,
            email,
            password: self.password,
        })
    }
}

/// Response body for user operations
#[derive(Debug, Serialize, PartialEq)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: String,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            created_at: user.created_at.to_rfc3339(),
        }
    }
}

pub async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<ApiSuccess<UserResponse>, ApiError> {
    // Parse user ID and request at HTTP boundary - errors automatically converted
    let user_id = UserId::from_string(&id).map_err(UserError::from)?;
    let command = req.try_into_command()?;

    state
        .user_service
        .update_user(&user_id, command)
        .await
        .map_err(ApiError::from)
        .map(|user| ApiSuccess::new(StatusCode::OK, user.into()))
}
