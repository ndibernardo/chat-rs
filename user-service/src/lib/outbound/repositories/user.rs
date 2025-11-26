use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::user::models::EmailAddress;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports::UserRepository;
use crate::user::errors::UserError;

pub struct PostgresUserRepository {
    pool: PgPool,
}

impl PostgresUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn create(&self, user: User) -> Result<User, UserError> {
        sqlx::query!(
            r#"
            INSERT INTO users (id, username, email, password_hash, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            user.id.0,
            user.username.as_str(),
            user.email.as_str(),
            user.password_hash,
            user.created_at
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    if db_err.constraint() == Some("users_username_key") {
                        return UserError::UsernameAlreadyExists(
                            user.username.as_str().to_string(),
                        );
                    }
                    if db_err.constraint() == Some("users_email_key") {
                        return UserError::EmailAlreadyExists(user.email.as_str().to_string());
                    }
                }
            }
            UserError::DatabaseError(e.to_string())
        })?;

        Ok(user)
    }

    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, UserError> {
        let row = sqlx::query!(
            r#"
            SELECT id, username, email, password_hash, created_at
            FROM users
            WHERE id = $1
            "#,
            id.0,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(User {
                id: UserId(r.id),
                username: Username::new(r.username)?,
                email: EmailAddress::new(r.email)?,
                password_hash: r.password_hash,
                created_at: r.created_at,
            })),
            None => Ok(None),
        }
    }

    async fn find_by_username(&self, username: &Username) -> Result<Option<User>, UserError> {
        let row = sqlx::query!(
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

        match row {
            Some(r) => Ok(Some(User {
                id: UserId(r.id),
                username: Username::new(r.username)?,
                email: EmailAddress::new(r.email)?,
                password_hash: r.password_hash,
                created_at: r.created_at,
            })),
            None => Ok(None),
        }
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, UserError> {
        let row = sqlx::query!(
            r#"
            SELECT id, username, email, password_hash, created_at
            FROM users
            WHERE email = $1
            "#,
            email,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(User {
                id: UserId(r.id),
                username: Username::new(r.username)?,
                email: EmailAddress::new(r.email)?,
                password_hash: r.password_hash,
                created_at: r.created_at,
            })),
            None => Ok(None),
        }
    }

    async fn list_all(&self) -> Result<Vec<User>, UserError> {
        let rows = sqlx::query!(
            r#"
            SELECT id, username, email, password_hash, created_at
            FROM users
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                Ok(User {
                    id: UserId(r.id),
                    username: Username::new(r.username)?,
                    email: EmailAddress::new(r.email)?,
                    password_hash: r.password_hash,
                    created_at: r.created_at,
                })
            })
            .collect()
    }

    async fn find_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>, UserError> {
        let uuids: Vec<_> = ids.iter().map(|id| id.0).collect();

        let rows = sqlx::query!(
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

        rows.into_iter()
            .map(|r| {
                Ok(User {
                    id: UserId(r.id),
                    username: Username::new(r.username)?,
                    email: EmailAddress::new(r.email)?,
                    password_hash: r.password_hash,
                    created_at: r.created_at,
                })
            })
            .collect()
    }

    async fn update(&self, user: User) -> Result<User, UserError> {
        let result = sqlx::query!(
            r#"
            UPDATE users
            SET username = $2, email = $3, password_hash = $4
            WHERE id = $1
            "#,
            user.id.0,
            user.username.as_str(),
            user.email.as_str(),
            user.password_hash
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            //TODO: check with claude
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    if db_err.constraint() == Some("users_username_key") {
                        return UserError::UsernameAlreadyExists(
                            user.username.as_str().to_string(),
                        );
                    }
                    if db_err.constraint() == Some("users_email_key") {
                        return UserError::EmailAlreadyExists(user.email.as_str().to_string());
                    }
                }
            }
            UserError::DatabaseError(e.to_string())
        })?;

        if result.rows_affected() == 0 {
            return Err(UserError::NotFound(user.id.to_string()));
        }

        Ok(user)
    }

    async fn delete(&self, id: &UserId) -> Result<(), UserError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM users
            WHERE id = $1
            "#,
            id.0,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| UserError::DatabaseError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(UserError::NotFound(id.to_string()));
        }

        Ok(())
    }
}
