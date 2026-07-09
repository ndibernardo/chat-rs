use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use super::outbox;
use super::outbox::OutboxEvent;
use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserUpdatedEvent;
use crate::domain::user::models::EmailAddress;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports;
use crate::outbound::kafka::envelope::SCHEMA_USER_V1;
use crate::outbound::kafka::messages::UserEventMessage;
use crate::user::errors::UserError;

pub struct UserRepository {
    pool: PgPool,
    /// Topic stamped on every outbox row this repository enqueues, so the
    /// row records where the relay will actually publish it.
    outbox_topic: String,
}

/// Row shape shared by every query that selects a full `users` row.
struct UserRow {
    id: uuid::Uuid,
    username: String,
    email: String,
    password_hash: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = UserError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        Ok(User::new(
            UserId::from_uuid(row.id),
            Username::new(row.username)?,
            EmailAddress::new(row.email)?,
            row.password_hash,
            row.created_at,
        ))
    }
}

impl UserRepository {
    pub fn new(pool: PgPool, outbox_topic: String) -> Self {
        Self { pool, outbox_topic }
    }

    /// Serializes `event` as the tagged `UserEventMessage` wire shape and
    /// enqueues it in the outbox within `tx`, aggregated under `user_id`.
    async fn enqueue_outbox<E>(
        &self,
        tx: &mut sqlx::PgConnection,
        user_id: Uuid,
        event: E,
    ) -> Result<(), UserError>
    where
        E: Into<UserEventMessage>,
    {
        let message: UserEventMessage = event.into();
        let payload = serde_json::to_value(&message).map_err(|e| {
            UserError::DatabaseError(format!("Failed to serialize outbox event: {e}"))
        })?;

        outbox::enqueue(
            tx,
            OutboxEvent {
                event_id: Uuid::new_v4(),
                aggregate_id: user_id,
                topic: self.outbox_topic.clone(),
                schema: SCHEMA_USER_V1.to_string(),
                payload,
            },
        )
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))
    }
}

#[async_trait]
impl ports::UserRepository for UserRepository {
    async fn create(&self, user: User, event: &UserCreatedEvent) -> Result<User, UserError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        sqlx::query!(
            r#"
            INSERT INTO users (id, username, email, password_hash, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            user.id().value(),
            user.username().as_str(),
            user.email().as_str(),
            user.password_hash(),
            user.created_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    if db_err.constraint() == Some("users_username_key") {
                        return UserError::UsernameAlreadyExists(
                            user.username().as_str().to_string(),
                        );
                    }
                    if db_err.constraint() == Some("users_email_key") {
                        return UserError::EmailAlreadyExists(user.email().as_str().to_string());
                    }
                }
            }
            UserError::DatabaseError(e.to_string())
        })?;

        self.enqueue_outbox(&mut tx, user.id().value(), event.clone())
            .await?;

        tx.commit()
            .await
            .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        Ok(user)
    }

    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, UserError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"
            SELECT id, username, email, password_hash, created_at
            FROM users
            WHERE id = $1
            "#,
            id.value(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        row.map(User::try_from).transpose()
    }

    async fn find_by_username(&self, username: &Username) -> Result<Option<User>, UserError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"
            SELECT id, username, email, password_hash, created_at
            FROM users
            WHERE username = $1
            "#,
            username.as_str(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        row.map(User::try_from).transpose()
    }

    async fn find_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>, UserError> {
        let uuids: Vec<_> = ids.iter().map(|id| id.value()).collect();

        let rows = sqlx::query_as!(
            UserRow,
            r#"
            SELECT id, username, email, password_hash, created_at
            FROM users
            WHERE id = ANY($1)
            "#,
            &uuids
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        rows.into_iter().map(User::try_from).collect()
    }

    async fn update(&self, user: User, event: &UserUpdatedEvent) -> Result<User, UserError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        let result = sqlx::query!(
            r#"
            UPDATE users
            SET username = $2, email = $3, password_hash = $4
            WHERE id = $1
            "#,
            user.id().value(),
            user.username().as_str(),
            user.email().as_str(),
            user.password_hash()
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    if db_err.constraint() == Some("users_username_key") {
                        return UserError::UsernameAlreadyExists(
                            user.username().as_str().to_string(),
                        );
                    }
                    if db_err.constraint() == Some("users_email_key") {
                        return UserError::EmailAlreadyExists(user.email().as_str().to_string());
                    }
                }
            }
            UserError::DatabaseError(e.to_string())
        })?;

        if result.rows_affected() == 0 {
            return Err(UserError::NotFound(user.id()));
        }

        self.enqueue_outbox(&mut tx, user.id().value(), event.clone())
            .await?;

        tx.commit()
            .await
            .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        Ok(user)
    }

    async fn delete(&self, id: &UserId, event: &UserDeletedEvent) -> Result<(), UserError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        let result = sqlx::query!(
            r#"
            DELETE FROM users
            WHERE id = $1
            "#,
            id.value(),
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(UserError::NotFound(*id));
        }

        self.enqueue_outbox(&mut tx, id.value(), event.clone())
            .await?;

        tx.commit()
            .await
            .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}
