use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::ApiError;
use super::ApiSuccess;
use crate::domain::user::models::User;
use crate::domain::user::ports::UserServicePort;
use crate::inbound::http::router::AppState;
use crate::user::errors::UserError;
use crate::user::models::Username;

pub async fn authenticate(
    State(state): State<AppState>,
    Json(body): Json<AuthenticateRequestBody>,
) -> Result<ApiSuccess<AuthenticateResponseData>, ApiError> {
    // Parse and validate username
    let username = Username::new(body.username)
        .map_err(|_| ApiError::Unauthorized("Invalid credentials".to_string()))?;

    // Get user from database
    let user = state
        .user_service
        .get_user_by_username(&username)
        .await
        .map_err(|e| match e {
            UserError::NotFoundByUsername(_) => {
                ApiError::Unauthorized("Invalid credentials".to_string())
            }
            _ => ApiError::from(e),
        })?;

    // Create JWT claims (from auth library)
    let claims = auth::Claims::for_user(
        user.id.clone(),
        user.username.as_str().to_string(),
        state.jwt_expiration_hours,
    );

    // Verify password and generate token
    let result = state
        .authenticator
        .authenticate(&body.password, &user.password_hash, &claims)
        .map_err(|e| match e {
            auth::AuthenticationError::InvalidCredentials => {
                ApiError::Unauthorized("Invalid credentials".to_string())
            }
            auth::AuthenticationError::PasswordError(err) => {
                ApiError::InternalServerError(format!("Password verification failed: {}", err))
            }
            auth::AuthenticationError::JwtError(err) => {
                ApiError::InternalServerError(format!("Token generation failed: {}", err))
            }
        })?;

    Ok(ApiSuccess::new(
        StatusCode::OK,
        AuthenticateResponseData {
            user: (&user).into(),
            token: result.access_token,
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AuthenticateRequestBody {
    username: String,
    password: String,
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
            id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            created_at: user.created_at,
        }
    }
}
