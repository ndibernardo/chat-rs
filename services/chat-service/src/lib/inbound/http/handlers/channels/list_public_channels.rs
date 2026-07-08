use axum::extract::State;
use axum::http::StatusCode;

use crate::domain::channel::ports::ChannelService;
use crate::domain::message::ports::MessageService;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::ChannelResponseData;
use crate::inbound::http::router::AppState;

pub async fn list_public_channels<CS, MS>(
    State(state): State<AppState<CS, MS>>,
) -> Result<ApiSuccess<Vec<ChannelResponseData>>, ApiError>
where
    CS: ChannelService,
    MS: MessageService,
{
    state
        .channel_service
        .list_public_channels()
        .await
        .map_err(ApiError::from)
        .map(|channels| {
            let channel_data: Vec<ChannelResponseData> =
                channels.iter().map(|c| c.into()).collect();
            ApiSuccess::new(StatusCode::OK, channel_data)
        })
}
