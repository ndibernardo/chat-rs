use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::ApiError;
use super::ApiSuccess;
use crate::domain::user::models::Password;
use crate::domain::user::models::User;
use crate::domain::user::ports::UserService;
use crate::inbound::http::router::AppState;
use crate::user::errors::UserError;
use crate::user::models::Username;

pub async fn authenticate(
    State(state): State<AppState>,
    Json(body): Json<AuthenticateRequestBody>,
) -> Result<ApiSuccess<AuthenticateResponseData>, ApiError> {
    let username = Username::new(body.username)
        .map_err(|_| ApiError::Unauthorized("Invalid credentials".to_string()))?;

    let user = state
        .user_service
        .verify_credentials(&username, body.password.as_str())
        .await
        .map_err(|e| match e {
            UserError::InvalidCredentials => {
                ApiError::Unauthorized("Invalid credentials".to_string())
            }
            _ => ApiError::from(e),
        })?;

    let claims = auth::Claims::for_user(
        user.id(),
        user.username().as_str().to_string(),
        state.jwt_expiration_hours,
    );

    let access_token = state
        .authenticator
        .generate_token(&claims)
        .map_err(|e| ApiError::InternalServerError(format!("Token generation failed: {}", e)))?;

    Ok(ApiSuccess::new(
        StatusCode::OK,
        AuthenticateResponseData {
            user: (&user).into(),
            token: access_token,
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AuthenticateRequestBody {
    username: String,
    password: Password,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthenticateResponseData {
    pub user: UserData,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserData {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl From<&User> for UserData {
    fn from(user: &User) -> Self {
        Self {
            id: user.id().to_string(),
            username: user.username().as_str().to_string(),
            email: user.email().as_str().to_string(),
            created_at: user.created_at(),
        }
    }
}
