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
use crate::user::ports::PasswordHasher;
use crate::user::ports::UserRepository;
use crate::user::ports::UserService;

/// Domain service for user operations.
pub struct Service<UR, PH>
where
    UR: UserRepository,
    PH: PasswordHasher,
{
    repository: Arc<UR>,
    password_hasher: Arc<PH>,
}

impl<UR, PH> Service<UR, PH>
where
    UR: UserRepository,
    PH: PasswordHasher,
{
    /// Create a new user service with injected dependencies.
    pub fn new(repository: Arc<UR>, password_hasher: Arc<PH>) -> Self {
        Self {
            repository,
            password_hasher,
        }
    }
}

#[async_trait]
impl<UR, PH> UserService for Service<UR, PH>
where
    UR: UserRepository,
    PH: PasswordHasher,
{
    async fn create_user(&self, command: CreateUserCommand) -> Result<User, UserError> {
        let password_hash = self
            .password_hasher
            .hash(command.password.as_str())
            .await
            .map_err(UserError::Password)?;

        let user = User::new(
            UserId::new(),
            command.username,
            command.email,
            password_hash,
            Utc::now(),
        );

        let event = UserCreatedEvent::new(&user);
        self.repository.create(user, &event).await
    }

    async fn get_user(&self, id: &UserId) -> Result<User, UserError> {
        self.repository
            .find_by_id(id)
            .await?
            .ok_or(UserError::NotFound(*id))
    }

    async fn get_user_by_username(&self, username: &Username) -> Result<User, UserError> {
        self.repository
            .find_by_username(username)
            .await?
            .ok_or_else(|| UserError::NotFoundByUsername(username.clone()))
    }

    async fn verify_credentials(
        &self,
        username: &Username,
        password: &str,
    ) -> Result<User, UserError> {
        let user = self
            .repository
            .find_by_username(username)
            .await?
            .ok_or(UserError::InvalidCredentials)?;

        let is_valid = self
            .password_hasher
            .verify(password, user.password_hash())
            .await
            .map_err(UserError::Password)?;

        if !is_valid {
            return Err(UserError::InvalidCredentials);
        }

        Ok(user)
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
            .ok_or(UserError::NotFound(*id))?;

        let new_password_hash = match command.password {
            Some(p) => Some(
                self.password_hasher
                    .hash(p.as_str())
                    .await
                    .map_err(UserError::Password)?,
            ),
            None => None,
        };

        user.apply_update(command.username, command.email, new_password_hash);

        let event = UserUpdatedEvent::new(&user);
        self.repository.update(user, &event).await
    }

    async fn delete_user(&self, id: &UserId) -> Result<(), UserError> {
        let event = UserDeletedEvent::new(id);
        self.repository.delete(id, &event).await
    }
}

#[cfg(test)]
mod tests {
    use mockall::mock;
    use mockall::predicate::*;

    use super::*;
    use crate::domain::user::models::EmailAddress;
    use crate::domain::user::models::Password;
    use crate::domain::user::models::Username;

    mock! {
        pub TestUserRepository {}

        #[async_trait]
        impl UserRepository for TestUserRepository {
            async fn create(&self, user: User, event: &UserCreatedEvent) -> Result<User, UserError>;
            async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, UserError>;
            async fn find_by_username(&self, username: &Username) -> Result<Option<User>, UserError>;
            async fn find_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>, UserError>;
            async fn update(&self, user: User, event: &UserUpdatedEvent) -> Result<User, UserError>;
            async fn delete(&self, id: &UserId, event: &UserDeletedEvent) -> Result<(), UserError>;
        }
    }

    mock! {
        pub TestPasswordHasher {}

        #[async_trait]
        impl PasswordHasher for TestPasswordHasher {
            async fn hash(&self, password: &str) -> Result<String, crate::user::errors::PasswordError>;
            async fn verify(&self, password: &str, hash: &str) -> Result<bool, crate::user::errors::PasswordError>;
        }
    }

    fn stub_password_hasher() -> MockTestPasswordHasher {
        let mut password_hasher = MockTestPasswordHasher::new();
        password_hasher
            .expect_hash()
            .returning(|_| Ok("$argon2id$test_hash".to_string()));
        password_hasher
    }

    fn miles_davis() -> User {
        User::new(
            UserId::new(),
            Username::new("miles-davis").unwrap(),
            EmailAddress::new("miles.davis@example.com").unwrap(),
            "$argon2id$test_hash".to_string(),
            Utc::now(),
        )
    }

    #[tokio::test]
    async fn create_user_returns_created_user() {
        let mut repository = MockTestUserRepository::new();

        repository
            .expect_create()
            .withf(|user, _event| {
                user.username().as_str() == "miles-davis"
                    && user.email().as_str() == "miles.davis@example.com"
                    && user.password_hash().starts_with("$argon2")
            })
            .times(1)
            .returning(|user, _event| Ok(user));

        let service = Service::new(Arc::new(repository), Arc::new(stub_password_hasher()));

        let command = CreateUserCommand {
            username: Username::new("miles-davis").unwrap(),
            email: EmailAddress::new("miles.davis@example.com").unwrap(),
            password: Password::new("K1nd-0f-Blue_1959!"),
        };

        let result = service.create_user(command).await;
        assert!(result.is_ok());

        let user = result.unwrap();
        assert_eq!(user.username().as_str(), "miles-davis");
        assert_eq!(user.email().as_str(), "miles.davis@example.com");
        assert!(user.password_hash().starts_with("$argon2"));
    }

    #[tokio::test]
    async fn create_user_returns_error_for_duplicate_username() {
        let mut repository = MockTestUserRepository::new();

        repository
            .expect_create()
            .times(1)
            .returning(|user, _event| {
                Err(UserError::UsernameAlreadyExists(
                    user.username().as_str().to_string(),
                ))
            });

        let service = Service::new(Arc::new(repository), Arc::new(stub_password_hasher()));

        let command = CreateUserCommand {
            username: Username::new("miles-davis").unwrap(),
            email: EmailAddress::new("john.coltrane@example.com").unwrap(),
            password: Password::new("G1ant-St3ps_1960!"),
        };

        let result = service.create_user(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UserError::UsernameAlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn create_user_returns_error_for_duplicate_email() {
        let mut repository = MockTestUserRepository::new();

        repository
            .expect_create()
            .times(1)
            .returning(|user, _event| {
                Err(UserError::EmailAlreadyExists(
                    user.email().as_str().to_string(),
                ))
            });

        let service = Service::new(Arc::new(repository), Arc::new(stub_password_hasher()));

        let command = CreateUserCommand {
            username: Username::new("john-coltrane").unwrap(),
            email: EmailAddress::new("miles.davis@example.com").unwrap(),
            password: Password::new("G1ant-St3ps_1960!"),
        };

        let result = service.create_user(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UserError::EmailAlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn get_user_returns_user_by_id() {
        let mut repository = MockTestUserRepository::new();

        let user = miles_davis();
        let user_id = user.id();

        let returned_user = user.clone();
        repository
            .expect_find_by_id()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(move |_| Ok(Some(returned_user.clone())));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let result = service.get_user(&user_id).await;
        assert!(result.is_ok());

        let found = result.unwrap();
        assert_eq!(found.id(), user_id);
        assert_eq!(found.username().as_str(), "miles-davis");
    }

    #[tokio::test]
    async fn get_user_returns_not_found_for_missing_id() {
        let mut repository = MockTestUserRepository::new();

        repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let result = service.get_user(&UserId::new()).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), UserError::NotFound(_)));
    }

    #[tokio::test]
    async fn get_user_by_username_returns_user() {
        let mut repository = MockTestUserRepository::new();

        let username = Username::new("miles-davis").unwrap();
        let user = miles_davis();

        let returned_user = user.clone();
        let username_clone = username.clone();
        repository
            .expect_find_by_username()
            .withf(move |u| u == &username_clone)
            .times(1)
            .returning(move |_| Ok(Some(returned_user.clone())));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let result = service.get_user_by_username(&username).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().username().as_str(), "miles-davis");
    }

    #[tokio::test]
    async fn get_user_by_username_returns_not_found() {
        let mut repository = MockTestUserRepository::new();

        repository
            .expect_find_by_username()
            .times(1)
            .returning(|_| Ok(None));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let username = Username::new("ravi-shankar").unwrap();
        let result = service.get_user_by_username(&username).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UserError::NotFoundByUsername(_)
        ));
    }

    #[tokio::test]
    async fn verify_credentials_returns_user_for_correct_password() {
        let mut repository = MockTestUserRepository::new();
        let mut password_hasher = MockTestPasswordHasher::new();

        let username = Username::new("miles-davis").unwrap();
        let user = miles_davis();

        let returned_user = user.clone();
        repository
            .expect_find_by_username()
            .times(1)
            .returning(move |_| Ok(Some(returned_user.clone())));

        password_hasher
            .expect_verify()
            .withf(|password, hash| {
                password == "K1nd-0f-Blue_1959!" && hash == "$argon2id$test_hash"
            })
            .times(1)
            .returning(|_, _| Ok(true));

        let service = Service::new(Arc::new(repository), Arc::new(password_hasher));

        let result = service
            .verify_credentials(&username, "K1nd-0f-Blue_1959!")
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().username().as_str(), "miles-davis");
    }

    #[tokio::test]
    async fn verify_credentials_rejects_wrong_password() {
        let mut repository = MockTestUserRepository::new();
        let mut password_hasher = MockTestPasswordHasher::new();

        let username = Username::new("miles-davis").unwrap();
        let user = miles_davis();

        repository
            .expect_find_by_username()
            .times(1)
            .returning(move |_| Ok(Some(user.clone())));

        password_hasher
            .expect_verify()
            .times(1)
            .returning(|_, _| Ok(false));

        let service = Service::new(Arc::new(repository), Arc::new(password_hasher));

        let result = service
            .verify_credentials(&username, "wrong-password")
            .await;
        assert!(matches!(result, Err(UserError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn verify_credentials_rejects_unknown_username_without_calling_hasher() {
        let mut repository = MockTestUserRepository::new();
        let mut password_hasher = MockTestPasswordHasher::new();

        repository
            .expect_find_by_username()
            .times(1)
            .returning(|_| Ok(None));
        password_hasher.expect_verify().times(0);

        let service = Service::new(Arc::new(repository), Arc::new(password_hasher));

        let username = Username::new("ravi-shankar").unwrap();
        let result = service.verify_credentials(&username, "irrelevant").await;
        assert!(matches!(result, Err(UserError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn get_users_by_ids_returns_all_matching_users() {
        let mut repository = MockTestUserRepository::new();

        let user_ids: Vec<UserId> = vec![UserId::new(), UserId::new(), UserId::new()];
        let names = ["john-coltrane", "kim-gordon", "nina-simone"];
        let emails = [
            "john.coltrane@example.com",
            "kim.gordon@example.com",
            "nina.simone@example.com",
        ];
        let users: Vec<User> = user_ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                User::new(
                    *id,
                    Username::new(names[i]).unwrap(),
                    EmailAddress::new(emails[i]).unwrap(),
                    "$argon2id$test_hash".to_string(),
                    Utc::now(),
                )
            })
            .collect();

        let returned_users = users.clone();
        repository
            .expect_find_by_ids()
            .times(1)
            .returning(move |_| Ok(returned_users.clone()));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let result = service.get_users_by_ids(&user_ids).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn get_users_by_ids_returns_only_found_users() {
        let mut repository = MockTestUserRepository::new();

        let existing_user = User::new(
            UserId::new(),
            Username::new("thelonious-monk").unwrap(),
            EmailAddress::new("thelonious.monk@example.com").unwrap(),
            "$argon2id$test_hash".to_string(),
            Utc::now(),
        );
        let existing_id = existing_user.id();

        let returned_user = existing_user.clone();
        repository
            .expect_find_by_ids()
            .times(1)
            .returning(move |_| Ok(vec![returned_user.clone()]));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );
        let result = service
            .get_users_by_ids(&[existing_id, UserId::new()])
            .await;

        assert!(result.is_ok());
        let users = result.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].id(), existing_id);
    }

    #[tokio::test]
    async fn update_user_returns_updated_user() {
        let mut repository = MockTestUserRepository::new();

        let existing = User::new(
            UserId::new(),
            Username::new("charlie-parker").unwrap(),
            EmailAddress::new("charlie.parker@example.com").unwrap(),
            "$argon2id$old_hash".to_string(),
            Utc::now(),
        );
        let user_id = existing.id();

        let returned = existing.clone();
        repository
            .expect_find_by_id()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(move |_| Ok(Some(returned.clone())));

        repository
            .expect_update()
            .withf(|user, _event| {
                user.username().as_str() == "bird-parker"
                    && user.email().as_str() == "bird.parker@example.com"
                    && user.password_hash().starts_with("$argon2")
            })
            .times(1)
            .returning(|user, _event| Ok(user));

        let service = Service::new(Arc::new(repository), Arc::new(stub_password_hasher()));

        let command = UpdateUserCommand {
            username: Some(Username::new("bird-parker").unwrap()),
            email: Some(EmailAddress::new("bird.parker@example.com").unwrap()),
            password: Some(Password::new("0mnivore_Jazz_1945!")),
        };

        let result = service.update_user(&user_id, command).await;
        assert!(result.is_ok());

        let updated = result.unwrap();
        assert_eq!(updated.username().as_str(), "bird-parker");
        assert_eq!(updated.email().as_str(), "bird.parker@example.com");
    }

    #[tokio::test]
    async fn update_user_returns_not_found_for_missing_id() {
        let mut repository = MockTestUserRepository::new();

        repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let command = UpdateUserCommand {
            username: Some(Username::new("ella-fitzgerald").unwrap()),
            email: None,
            password: None,
        };

        let result = service.update_user(&UserId::new(), command).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), UserError::NotFound(_)));
    }

    #[tokio::test]
    async fn delete_user_succeeds_for_existing_user() {
        let mut repository = MockTestUserRepository::new();

        let user_id = UserId::new();

        repository
            .expect_delete()
            .withf(move |id, _event| *id == user_id)
            .times(1)
            .returning(|_, _event| Ok(()));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let result = service.delete_user(&user_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_user_returns_not_found_for_missing_id() {
        let mut repository = MockTestUserRepository::new();

        let user_id = UserId::new();

        repository
            .expect_delete()
            .times(1)
            .returning(move |_, _event| Err(UserError::NotFound(user_id)));

        let service = Service::new(
            Arc::new(repository),
            Arc::new(MockTestPasswordHasher::new()),
        );

        let result = service.delete_user(&user_id).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), UserError::NotFound(_)));
    }
}
