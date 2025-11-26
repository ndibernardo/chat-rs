use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserUpdatedEvent;
use crate::domain::user::models::CreateUserCommand;
use crate::domain::user::models::UpdateUserCommand;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::user::errors::UserError;
use crate::user::ports::EventPublisher;
use crate::user::ports::UserRepository;
use crate::user::ports::UserServicePort;

/// Domain service implementation for user operations.
///
/// Concrete implementation of UserServicePort with dependency injection.
pub struct UserService<UR, EP>
where
    UR: UserRepository,
    EP: EventPublisher,
{
    repository: Arc<UR>,
    event_publisher: Arc<EP>,
    password_hasher: auth::PasswordHasher,
}

impl<UR, EP> UserService<UR, EP>
where
    UR: UserRepository,
    EP: EventPublisher,
{
    /// Create a new user service with injected dependencies.
    ///
    /// # Arguments
    /// * `repository` - User persistence implementation
    /// * `event_publisher` - Domain event publishing implementation
    ///
    /// # Returns
    /// Configured user service instance
    pub fn new(repository: Arc<UR>, event_publisher: Arc<EP>) -> Self {
        Self {
            repository,
            event_publisher,
            password_hasher: auth::PasswordHasher::new(),
        }
    }
}

#[async_trait]
impl<UR, EP> UserServicePort for UserService<UR, EP>
where
    UR: UserRepository,
    EP: EventPublisher,
{
    async fn create_user(&self, command: CreateUserCommand) -> Result<User, UserError> {
        // Hash password using auth library
        let password_hash = self
            .password_hasher
            .hash(&command.password)
            .map_err(|e| UserError::Unknown(format!("Password hashing failed: {}", e)))?;

        let user = User {
            id: UserId::new(),
            username: command.username,
            email: command.email,
            password_hash,
            created_at: Utc::now(),
        };

        let created_user = self.repository.create(user).await?;

        let event = UserCreatedEvent::new(&created_user);
        if let Err(e) = &self.event_publisher.publish_user_created(&event).await {
            tracing::error!(
                "Failed to publish UserCreated event for user {}: {}",
                created_user.id,
                e
            );
        }

        Ok(created_user)
    }

    async fn get_user(&self, id: &UserId) -> Result<User, UserError> {
        self.repository
            .find_by_id(id)
            .await?
            .ok_or(UserError::NotFound(id.to_string()))
    }

    async fn get_user_by_username(&self, username: &Username) -> Result<User, UserError> {
        self.repository
            .find_by_username(username)
            .await?
            .ok_or(UserError::NotFoundByUsername(username.to_string()))
    }

    async fn get_users_by_ids(&self, user_ids: &[UserId]) -> Result<Vec<User>, UserError> {
        self.repository.find_by_ids(user_ids).await
    }

    async fn update_user(
        &self,
        id: &UserId,
        command: UpdateUserCommand,
    ) -> Result<User, UserError> {
        let mut user = self
            .repository
            .find_by_id(id)
            .await?
            .ok_or(UserError::NotFound(id.to_string()))?;

        if let Some(new_username) = command.username {
            user.username = new_username;
        }

        if let Some(new_email) = command.email {
            user.email = new_email;
        }

        if let Some(new_password) = command.password {
            user.password_hash = self
                .password_hasher
                .hash(&new_password)
                .map_err(|e| UserError::Unknown(format!("Password hashing failed: {}", e)))?;
        }

        let updated_user = self.repository.update(user).await?;

        let event = UserUpdatedEvent::new(&updated_user);
        if let Err(e) = &self.event_publisher.publish_user_updated(&event).await {
            tracing::error!(
                "Failed to publish UserUpdated event for user {}: {}",
                updated_user.id,
                e
            );
        }

        Ok(updated_user)
    }

    async fn delete_user(&self, id: &UserId) -> Result<(), UserError> {
        self.repository.delete(id).await?;

        let event = UserDeletedEvent::new(id.to_string());
        if let Err(e) = &self.event_publisher.publish_user_deleted(&event).await {
            tracing::error!("Failed to publish UserDeleted event for user {}: {}", id, e);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mockall::mock;
    use mockall::predicate::*;

    use super::*;
    use crate::domain::user::models::EmailAddress;
    use crate::domain::user::models::Username;
    use crate::user::errors::EventPublisherError;

    // Define mocks in the test module using mockall
    mock! {
        pub TestUserRepository {}

        #[async_trait]
        impl UserRepository for TestUserRepository {
            async fn create(&self, user: User) -> Result<User, UserError>;
            async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, UserError>;
            async fn find_by_username(&self, username: &Username) -> Result<Option<User>, UserError>;
            async fn find_by_email(&self, email: &str) -> Result<Option<User>, UserError>;
            async fn list_all(&self) -> Result<Vec<User>, UserError>;
            async fn find_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>, UserError>;
            async fn update(&self, user: User) -> Result<User, UserError>;
            async fn delete(&self, id: &UserId) -> Result<(), UserError>;
        }
    }

    mock! {
        pub TestEventPublisher {}

        #[async_trait]
        impl EventPublisher for TestEventPublisher {
            async fn publish_user_created(&self, event: &UserCreatedEvent) -> Result<(), EventPublisherError>;
            async fn publish_user_updated(&self, event: &UserUpdatedEvent) -> Result<(), EventPublisherError>;
            async fn publish_user_deleted(&self, event: &UserDeletedEvent) -> Result<(), EventPublisherError>;
        }
    }

    #[tokio::test]
    async fn test_create_user_success() {
        let mut repository = MockTestUserRepository::new();
        let mut event_publisher = MockTestEventPublisher::new();

        // Set up mock expectations
        repository
            .expect_create()
            .withf(|user| {
                user.username.as_str() == "testuser"
                    && user.email.as_str() == "test@example.com"
                    && user.password_hash.starts_with("$argon2")
            })
            .times(1)
            .returning(|user| Ok(user));

        event_publisher
            .expect_publish_user_created()
            .times(1)
            .returning(|_| Ok(()));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let command = CreateUserCommand {
            username: Username::new("testuser".to_string()).unwrap(),
            email: EmailAddress::new("test@example.com".to_string()).unwrap(),
            password: "password123".to_string(),
        };

        let result = service.create_user(command).await;
        assert!(result.is_ok());

        let user = result.unwrap();
        assert_eq!(user.username.as_str(), "testuser");
        assert_eq!(user.email.as_str(), "test@example.com");
        // Password is hashed with real Argon2
        assert!(user.password_hash.starts_with("$argon2"));
    }

    #[tokio::test]
    async fn test_create_user_duplicate_username() {
        let mut repository = MockTestUserRepository::new();
        let mut event_publisher = MockTestEventPublisher::new();

        repository.expect_create().times(1).returning(|user| {
            Err(UserError::UsernameAlreadyExists(
                user.username.as_str().to_string(),
            ))
        });

        event_publisher.expect_publish_user_created().times(0);

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let command = CreateUserCommand {
            username: Username::new("testuser".to_string()).unwrap(),
            email: EmailAddress::new("test2@example.com".to_string()).unwrap(),
            password: "password456".to_string(),
        };

        let result = service.create_user(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UserError::UsernameAlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn test_create_user_duplicate_email() {
        let mut repository = MockTestUserRepository::new();
        let mut event_publisher = MockTestEventPublisher::new();

        repository.expect_create().times(1).returning(|user| {
            Err(UserError::EmailAlreadyExists(
                user.email.as_str().to_string(),
            ))
        });

        event_publisher.expect_publish_user_created().times(0);

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let command = CreateUserCommand {
            username: Username::new("user2".to_string()).unwrap(),
            email: EmailAddress::new("test@example.com".to_string()).unwrap(),
            password: "password456".to_string(),
        };

        let result = service.create_user(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UserError::EmailAlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn test_get_user_success() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let expected_user = User {
            id: user_id,
            username: Username::new("testuser".to_string()).unwrap(),
            email: EmailAddress::new("test@example.com".to_string()).unwrap(),
            password_hash: "$argon2id$test_hash".to_string(),
            created_at: Utc::now(),
        };

        let returned_user = expected_user.clone();
        repository
            .expect_find_by_id()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(move |_| Ok(Some(returned_user.clone())));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let result = service.get_user(&user_id).await;
        assert!(result.is_ok());

        let user = result.unwrap();
        assert_eq!(user.id, user_id);
        assert_eq!(user.username.as_str(), "testuser");
    }

    #[tokio::test]
    async fn test_get_user_not_found() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let non_existent_id = UserId::new();
        let result = service.get_user(&non_existent_id).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), UserError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_user_by_username_success() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        let username = Username::new("testuser".to_string()).unwrap();
        let expected_user = User {
            id: UserId::new(),
            username: username.clone(),
            email: EmailAddress::new("test@example.com".to_string()).unwrap(),
            password_hash: "$argon2id$test_hash".to_string(),
            created_at: Utc::now(),
        };

        let returned_user = expected_user.clone();
        let username_clone = username.clone();
        repository
            .expect_find_by_username()
            .withf(move |u| u == &username_clone)
            .times(1)
            .returning(move |_| Ok(Some(returned_user.clone())));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let result = service.get_user_by_username(&username).await;
        assert!(result.is_ok());

        let user = result.unwrap();
        assert_eq!(user.username.as_str(), "testuser");
    }

    #[tokio::test]
    async fn test_get_user_by_username_not_found() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        repository
            .expect_find_by_username()
            .times(1)
            .returning(|_| Ok(None));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let username = Username::new("nonexistent".to_string()).unwrap();
        let result = service.get_user_by_username(&username).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UserError::NotFoundByUsername(_)
        ));
    }

    #[tokio::test]
    async fn test_get_users_by_ids() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_ids: Vec<UserId> = vec![UserId::new(), UserId::new(), UserId::new()];
        let expected_users: Vec<User> = user_ids
            .iter()
            .enumerate()
            .map(|(i, id)| User {
                id: *id,
                username: Username::new(format!("user{}", i + 1)).unwrap(),
                email: EmailAddress::new(format!("user{}@example.com", i + 1)).unwrap(),
                password_hash: "$argon2id$test_hash".to_string(),
                created_at: Utc::now(),
            })
            .collect();

        let returned_users = expected_users.clone();
        repository
            .expect_find_by_ids()
            .times(1)
            .returning(move |_| Ok(returned_users.clone()));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let result = service.get_users_by_ids(&user_ids).await;
        assert!(result.is_ok());

        let users = result.unwrap();
        assert_eq!(users.len(), 3);
    }

    #[tokio::test]
    async fn test_get_users_by_ids_partial_match() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        let existing_user_id = UserId::new();
        let existing_user = User {
            id: existing_user_id,
            username: Username::new("user1".to_string()).unwrap(),
            email: EmailAddress::new("user1@example.com".to_string()).unwrap(),
            password_hash: "$argon2id$test_hash".to_string(),
            created_at: Utc::now(),
        };

        let returned_user = existing_user.clone();
        repository
            .expect_find_by_ids()
            .times(1)
            .returning(move |_| Ok(vec![returned_user.clone()]));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));
        let ids = vec![existing_user_id, UserId::new()];
        let result = service.get_users_by_ids(&ids).await;

        assert!(result.is_ok());
        let users = result.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].id, existing_user_id);
    }

    #[tokio::test]
    async fn test_update_user_success() {
        let mut repository = MockTestUserRepository::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let existing_user = User {
            id: user_id,
            username: Username::new("olduser".to_string()).unwrap(),
            email: EmailAddress::new("old@example.com".to_string()).unwrap(),
            password_hash: "$argon2id$old_hash".to_string(),
            created_at: Utc::now(),
        };

        // Mock find_by_id to return existing user
        let returned_user = existing_user.clone();
        repository
            .expect_find_by_id()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(move |_| Ok(Some(returned_user.clone())));

        // Mock update to return updated user
        repository
            .expect_update()
            .withf(|user| {
                user.username.as_str() == "newuser"
                    && user.email.as_str() == "new@example.com"
                    && user.password_hash.starts_with("$argon2")
            })
            .times(1)
            .returning(|user| Ok(user));

        event_publisher
            .expect_publish_user_updated()
            .times(1)
            .returning(|_| Ok(()));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let command = UpdateUserCommand {
            username: Some(Username::new("newuser".to_string()).unwrap()),
            email: Some(EmailAddress::new("new@example.com".to_string()).unwrap()),
            password: Some("newpassword".to_string()),
        };

        let result = service.update_user(&user_id, command).await;
        assert!(result.is_ok());

        let updated_user = result.unwrap();
        assert_eq!(updated_user.username.as_str(), "newuser");
        assert_eq!(updated_user.email.as_str(), "new@example.com");
    }

    #[tokio::test]
    async fn test_update_user_not_found() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let user_id = UserId::new();
        let command = UpdateUserCommand {
            username: Some(Username::new("newuser".to_string()).unwrap()),
            email: None,
            password: None,
        };

        let result = service.update_user(&user_id, command).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), UserError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_user_success() {
        let mut repository = MockTestUserRepository::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();

        repository
            .expect_delete()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(|_| Ok(()));

        event_publisher
            .expect_publish_user_deleted()
            .times(1)
            .returning(|_| Ok(()));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let result = service.delete_user(&user_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_user_not_found() {
        let mut repository = MockTestUserRepository::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();

        repository
            .expect_delete()
            .times(1)
            .returning(move |_| Err(UserError::NotFound(user_id.to_string())));

        let service = UserService::new(Arc::new(repository), Arc::new(event_publisher));

        let result = service.delete_user(&user_id).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), UserError::NotFound(_)));
    }
}
