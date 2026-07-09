use std::sync::Arc;

use tonic::Status;

use super::super::proto::GetUserRequest;
use super::super::proto::GetUserResponse;
use crate::domain::user::errors::UserError;
use crate::domain::user::models::UserId;
use crate::domain::user::ports::UserService;
use crate::domain::user::service::Service;
use crate::outbound::argon2::PasswordHasher;
use crate::outbound::postgres::UserRepository;

pub async fn get_user(
    service: Arc<Service<UserRepository, PasswordHasher>>,
    request: GetUserRequest,
) -> Result<GetUserResponse, Status> {
    let user_id = UserId::from_string(&request.user_id)
        .map_err(|e| Status::invalid_argument(format!("Invalid user ID: {}", e)))?;

    match service.get_user(&user_id).await {
        Ok(user) => {
            let proto_user: super::super::proto::User = user.into();
            Ok(GetUserResponse {
                user: Some(proto_user),
            })
        }
        Err(UserError::NotFound(id)) => Err(Status::not_found(format!("User not found: {}", id))),
        Err(e) => Err(Status::internal(e.to_string())),
    }
}
