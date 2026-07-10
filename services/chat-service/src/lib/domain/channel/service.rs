use std::sync::Arc;

use async_trait::async_trait;

use super::errors::ChannelError;
use super::events::ChannelCreatedEvent;
use super::models::Channel;
use super::models::ChannelId;
use super::models::CreateChannelCommand;
use super::ports::ChannelRepository;
use super::ports::ChannelService;
use crate::domain::user::models::UserId;

pub struct Service<CR>
where
    CR: ChannelRepository,
{
    channel_repository: Arc<CR>,
}

impl<CR> Service<CR>
where
    CR: ChannelRepository,
{
    pub fn new(channel_repository: Arc<CR>) -> Self {
        Self { channel_repository }
    }
}

#[async_trait]
impl<CR> ChannelService for Service<CR>
where
    CR: ChannelRepository + 'static,
{
    async fn create_channel(
        &self,
        command: CreateChannelCommand,
        created_by: UserId,
    ) -> Result<Channel, ChannelError> {
        let channel = match command {
            CreateChannelCommand::Public { name, description } => {
                Channel::new_public(name, description, created_by)
            }
            CreateChannelCommand::Private {
                name,
                description,
                members,
            } => Channel::new_private(name, description, members, created_by),
            CreateChannelCommand::Direct { participant_id } => {
                Channel::new_direct(created_by, participant_id)?
            }
        };

        let event = ChannelCreatedEvent::new(&channel);
        self.channel_repository.create(channel, &event).await
    }

    async fn get_channel(&self, id: ChannelId) -> Result<Channel, ChannelError> {
        self.channel_repository
            .find_by_id(id)
            .await?
            .ok_or(ChannelError::NotFound(id))
    }

    async fn list_public_channels(&self) -> Result<Vec<Channel>, ChannelError> {
        self.channel_repository.find_public_channels().await
    }

    async fn list_user_channels(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError> {
        self.channel_repository.find_by_user(user_id).await
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::Utc;
    use mockall::mock;
    use mockall::predicate::*;

    use super::*;
    use crate::domain::channel::models::ChannelName;

    mock! {
        pub TestChannelRepository {}

        #[async_trait]
        impl ChannelRepository for TestChannelRepository {
            async fn create(&self, channel: Channel, event: &ChannelCreatedEvent) -> Result<Channel, ChannelError>;
            async fn find_by_id(&self, id: ChannelId) -> Result<Option<Channel>, ChannelError>;
            async fn find_public_channels(&self) -> Result<Vec<Channel>, ChannelError>;
            async fn find_by_user(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError>;
            async fn delete(&self, id: ChannelId) -> Result<(), ChannelError>;
            async fn remove_user_memberships(&self, user_id: UserId) -> Result<(), ChannelError>;
            async fn deactivate_direct_channels_of(
                &self,
                user_id: UserId,
                deactivated_at: chrono::DateTime<chrono::Utc>,
            ) -> Result<(), ChannelError>;
        }
    }

    fn make_service(repo: MockTestChannelRepository) -> Service<MockTestChannelRepository> {
        Service::new(Arc::new(repo))
    }

    #[tokio::test]
    async fn create_channel_returns_public_channel() {
        let mut repo = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        repo.expect_create()
            .withf(move |channel, _event| {
                matches!(channel, Channel::Public(_))
                    && channel.name().unwrap().as_str() == "engineering"
                    && channel.created_by() == creator_id
            })
            .times(1)
            .returning(|channel, _event| Ok(channel));

        let service = make_service(repo);

        let result = service
            .create_channel(
                CreateChannelCommand::Public {
                    name: ChannelName::new("engineering").unwrap(),
                    description: Some("Engineering team".to_string()),
                },
                creator_id,
            )
            .await;

        assert!(result.is_ok());
        let channel = result.unwrap();
        assert!(matches!(channel, Channel::Public(_)));
        assert_eq!(channel.name().unwrap().as_str(), "engineering");
    }

    #[tokio::test]
    async fn create_channel_returns_private_channel_with_members() {
        let mut repo = MockTestChannelRepository::new();

        let creator_id = UserId::new();
        let member1_id = UserId::new();
        let member2_id = UserId::new();

        repo.expect_create()
            .withf(move |channel, _event| {
                matches!(channel, Channel::Private(_))
                    && channel.name().unwrap().as_str() == "private-team"
                    && channel.created_by() == creator_id
                    && channel.members().contains(&creator_id)
                    && channel.members().contains(&member1_id)
                    && channel.members().contains(&member2_id)
            })
            .times(1)
            .returning(|channel, _event| Ok(channel));

        let service = make_service(repo);

        let result = service
            .create_channel(
                CreateChannelCommand::Private {
                    name: ChannelName::new("private-team").unwrap(),
                    description: Some("Team channel".to_string()),
                    members: vec![member1_id, member2_id],
                },
                creator_id,
            )
            .await;

        assert!(result.is_ok());
        let channel = result.unwrap();
        assert!(matches!(channel, Channel::Private(_)));
        assert_eq!(channel.name().unwrap().as_str(), "private-team");
        assert!(
            channel.members().contains(&creator_id),
            "creator must be a member"
        );
        assert!(channel.members().contains(&member1_id));
        assert!(channel.members().contains(&member2_id));
    }

    #[tokio::test]
    async fn create_private_channel_deduplicates_creator_in_member_list() {
        let mut repo = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        repo.expect_create()
            .withf(move |channel, _event| {
                channel
                    .members()
                    .iter()
                    .filter(|&&m| m == creator_id)
                    .count()
                    == 1
            })
            .times(1)
            .returning(|channel, _event| Ok(channel));

        let service = make_service(repo);

        let result = service
            .create_channel(
                CreateChannelCommand::Private {
                    name: ChannelName::new("engineering").unwrap(),
                    description: None,
                    members: vec![creator_id],
                },
                creator_id,
            )
            .await;

        assert!(result.is_ok());
        let channel = result.unwrap();
        assert!(matches!(channel, Channel::Private(_)));
        assert_eq!(
            channel
                .members()
                .iter()
                .filter(|&&m| m == creator_id)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn create_channel_returns_direct_channel() {
        let mut repo = MockTestChannelRepository::new();

        let user1_id = UserId::new();
        let user2_id = UserId::new();

        repo.expect_create()
            .withf(move |channel, _event| {
                matches!(channel, Channel::Direct(_)) && channel.created_by() == user1_id
            })
            .times(1)
            .returning(|channel, _event| Ok(channel));

        let service = make_service(repo);

        let result = service
            .create_channel(
                CreateChannelCommand::Direct {
                    participant_id: user2_id,
                },
                user1_id,
            )
            .await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Channel::Direct(_)));
    }

    #[tokio::test]
    async fn get_channel_returns_channel_by_id() {
        let mut repo = MockTestChannelRepository::new();

        let creator_id = UserId::new();
        let channel_id = ChannelId::new();

        let expected = Channel::from_public_parts(
            channel_id,
            ChannelName::new("engineering").unwrap(),
            None,
            creator_id,
            Utc::now(),
        );

        let returned = expected.clone();
        repo.expect_find_by_id()
            .withf(move |id| *id == channel_id)
            .times(1)
            .returning(move |_| Ok(Some(returned.clone())));

        let service = make_service(repo);

        let result = service.get_channel(channel_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id(), channel_id);
    }

    #[tokio::test]
    async fn get_channel_returns_not_found_for_missing_id() {
        let mut repo = MockTestChannelRepository::new();

        repo.expect_find_by_id().times(1).returning(|_| Ok(None));

        let service = make_service(repo);

        let result = service.get_channel(ChannelId::new()).await;
        assert!(matches!(result, Err(ChannelError::NotFound(_))));
    }

    #[tokio::test]
    async fn list_public_channels_returns_all_public_channels() {
        let mut repo = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        let channels = vec![
            Channel::from_public_parts(
                ChannelId::new(),
                ChannelName::new("engineering").unwrap(),
                None,
                creator_id,
                Utc::now(),
            ),
            Channel::from_public_parts(
                ChannelId::new(),
                ChannelName::new("product").unwrap(),
                None,
                creator_id,
                Utc::now(),
            ),
        ];

        let returned = channels.clone();
        repo.expect_find_public_channels()
            .times(1)
            .returning(move || Ok(returned.clone()));

        let service = make_service(repo);

        let result = service.list_public_channels().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn create_channel_returns_name_already_exists_for_duplicate_name() {
        let mut repo = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        repo.expect_create()
            .times(1)
            .returning(|_, _event| Err(ChannelError::NameAlreadyExists("engineering".to_string())));

        let service = make_service(repo);

        let result = service
            .create_channel(
                CreateChannelCommand::Public {
                    name: ChannelName::new("engineering").unwrap(),
                    description: None,
                },
                creator_id,
            )
            .await;

        assert!(matches!(result, Err(ChannelError::NameAlreadyExists(_))));
    }
}
