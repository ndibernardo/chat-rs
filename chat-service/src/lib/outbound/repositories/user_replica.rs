use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports::UserReplicaRepository;

/// PostgreSQL implementation of UserReplicaRepository.
///
/// Stores denormalized user data from user-service events in a local replica table.
/// This enables fast read-path queries without calling user-service gRPC.
pub struct PostgresUserReplicaRepository {
    pool: PgPool,
}

impl PostgresUserReplicaRepository {
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
impl UserReplicaRepository for PostgresUserReplicaRepository {
    async fn upsert(&self, user: User) -> Result<(), String> {
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
            user.id.as_uuid(),
            user.username.as_str(),
            user.created_at,
            user.updated_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to upsert user replica: {}", e))?;

        tracing::debug!("User {} upserted in replica", user.id);
        Ok(())
    }

    async fn delete(&self, user_id: UserId) -> Result<(), String> {
        let result = sqlx::query!(
            r#"
            DELETE FROM user_replica
            WHERE id = $1
            "#,
            user_id.as_uuid(),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to delete user from replica: {}", e))?;

        if result.rows_affected() == 0 {
            tracing::warn!("User {} not found in replica for deletion", user_id);
        } else {
            tracing::debug!("User {} deleted from replica", user_id);
        }

        Ok(())
    }

    async fn get(&self, user_id: UserId) -> Result<Option<User>, String> {
        let record = sqlx::query!(
            r#"
            SELECT id, username, created_at, updated_at
            FROM user_replica
            WHERE id = $1
            "#,
            user_id.as_uuid(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Failed to get user from replica: {}", e))?;

        Ok(record.map(|r| {
            let username = Username::new(r.username)
                .expect("Invalid username in database - should never happen");
            User {
                id: UserId(r.id),
                username,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        }))
    }

    async fn get_many(&self, user_ids: &[UserId]) -> Result<Vec<User>, String> {
        let uuids: Vec<uuid::Uuid> = user_ids.iter().map(|id| *id.as_uuid()).collect();

        let records = sqlx::query!(
            r#"
            SELECT id, username, created_at, updated_at
            FROM user_replica
            WHERE id = ANY($1)
            "#,
            &uuids[..],
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to get users from replica: {}", e))?;

        Ok(records
            .into_iter()
            .map(|r| {
                let username = Username::new(r.username)
                    .expect("Invalid username in database - should never happen");
                User {
                    id: UserId(r.id),
                    username,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                }
            })
            .collect())
    }
}
