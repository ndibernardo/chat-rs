use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::domain::channel::models::ChannelName;
use crate::domain::channel::models::CreateChannelCommand;
use crate::domain::channel::ports::ChannelService;
use crate::domain::message::ports::MessageService;
use crate::domain::user::models::UserId;
use crate::inbound::http::handlers::ApiError;
use crate::inbound::http::handlers::ApiSuccess;
use crate::inbound::http::handlers::ChannelResponseData;
use crate::inbound::http::handlers::CreateChannelRequest;
use crate::inbound::http::router::AppState;
use web::AuthenticatedUser;

pub async fn create_channel<CS, MS>(
    State(state): State<AppState<CS, MS>>,
    Extension(auth_user): Extension<AuthenticatedUser>,
    Json(req): Json<CreateChannelRequest>,
) -> Result<ApiSuccess<ChannelResponseData>, ApiError>
where
    CS: ChannelService,
    MS: MessageService,
{
    let command = match req {
        CreateChannelRequest::Public { name, description } => {
            let channel_name =
                ChannelName::new(name).map_err(|e| ApiError::UnprocessableEntity(e.to_string()))?;

            CreateChannelCommand::Public {
                name: channel_name,
                description,
            }
        }
        CreateChannelRequest::Private {
            name,
            description,
            members,
        } => {
            let channel_name =
                ChannelName::new(name).map_err(|e| ApiError::UnprocessableEntity(e.to_string()))?;

            // Parse member UUIDs from strings
            let member_ids: Result<Vec<UserId>, _> =
                members.iter().map(|s| UserId::from_string(s)).collect();
            let member_ids = member_ids
                .map_err(|e| ApiError::UnprocessableEntity(format!("Invalid member ID: {}", e)))?;

            CreateChannelCommand::Private {
                name: channel_name,
                description,
                members: member_ids,
            }
        }
        CreateChannelRequest::Direct { participant_id } => {
            let participant_id = UserId::from_string(&participant_id).map_err(|e| {
                ApiError::UnprocessableEntity(format!("Invalid participant ID: {}", e))
            })?;

            CreateChannelCommand::Direct { participant_id }
        }
    };

    state
        .channel_service
        .create_channel(command, UserId::from_uuid(auth_user.user_id))
        .await
        .map_err(ApiError::from)
        .map(|ref channel| ApiSuccess::new(StatusCode::CREATED, channel.into()))
}
