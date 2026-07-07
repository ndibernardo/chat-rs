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
    /// Builds the channel lazily: the actual TCP/HTTP2 connection is deferred
    /// to the first RPC call, so chat-service can start even if user-service
    /// is temporarily down (the replica-first design already tolerates a
    /// stale/missing row via `ReplicaWithFallback`) instead of failing to boot.
    pub async fn new(url: &str) -> Result<Self, Error> {
        let endpoint = tonic::transport::Endpoint::from_shared(url.to_string())?;
        let client = ProtoUserServiceClient::new(endpoint.connect_lazy());
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
