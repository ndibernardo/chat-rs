use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use super::ApiError;
use super::ApiSuccess;
use crate::domain::user::models::CreateUserCommand;
use crate::domain::user::models::EmailAddress;
use crate::domain::user::models::User;
use crate::domain::user::models::Username;
use crate::domain::user::ports::UserServicePort;
use crate::inbound::http::router::AppState;
use crate::user::errors::EmailError;
use crate::user::errors::UsernameError;

pub async fn create_user(
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<ApiSuccess<CreateUserResponseData>, ApiError> {
    state
        .user_service
        .create_user(body.try_into_command()?)
        .await
        .map_err(ApiError::from)
        .map(|ref user| ApiSuccess::new(StatusCode::CREATED, user.into()))
}

/// HTTP request body for creating a user (raw JSON)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CreateUserRequest {
    username: String,
    email_address: String,
    password: String,
}

#[derive(Debug, Clone, Error)]
enum ParseCreateUserRequestError {
    #[error("Invalid username: {0}")]
    Username(#[from] UsernameError),

    #[error("Invalid email: {0}")]
    Email(#[from] EmailError),
}

impl CreateUserRequest {
    fn try_into_command(self) -> Result<CreateUserCommand, ParseCreateUserRequestError> {
        let username = Username::new(self.username)?;
        let email_address = EmailAddress::new(self.email_address)?;
        let password = self.password;
        Ok(CreateUserCommand::new(username, email_address, password))
    }
}

impl From<ParseCreateUserRequestError> for ApiError {
    fn from(err: ParseCreateUserRequestError) -> Self {
        ApiError::UnprocessableEntity(err.to_string())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateUserResponseData {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl From<&User> for CreateUserResponseData {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            created_at: user.created_at,
        }
    }
}
