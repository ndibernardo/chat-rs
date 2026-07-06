use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Extension;
use serde::Deserialize;

use crate::domain::channel::models::ChannelId;
use crate::domain::channel::ports::ChannelService;
use crate::domain::message::models::Limit;
use crate::domain::message::ports::MessageService;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::MessageResponseData;
use crate::inbound::http::router::AppState;
use crate::inbound::middleware::AuthenticatedUser;

#[derive(Debug, Deserialize)]
pub struct MessageQuery {
    limit: Option<i32>,
    before: Option<String>, // ISO 8601 timestamp
}

pub async fn get_channel_messages(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthenticatedUser>,
    Path(channel_id): Path<String>,
    Query(params): Query<MessageQuery>,
) -> Result<ApiSuccess<Vec<MessageResponseData>>, ApiError> {
    let channel_id =
        ChannelId::from_string(&channel_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let channel = state
        .channel_service
        .get_channel(channel_id)
        .await
        .map_err(ApiError::from)?;

    let membership = channel.membership_of(auth_user.user_id)?;

    let limit = params
        .limit
        .map(Limit::new)
        .transpose()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
        .unwrap_or_default();
    let before = params
        .before
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| ApiError::BadRequest(format!("Invalid 'before' cursor: {}", e)))
        })
        .transpose()?;

    state
        .message_service
        .get_channel_messages(membership, limit, before)
        .await
        .map_err(ApiError::from)
        .map(|messages| {
            let message_data: Vec<MessageResponseData> =
                messages.iter().map(|m| m.into()).collect();
            ApiSuccess::new(StatusCode::OK, message_data)
        })
}
