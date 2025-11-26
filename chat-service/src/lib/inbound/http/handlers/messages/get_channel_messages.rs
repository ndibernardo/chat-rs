use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::domain::channel::models::ChannelId;
use crate::domain::message::ports::MessageServicePort;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::MessageResponseData;
use crate::inbound::http::router::AppState;

#[derive(Debug, Deserialize)]
pub struct MessageQuery {
    limit: Option<i32>,
    before: Option<String>, // ISO 8601 timestamp
}

pub async fn get_channel_messages(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(params): Query<MessageQuery>,
) -> Result<ApiSuccess<Vec<MessageResponseData>>, ApiError> {
    let channel_id =
        ChannelId::from_string(&channel_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let limit = params.limit.unwrap_or(50);
    let before = params
        .before
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    state
        .message_service
        .get_channel_messages(channel_id, limit, before)
        .await
        .map_err(ApiError::from)
        .map(|messages| {
            let message_data: Vec<MessageResponseData> =
                messages.iter().map(|m| m.into()).collect();
            ApiSuccess::new(StatusCode::OK, message_data)
        })
}
