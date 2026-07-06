use anyhow::Error;
use tonic::transport::Channel;

use crate::domain::user::errors::UserError;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports::RemoteUserLookup;
use crate::proto::user_service_client::UserServiceClient as ProtoUserServiceClient;
use crate::proto::GetUserRequest;

pub struct UserServiceClient {
    client: ProtoUserServiceClient<Channel>,
}

impl UserServiceClient {
    pub async fn new(url: &str) -> Result<Self, Error> {
        let client = ProtoUserServiceClient::connect(url.to_string()).await?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl RemoteUserLookup for UserServiceClient {
    async fn get_user(&self, user_id: UserId) -> Result<Option<User>, UserError> {
        let request = tonic::Request::new(GetUserRequest {
            user_id: user_id.to_string(),
        });

        let mut client = self.client.clone();
        let response = client
            .get_user(request)
            .await
            .map_err(|e| UserError::RemoteError(format!("gRPC error: {}", e)))?;

        let result = response.into_inner();

        match result.result {
            Some(crate::proto::get_user_response::Result::User(user)) => {
                let user_id = UserId::from_string(&user.id)?;
                let username = Username::new(user.username)?;

                Ok(Some(User::new(
                    user_id,
                    username,
                    Default::default(),
                    Default::default(),
                )))
            }
            Some(crate::proto::get_user_response::Result::Error(err)) => {
                Err(UserError::RemoteError(err))
            }
            None => Ok(None),
        }
    }
}
