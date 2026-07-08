use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Extension;

use crate::domain::channel::models::ChannelId;
use crate::domain::channel::ports::ChannelService;
use crate::domain::message::ports::MessageService;
use crate::domain::user::models::UserId;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::ChannelResponseData;
use crate::inbound::http::router::AppState;
use web::AuthenticatedUser;

pub async fn get_channel<CS, MS>(
    State(state): State<AppState<CS, MS>>,
    Extension(auth_user): Extension<AuthenticatedUser>,
    Path(channel_id): Path<String>,
) -> Result<ApiSuccess<ChannelResponseData>, ApiError>
where
    CS: ChannelService,
    MS: MessageService,
{
    let channel_id =
        ChannelId::from_string(&channel_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let channel = state
        .channel_service
        .get_channel(channel_id)
        .await
        .map_err(ApiError::from)?;

    channel.membership_of(UserId::from_uuid(auth_user.user_id))?;

    Ok(ApiSuccess::new(StatusCode::OK, (&channel).into()))
}
