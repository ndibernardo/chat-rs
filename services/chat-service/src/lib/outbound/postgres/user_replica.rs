use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::user::errors::UserError;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports;

/// PostgreSQL implementation of UserReplicaRepository.
///
/// Stores denormalized user data from user-service events in a local replica table.
/// This enables fast read-path queries without calling user-service gRPC.
pub struct UserReplicaRepository {
    pool: PgPool,
}

/// Row shape shared by every query that selects a full `user_replica` row.
struct UserReplicaRow {
    id: uuid::Uuid,
    username: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<UserReplicaRow> for User {
    fn from(row: UserReplicaRow) -> Self {
        let username = Username::new(row.username)
            .expect("Invalid username in database - should never happen");
        User::new(
            UserId::from_uuid(row.id),
            username,
            row.created_at,
            row.updated_at,
        )
    }
}

impl UserReplicaRepository {
    /// Create a new PostgreSQL user replica repository.
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    ///
    /// # Returns
    /// Configured repository instance
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ports::UserReplicaRepository for UserReplicaRepository {
    async fn upsert(&self, user: User) -> Result<(), UserError> {
        sqlx::query!(
            r#"
            INSERT INTO user_replica (id, username, created_at, updated_at, synced_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (id)
            DO UPDATE SET
                username = EXCLUDED.username,
                updated_at = EXCLUDED.updated_at,
                synced_at = NOW()
            "#,
            user.id().into_uuid(),
            user.username().as_str(),
            user.created_at(),
            user.updated_at(),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(format!("Failed to upsert user replica: {}", e)))?;

        tracing::debug!("User {} upserted in replica", user.id());
        Ok(())
    }

    async fn delete(&self, user_id: UserId) -> Result<(), UserError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM user_replica
            WHERE id = $1
            "#,
            user_id.as_uuid(),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            UserError::DatabaseError(format!("Failed to delete user from replica: {}", e))
        })?;

        if result.rows_affected() == 0 {
            tracing::warn!("User {} not found in replica for deletion", user_id);
        } else {
            tracing::debug!("User {} deleted from replica", user_id);
        }

        Ok(())
    }

    async fn get(&self, user_id: UserId) -> Result<Option<User>, UserError> {
        let record = sqlx::query_as!(
            UserReplicaRow,
            r#"
            SELECT id, username, created_at, updated_at
            FROM user_replica
            WHERE id = $1
            "#,
            user_id.as_uuid(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(format!("Failed to get user from replica: {}", e)))?;

        Ok(record.map(User::from))
    }

    async fn get_many(&self, user_ids: &[UserId]) -> Result<Vec<User>, UserError> {
        let uuids: Vec<uuid::Uuid> = user_ids.iter().map(|id| *id.as_uuid()).collect();

        let records = sqlx::query_as!(
            UserReplicaRow,
            r#"
            SELECT id, username, created_at, updated_at
            FROM user_replica
            WHERE id = ANY($1)
            "#,
            &uuids[..],
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            UserError::DatabaseError(format!("Failed to get users from replica: {}", e))
        })?;

        Ok(records.into_iter().map(User::from).collect())
    }
}
