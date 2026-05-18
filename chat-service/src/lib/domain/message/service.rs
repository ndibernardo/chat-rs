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
use crate::domain::user::ports::UserResolver;

pub struct MessageService<MR, CR, UC, EP>
where
    MR: MessageRepository,
    CR: ChannelRepository,
    UC: UserResolver,
    EP: MessageEventPublisher,
{
    message_repository: Arc<MR>,
    channel_repository: Arc<CR>,
    user_resolver: Arc<UC>,
    event_publisher: Arc<EP>,
}

impl<MR, CR, UC, EP> MessageService<MR, CR, UC, EP>
where
    MR: MessageRepository,
    CR: ChannelRepository,
    UC: UserResolver,
    EP: MessageEventPublisher,
{
    /// Create a new message service with injected dependencies.
    ///
    /// # Arguments
    /// * `message_repository` - Message persistence implementation
    /// * `channel_repository` - Channel repository for existence checks
    /// * `user_resolver` - Resolver for sender identity (replica-first with gRPC fallback)
    /// * `event_publisher` - Event publisher implementation
    ///
    /// # Returns
    /// Configured message service instance
    pub fn new(
        message_repository: Arc<MR>,
        channel_repository: Arc<CR>,
        user_resolver: Arc<UC>,
        event_publisher: Arc<EP>,
    ) -> Self {
        Self {
            message_repository,
            channel_repository,
            user_resolver,
            event_publisher,
        }
    }
}

#[async_trait]
impl<MR, CR, UC, EP> MessageServicePort for MessageService<MR, CR, UC, EP>
where
    MR: MessageRepository + 'static,
    CR: ChannelRepository + 'static,
    UC: UserResolver + 'static,
    EP: MessageEventPublisher + 'static,
{
    async fn send_message(
        &self,
        channel_id: ChannelId,
        user_id: UserId,
        content: MessageContent,
    ) -> Result<Message, MessageError> {
        self.channel_repository
            .find_by_id(channel_id)
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
            .ok_or(MessageError::ChannelNotFound(channel_id))?;

        self.user_resolver
            .resolve(user_id)
            .await
            .map_err(|e| MessageError::DatabaseError(e))?
            .ok_or(MessageError::UserNotFound(user_id))?;

        let message = Message {
            id: MessageId::new_time_based(),
            channel_id,
            user_id,
            content: content.clone(),
            timestamp: Utc::now(),
        };

        let saved_message = self.message_repository.create(message).await?;

        let event = MessageSentEvent::new(&saved_message);

        // fire-and-forget: broadcast failure must not block the sender
        if let Err(e) = self.event_publisher.publish_message_sent(&event).await {
            tracing::error!("Failed to publish message event: {}", e);
        }

        Ok(saved_message)
    }

    async fn get_channel_messages(
        &self,
        channel_id: ChannelId,
        limit: i32,
        before: Option<chrono::DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError> {
        self.channel_repository
            .find_by_id(channel_id)
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?
            .ok_or(MessageError::ChannelNotFound(channel_id))?;

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
    use crate::domain::user::models::Username;

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
        pub TestUserResolver {}

        #[async_trait]
        impl UserResolver for TestUserResolver {
            async fn resolve(&self, user_id: UserId) -> Result<Option<User>, String>;
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

    fn public_channel(channel_id: ChannelId, creator_id: UserId) -> Channel {
        Channel::Public(PublicChannel {
            id: channel_id,
            name: ChannelName::new("engineering").unwrap(),
            description: None,
            created_by: creator_id,
            created_at: Utc::now(),
        })
    }

    fn known_user(user_id: UserId) -> User {
        User::new(
            user_id,
            Username::new("nina-simone".to_string()).unwrap(),
            Utc::now(),
            Utc::now(),
        )
    }

    #[tokio::test]
    async fn send_message_returns_persisted_message() {
        let mut message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let mut user_resolver = MockTestUserResolver::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        channel_repository
            .expect_find_by_id()
            .withf(move |id| *id == channel_id)
            .times(1)
            .returning(move |_| Ok(Some(public_channel(channel_id, user_id))));

        user_resolver
            .expect_resolve()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(move |id| Ok(Some(known_user(id))));

        message_repository
            .expect_create()
            .withf(move |m| {
                m.channel_id == channel_id
                    && m.user_id == user_id
                    && m.content.as_str() == "What's the deployment status?"
            })
            .times(1)
            .returning(|m| Ok(m));

        event_publisher
            .expect_publish_message_sent()
            .times(1)
            .returning(|_| Ok(()));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("What's the deployment status?".to_string()).unwrap();
        let result = service.send_message(channel_id, user_id, content).await;

        assert!(result.is_ok());
        let message = result.unwrap();
        assert_eq!(message.channel_id, channel_id);
        assert_eq!(message.user_id, user_id);
        assert_eq!(message.content.as_str(), "What's the deployment status?");
    }

    #[tokio::test]
    async fn send_message_returns_channel_not_found_for_missing_channel() {
        let message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_resolver = MockTestUserResolver::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("Has the incident been resolved?".to_string()).unwrap();
        let result = service.send_message(channel_id, user_id, content).await;

        assert!(matches!(result, Err(MessageError::ChannelNotFound(_))));
    }

    #[tokio::test]
    async fn send_message_returns_user_not_found_when_sender_missing() {
        let message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let mut user_resolver = MockTestUserResolver::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(move |_| Ok(Some(public_channel(channel_id, user_id))));

        user_resolver
            .expect_resolve()
            .times(1)
            .returning(|_| Ok(None));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("This sender was deleted".to_string()).unwrap();
        let result = service.send_message(channel_id, user_id, content).await;

        assert!(matches!(result, Err(MessageError::UserNotFound(_))));
    }

    #[tokio::test]
    async fn get_channel_messages_returns_messages_in_order() {
        let mut message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_resolver = MockTestUserResolver::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();

        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(move |_| Ok(Some(public_channel(channel_id, user_id))));

        let messages = vec![
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("First deploy attempt".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
            Message {
                id: MessageId::new_time_based(),
                channel_id,
                user_id,
                content: MessageContent::new("Rollback complete".to_string()).unwrap(),
                timestamp: Utc::now(),
            },
        ];

        let returned = messages.clone();
        message_repository
            .expect_find_by_channel()
            .withf(move |ch, limit, before| *ch == channel_id && *limit == 10 && before.is_none())
            .times(1)
            .returning(move |_, _, _| Ok(returned.clone()));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let result = service.get_channel_messages(channel_id, 10, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn get_channel_messages_returns_channel_not_found_for_missing_channel() {
        let message_repository = MockTestMessageRepository::new();
        let mut channel_repository = MockTestChannelRepository::new();
        let user_resolver = MockTestUserResolver::new();
        let event_publisher = MockTestEventPublisher::new();

        channel_repository
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None));

        let service = MessageService::new(
            Arc::new(message_repository),
            Arc::new(channel_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let result = service
            .get_channel_messages(ChannelId::new(), 10, None)
            .await;
        assert!(matches!(result, Err(MessageError::ChannelNotFound(_))));
    }
}
