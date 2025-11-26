use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;

use crate::domain::channel::models::ChannelId;
use crate::domain::channel::ports::ChannelServicePort;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::CreateChannelResponseData;
use crate::inbound::http::router::AppState;

pub async fn get_channel(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<ApiSuccess<CreateChannelResponseData>, ApiError> {
    let channel_id =
        ChannelId::from_string(&channel_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    state
        .channel_service
        .get_channel(channel_id)
        .await
        .map_err(ApiError::from)
        .map(|ref channel| ApiSuccess::new(StatusCode::OK, channel.into()))
}
