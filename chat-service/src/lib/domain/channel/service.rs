use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use super::errors::ChannelError;
use super::models::Channel;
use super::models::ChannelId;
use super::models::CreateChannelCommand;
use super::models::DirectChannel;
use super::models::PrivateChannel;
use super::models::PublicChannel;
use super::ports::ChannelRepository;
use super::ports::ChannelServicePort;
use crate::domain::user::models::UserId;

/// Concrete implementation of ChannelServicePort.
///
/// Manages channel creation, retrieval, and deletion with eventual consistency.
/// Generic over repository for testability.
pub struct ChannelService<CR>
where
    CR: ChannelRepository,
{
    channel_repository: Arc<CR>,
}

impl<CR> ChannelService<CR>
where
    CR: ChannelRepository,
{
    pub fn new(channel_repository: Arc<CR>) -> Self {
        Self { channel_repository }
    }
}

#[async_trait]
impl<CR> ChannelServicePort for ChannelService<CR>
where
    CR: ChannelRepository + 'static,
{
    async fn create_channel(
        &self,
        command: CreateChannelCommand,
        created_by: UserId,
    ) -> Result<Channel, ChannelError> {
        let channel = match command {
            CreateChannelCommand::Public { name, description } => Channel::Public(PublicChannel {
                id: ChannelId::new(),
                name,
                description,
                created_by,
                created_at: Utc::now(),
            }),
            CreateChannelCommand::Private {
                name,
                description,
                members,
            } => Channel::Private(PrivateChannel {
                id: ChannelId::new(),
                name,
                description,
                created_by,
                created_at: Utc::now(),
                members,
            }),
            CreateChannelCommand::Direct { participant_id } => Channel::Direct(DirectChannel {
                id: ChannelId::new(),
                created_by,
                created_at: Utc::now(),
                participants: [created_by, participant_id],
            }),
        };

        self.channel_repository.create(channel).await
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
    use mockall::mock;
    use mockall::predicate::*;

    use super::*;
    use crate::ChannelName;

    mock! {
        pub TestChannelRepository {}

        #[async_trait]
        impl ChannelRepository for TestChannelRepository {
            async fn create(&self, channel: Channel) -> Result<Channel, ChannelError>;
            async fn find_by_id(&self, id: ChannelId) -> Result<Option<Channel>, ChannelError>;
            async fn find_public_channels(&self) -> Result<Vec<Channel>, ChannelError>;
            async fn find_by_user(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError>;
            async fn delete(&self, id: ChannelId) -> Result<(), ChannelError>;
        }
    }

    #[tokio::test]
    async fn test_create_public_channel_success() {
        let mut channel_repository = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        channel_repository
            .expect_create()
            .withf(move |channel| {
                matches!(channel, Channel::Public(_))
                    && channel.name().unwrap().as_str() == "general"
                    && channel.created_by() == creator_id
            })
            .times(1)
            .returning(|channel| Ok(channel));

        let service = ChannelService::new(Arc::new(channel_repository));

        let req = CreateChannelCommand::Public {
            name: ChannelName::new("general".to_string()).unwrap(),
            description: Some("General discussion".to_string()),
        };

        let result = service.create_channel(req, creator_id).await;
        assert!(result.is_ok());

        let channel = result.unwrap();
        assert!(matches!(channel, Channel::Public(_)));
        assert_eq!(channel.name().unwrap().as_str(), "general");
        assert_eq!(channel.created_by(), creator_id);
    }

    #[tokio::test]
    async fn test_create_private_channel_success() {
        let mut channel_repository = MockTestChannelRepository::new();

        let creator_id = UserId::new();
        let member1_id = UserId::new();
        let member2_id = UserId::new();

        channel_repository
            .expect_create()
            .withf(move |channel| {
                matches!(channel, Channel::Private(_))
                    && channel.name().unwrap().as_str() == "private-team"
                    && channel.created_by() == creator_id
            })
            .times(1)
            .returning(|channel| Ok(channel));

        let service = ChannelService::new(Arc::new(channel_repository));

        let req = CreateChannelCommand::Private {
            name: ChannelName::new("private-team".to_string()).unwrap(),
            description: Some("Team channel".to_string()),
            members: vec![member1_id, member2_id],
        };

        let result = service.create_channel(req, creator_id).await;
        assert!(result.is_ok());

        let channel = result.unwrap();
        assert!(matches!(channel, Channel::Private(_)));
        assert_eq!(channel.name().unwrap().as_str(), "private-team");
    }

    #[tokio::test]
    async fn test_create_direct_channel_success() {
        let mut channel_repository = MockTestChannelRepository::new();

        let user1_id = UserId::new();
        let user2_id = UserId::new();

        channel_repository
            .expect_create()
            .withf(move |channel| {
                matches!(channel, Channel::Direct(_)) && channel.created_by() == user1_id
            })
            .times(1)
            .returning(|channel| Ok(channel));

        let service = ChannelService::new(Arc::new(channel_repository));

        let req = CreateChannelCommand::Direct {
            participant_id: user2_id,
        };

        let result = service.create_channel(req, user1_id).await;
        assert!(result.is_ok());

        let channel = result.unwrap();
        assert!(matches!(channel, Channel::Direct(_)));
        assert_eq!(channel.created_by(), user1_id);
    }

    #[tokio::test]
    async fn test_get_channel_success() {
        let mut channel_repository = MockTestChannelRepository::new();

        let creator_id = UserId::new();
        let channel_id = ChannelId::new();

        let expected_channel = Channel::Public(PublicChannel {
            id: channel_id,
            name: ChannelName::new("general".to_string()).unwrap(),
            description: None,
            created_by: creator_id,
            created_at: Utc::now(),
        });

        let returned_channel = expected_channel.clone();
        channel_repository
            .expect_find_by_id()
            .withf(move |id| *id == channel_id)
            .times(1)
            .returning(move |_| Ok(Some(returned_channel.clone())));

        let service = ChannelService::new(Arc::new(channel_repository));

        let result = service.get_channel(channel_id).await;
        assert!(result.is_ok());

        let channel = result.unwrap();
        assert_eq!(channel.id(), channel_id);
    }

    #[tokio::test]
    async fn test_get_channel_not_found() {
        let mut channel_repository = MockTestChannelRepository::new();

        let non_existent_id = ChannelId::new();

        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = ChannelService::new(Arc::new(channel_repository));

        let result = service.get_channel(non_existent_id).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChannelError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_list_public_channels() {
        let mut channel_repository = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        let expected_channels = vec![
            Channel::Public(PublicChannel {
                id: ChannelId::new(),
                name: ChannelName::new("channel1".to_string()).unwrap(),
                description: None,
                created_by: creator_id,
                created_at: Utc::now(),
            }),
            Channel::Public(PublicChannel {
                id: ChannelId::new(),
                name: ChannelName::new("channel2".to_string()).unwrap(),
                description: None,
                created_by: creator_id,
                created_at: Utc::now(),
            }),
            Channel::Public(PublicChannel {
                id: ChannelId::new(),
                name: ChannelName::new("channel3".to_string()).unwrap(),
                description: None,
                created_by: creator_id,
                created_at: Utc::now(),
            }),
        ];

        let returned_channels = expected_channels.clone();
        channel_repository
            .expect_find_public_channels()
            .times(1)
            .returning(move || Ok(returned_channels.clone()));

        let service = ChannelService::new(Arc::new(channel_repository));

        let result = service.list_public_channels().await;
        assert!(result.is_ok());

        let channels = result.unwrap();
        assert_eq!(channels.len(), 3);
        assert!(channels.iter().all(|c| matches!(c, Channel::Public(_))));
    }

    #[tokio::test]
    async fn test_list_user_channels() {
        let mut channel_repository = MockTestChannelRepository::new();

        let user1_id = UserId::new();
        let user2_id = UserId::new();

        let expected_channels = vec![
            Channel::Public(PublicChannel {
                id: ChannelId::new(),
                name: ChannelName::new("user1-public".to_string()).unwrap(),
                description: None,
                created_by: user1_id,
                created_at: Utc::now(),
            }),
            Channel::Direct(DirectChannel {
                id: ChannelId::new(),
                created_by: user1_id,
                created_at: Utc::now(),
                participants: [user1_id, user2_id],
            }),
        ];

        let returned_channels = expected_channels.clone();
        channel_repository
            .expect_find_by_user()
            .withf(move |user_id| *user_id == user1_id)
            .times(1)
            .returning(move |_| Ok(returned_channels.clone()));

        let service = ChannelService::new(Arc::new(channel_repository));

        let result = service.list_user_channels(user1_id).await;
        assert!(result.is_ok());

        let channels = result.unwrap();
        assert_eq!(channels.len(), 2);
    }

    #[tokio::test]
    async fn test_create_channel_invalid_name() {
        let mut channel_repository = MockTestChannelRepository::new();

        let creator_id = UserId::new();

        let invalid_name = ChannelName::new("".to_string());
        assert!(invalid_name.is_err(), "Empty channel name should fail");

        channel_repository
            .expect_create()
            .times(1)
            .returning(|channel| Ok(channel));

        let service = ChannelService::new(Arc::new(channel_repository));

        let valid_name = ChannelName::new("valid-channel".to_string()).unwrap();
        let cmd = CreateChannelCommand::Public {
            name: valid_name,
            description: None,
        };
        let result = service.create_channel(cmd, creator_id).await;
        assert!(result.is_ok(), "Valid channel name should succeed");
    }
}
