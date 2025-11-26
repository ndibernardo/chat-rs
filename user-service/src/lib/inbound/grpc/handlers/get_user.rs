use std::sync::Arc;

use tonic::Status;

use crate::domain::user::models::UserId;
use crate::domain::user::ports::UserServicePort;
use crate::domain::user::service::UserService;
use crate::outbound::events::KafkaEventProducer;
use crate::outbound::repositories::user::PostgresUserRepository;
use crate::proto::GetUserRequest;
use crate::proto::GetUserResponse;
use crate::proto::User as ProtoUser;

pub async fn get_user(
    service: Arc<UserService<PostgresUserRepository, KafkaEventProducer>>,
    request: GetUserRequest,
) -> Result<GetUserResponse, Status> {
    let user_id = UserId::from_string(&request.user_id)
        .map_err(|e| Status::invalid_argument(format!("Invalid user ID: {}", e)))?;

    match service.get_user(&user_id).await {
        Ok(user) => {
            let proto_user: ProtoUser = user.into();
            Ok(GetUserResponse {
                result: Some(crate::proto::get_user_response::Result::User(proto_user)),
            })
        }
        Err(e) => Ok(GetUserResponse {
            result: Some(crate::proto::get_user_response::Result::Error(
                e.to_string(),
            )),
        }),
    }
}
