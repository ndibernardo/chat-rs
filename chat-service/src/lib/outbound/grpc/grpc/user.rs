use anyhow::Error;
use tonic::transport::Channel;

use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports::UserServicePort;
use crate::proto::user_service_client::UserServiceClient;
use crate::proto::GetUserRequest;

pub struct GrpcUserServiceClient {
    client: UserServiceClient<Channel>,
}

impl GrpcUserServiceClient {
    pub async fn new(url: &str) -> Result<Self, Error> {
        let client = UserServiceClient::connect(url.to_string()).await?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl UserServicePort for GrpcUserServiceClient {
    async fn get_user(&self, user_id: UserId) -> Result<Option<User>, String> {
        let request = tonic::Request::new(GetUserRequest {
            user_id: user_id.to_string(),
        });

        let mut client = self.client.clone();
        let response = client
            .get_user(request)
            .await
            .map_err(|e| format!("gRPC error: {}", e))?;

        let result = response.into_inner();

        match result.result {
            Some(crate::proto::get_user_response::Result::User(user)) => {
                let user_id =
                    UserId::from_string(&user.id).map_err(|e| format!("Invalid user ID: {}", e))?;

                let username = Username::new(user.username)
                    .map_err(|e| format!("Invalid username from gRPC: {}", e))?;

                //@TODO remove created_at, updated_at

                Ok(Some(User {
                    id: user_id,
                    username,
                    created_at: Default::default(),
                    updated_at: Default::default(),
                }))
            }
            Some(crate::proto::get_user_response::Result::Error(err)) => Err(err),
            None => Ok(None),
        }
    }
}
