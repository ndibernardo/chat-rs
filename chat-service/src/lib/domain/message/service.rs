use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use super::events::MessageSentEvent;
use super::models::Message;
use super::models::MessageContent;
use super::models::MessageId;
use super::ports::MessageEventPublisher;
use super::ports::MessageRepository;
use super::ports::MessageServicePort;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::ports::ChannelRepository;
use crate::domain::message::errors::MessageError;
use crate::domain::user::models::UserId;
use crate::domain::user::ports::UserServicePort;

/// Concrete implementation of MessageServicePort.
///
/// Manages message creation, retrieval, and event publishing with eventual consistency.
pub struct MessageService<MR, CR, UC, EP>
where
    MR: MessageRepository,
    CR: ChannelRepository,
    UC: UserServicePort,
    EP: MessageEventPublisher,
{
    message_repository: Arc<MR>,
    channel_repository: Arc<CR>,
    user_proxy: Arc<UC>,
    event_publisher: Arc<EP>,
}

impl<MR, CR, UC, EP> MessageService<MR, CR, UC, EP>
where
    MR: MessageRepository,
    CR: ChannelRepository,
    UC: UserServicePort,
    EP: MessageEventPublisher,
{
    /// Create a new message service with injected dependencies.
    ///
    /// # Arguments
    /// * `message_repository` - Message persistence implementation
    /// * `channel_repository` - Channel repository for validation
    /// * `user_proxy` - User service client for future enrichment
    /// * `event_publisher` - Event publisher implementation
    ///
    /// # Returns
    /// Configured message service instance
    pub fn new(
        message_repository: Arc<MR>,
        channel_repository: Arc<CR>,
        user_proxy: Arc<UC>,
        event_publisher: Arc<EP>,
    ) -> Self {
        Self {
            message_repository,
            channel_repository,
            user_proxy,
            event_publisher,
        }
    }
}

#[async_trait]
impl<MR, CR, UC, EP> MessageServicePort for MessageService<MR, CR, UC, EP>
where
    MR: MessageRepository + 'static,
    CR: ChannelRepository + 'static,
    UC: UserServicePort + 'static,
    EP: MessageEventPublisher + 'static,
{
    async fn send_message(
        &self,
        channel_id: ChannelId,
        user_id: UserId,
        content: MessageContent,
    ) -> Result<Message, MessageError> {
        // Verify channel exists
        self.channel_repository
            .find_by_id(channel_id)
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
            .ok_or(MessageError::ChannelNotFound(channel_id))?;

        let message = Message {
            id: MessageId::new_time_based(),
            channel_id,
            user_id,
            content: content.clone(),
            timestamp: Utc::now(),
        };

        // Save message to database
        let saved_message = self.message_repository.create(message).await?;

        // Publish event
        // Event will be published to a topic/shard determined by implementation
        let event = MessageSentEvent::new(&saved_message);

        if let Err(e) = self.event_publisher.publish_message_sent(&event).await {
            tracing::error!("Failed to publish message event: {}", e);
        } else {
            tracing::debug!(
                "Published message event for message {} in channel {}",
                saved_message.id,
                saved_message.channel_id
            );
        }

        Ok(saved_message)
    }

    async fn get_channel_messages(
        &self,
        channel_id: ChannelId,
        limit: i32,
        before: Option<chrono::DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError> {
        self.message_repository
            .find_by_channel(channel_id, limit, before)
            .await
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use mockall::mock;
    use mockall::predicate::*;

    use super::*;
    use crate::domain::channel::errors::ChannelError;
    use crate::domain::channel::models::Channel;
    use crate::domain::channel::models::ChannelName;
    use crate::domain::channel::models::PublicChannel;
    use crate::domain::channel::ports::ChannelRepository;
    use crate::domain::message::events::MessageDeletedEvent;
    use crate::domain::user::models::User;

    mock! {
        pub TestMessageRepository {}

        #[async_trait]
        impl MessageRepository for TestMessageRepository {
            async fn create(&self, message: Message) -> Result<Message, MessageError>;
            async fn find_by_channel(
                &self,
                channel_id: ChannelId,
                limit: i32,
                before: Option<chrono::DateTime<Utc>>,
            ) -> Result<Vec<Message>, MessageError>;
            async fn find_by_user(
                &self,
                user_id: UserId,
                limit: i32,
            ) -> Result<Vec<Message>, MessageError>;
        }
    }

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

    mock! {
        pub TestUserService {}

        #[async_trait]
        impl UserServicePort for TestUserService {
            async fn get_user(&self, user_id: UserId) -> Result<Option<User>, String>;
        }
    }

    mock! {
        pub TestEventPublisher {}

        #[async_trait]
        impl MessageEventPublisher for TestEventPublisher {
            async fn publish_message_sent(
                &self,
                event: &MessageSentEvent,
            ) -> Result<(), crate::domain::errors::EventPublisherError>;

            async fn publish_message_deleted(
                &self,
                event: &MessageDeletedEvent,
            ) -> Result<(), crate::domain::errors::EventPublisherError>;
        }
    }

    #[tokio::test]
    async fn test_send_message_success() {
        let mut message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_client = MockTestUserService::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        // Setup channel
        let channel = Channel::Public(PublicChannel {
            id: channel_id,
            name: ChannelName::new("general".to_string()).unwrap(),
            description: None,
            created_by: user_id,
            created_at: Utc::now(),
        });

        let returned_channel = channel.clone();
        channel_repository
            .expect_find_by_id()
            .withf(move |id| *id == channel_id)
            .times(1)
            .returning(move |_| Ok(Some(returned_channel.clone())));

        message_repository
            .expect_create()
            .withf(move |message| {
                message.channel_id == channel_id
                    && message.user_id == user_id
                    && message.content.as_str() == "Hello, world!"
            })
            .times(1)
            .returning(|message| Ok(message));

        // Expect event to be published
        event_publisher
            .expect_publish_message_sent()
            .times(1)
            .returning(|_| Ok(()));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_client),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("Hello, world!".to_string()).unwrap();

        let result = service.send_message(channel_id, user_id, content).await;
        assert!(result.is_ok());

        let message = result.unwrap();
        assert_eq!(message.channel_id, channel_id);
        assert_eq!(message.user_id, user_id);
        assert_eq!(message.content.as_str(), "Hello, world!");
    }

    #[tokio::test]
    async fn test_send_message_channel_not_found() {
        let message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_client = MockTestUserService::new();

        let user_id = UserId::new();
        let non_existent_channel = ChannelId::new();

        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let event_publisher = MockTestEventPublisher::new();
        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_client),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("Hello".to_string()).unwrap();

        let result = service
            .send_message(non_existent_channel, user_id, content)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MessageError::ChannelNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_send_message_empty_content() {
        let mut message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_client = MockTestUserService::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        let channel = Channel::Public(PublicChannel {
            id: channel_id,
            name: ChannelName::new("general".to_string()).unwrap(),
            description: None,
            created_by: user_id,
            created_at: Utc::now(),
        });

        let returned_channel = channel.clone();
        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(move |_| Ok(Some(returned_channel.clone())));

        message_repository
            .expect_create()
            .times(1)
            .returning(|message| Ok(message));

        event_publisher
            .expect_publish_message_sent()
            .times(1)
            .returning(|_| Ok(()));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_client),
            Arc::new(event_publisher),
        );

        let empty_content = MessageContent::new("".to_string());
        assert!(
            empty_content.is_err(),
            "Empty content should fail validation"
        );

        let valid_content = MessageContent::new("Valid message".to_string()).unwrap();
        let result = service
            .send_message(channel_id, user_id, valid_content)
            .await;
        assert!(result.is_ok(), "Valid message should succeed");
    }

    #[tokio::test]
    async fn test_get_channel_messages() {
        let mut message_repository = MockTestMessageRepository::new();
        let channel_repository = MockTestChannelRepository::new();
        let user_client = MockTestUserService::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        let expected_messages = vec![
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 1".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 2".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 3".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 4".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 5".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
        ];

        let returned_messages = expected_messages.clone();
        message_repository
            .expect_find_by_channel()
            .withf(move |ch_id, limit, before| {
                *ch_id == channel_id && *limit == 10 && before.is_none()
            })
            .times(1)
            .returning(move |_, _, _| Ok(returned_messages.clone()));

        let event_publisher = MockTestEventPublisher::new();
        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_client),
            Arc::new(event_publisher),
        );

        // Get messages
        let result = service.get_channel_messages(channel_id, 10, None).await;
        assert!(result.is_ok());

        let messages = result.unwrap();
        assert_eq!(messages.len(), 5);
    }

    #[tokio::test]
    async fn test_get_channel_messages_with_limit() {
        let mut message_repository = MockTestMessageRepository::new();
        let channel_repository = MockTestChannelRepository::new();
        let user_client = MockTestUserService::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        let expected_messages = vec![
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 1".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 2".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Message 3".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
        ];

        let returned_messages = expected_messages.clone();
        message_repository
            .expect_find_by_channel()
            .withf(move |ch_id, limit, before| {
                *ch_id == channel_id && *limit == 3 && before.is_none()
            })
            .times(1)
            .returning(move |_, _, _| Ok(returned_messages.clone()));

        let event_publisher = MockTestEventPublisher::new();
        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_client),
            Arc::new(event_publisher),
        );

        // Get messages with limit
        let result = service.get_channel_messages(channel_id, 3, None).await;
        assert!(result.is_ok());

        let messages = result.unwrap();
        assert_eq!(messages.len(), 3);
    }

    #[tokio::test]
    async fn test_send_message_content_too_long() {
        let mut message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_client = MockTestUserService::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        let channel = Channel::Public(PublicChannel {
            id: channel_id,
            name: ChannelName::new("general".to_string()).unwrap(),
            description: None,
            created_by: user_id,
            created_at: Utc::now(),
        });

        let returned_channel = channel.clone();
        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(move |_| Ok(Some(returned_channel.clone())));

        message_repository
            .expect_create()
            .times(1)
            .returning(|message| Ok(message));

        // Expect event to be published for valid message
        event_publisher
            .expect_publish_message_sent()
            .times(1)
            .returning(|_| Ok(()));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_client),
            Arc::new(event_publisher),
        );

        // Test 1: Content that's too long should fail at newtype validation
        let long_content = "a".repeat(5000);
        let invalid_content = MessageContent::new(long_content);
        assert!(
            invalid_content.is_err(),
            "Content over 4000 chars should fail"
        );

        // Test 2: Content at max length should work
        let max_content = "a".repeat(4000);
        let valid_content = MessageContent::new(max_content).unwrap();
        let result = service
            .send_message(channel_id, user_id, valid_content)
            .await;
        assert!(result.is_ok(), "Content at max length should succeed");
    }
}
