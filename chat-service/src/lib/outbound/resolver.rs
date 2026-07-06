use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::user::errors::UserError;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::ports::RemoteUserLookup;
use crate::domain::user::ports::UserReplicaRepository;
use crate::domain::user::ports::UserResolver;

/// Resolves users from the local replica, falling back to user-service via gRPC when not found.
///
/// Checks the replica first to avoid unnecessary network calls. Falls back to gRPC
/// when a user is missing from the replica (e.g., the event has not yet arrived or was dropped).
pub struct ReplicaWithFallback<R, C>
where
    R: UserReplicaRepository,
    C: RemoteUserLookup,
{
    replica: Arc<R>,
    client: Arc<C>,
}

impl<R, C> ReplicaWithFallback<R, C>
where
    R: UserReplicaRepository,
    C: RemoteUserLookup,
{
    /// Create a new resolver that checks the replica before calling user-service.
    ///
    /// # Arguments
    /// * `replica` - Local user read-model repository
    /// * `client` - gRPC client for user-service (fallback)
    ///
    /// # Returns
    /// Configured resolver
    pub fn new(replica: Arc<R>, client: Arc<C>) -> Self {
        Self { replica, client }
    }
}

#[async_trait]
impl<R, C> UserResolver for ReplicaWithFallback<R, C>
where
    R: UserReplicaRepository + 'static,
    C: RemoteUserLookup + 'static,
{
    async fn resolve(&self, user_id: UserId) -> Result<Option<User>, UserError> {
        if let Some(user) = self.replica.get(user_id).await? {
            return Ok(Some(user));
        }
        tracing::debug!(
            user_id = %user_id,
            "User not in replica, falling back to user-service"
        );
        self.client.get_user(user_id).await
    }
}
