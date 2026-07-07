use anyhow::Error;
use tonic::transport::Channel;

use crate::domain::user::errors::UserError;
use crate::domain::user::models::ResolvedUser;
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
    async fn get_user(&self, user_id: UserId) -> Result<Option<ResolvedUser>, UserError> {
        let request = tonic::Request::new(GetUserRequest {
            user_id: user_id.to_string(),
        });

        let mut client = self.client.clone();
        let response = match client.get_user(request).await {
            Ok(response) => response,
            Err(status) if status.code() == tonic::Code::NotFound => return Ok(None),
            Err(status) => return Err(UserError::RemoteError(format!("gRPC error: {}", status))),
        };

        let user = response.into_inner().user.ok_or_else(|| {
            UserError::RemoteError("gRPC response missing user".to_string())
        })?;

        let user_id = UserId::from_string(&user.id)?;
        let username = Username::new(user.username)?;

        Ok(Some(ResolvedUser::new(user_id, username)))
    }
}
