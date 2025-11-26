use axum::extract::State;
use axum::http::StatusCode;

use crate::domain::channel::ports::ChannelServicePort;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::CreateChannelResponseData;
use crate::inbound::http::router::AppState;

pub async fn list_public_channels(
    State(state): State<AppState>,
) -> Result<ApiSuccess<Vec<CreateChannelResponseData>>, ApiError> {
    state
        .channel_service
        .list_public_channels()
        .await
        .map_err(ApiError::from)
        .map(|channels| {
            let channel_data: Vec<CreateChannelResponseData> =
                channels.iter().map(|c| c.into()).collect();
            ApiSuccess::new(StatusCode::OK, channel_data)
        })
}
