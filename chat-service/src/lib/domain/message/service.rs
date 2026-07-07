use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use super::events::MessageSentEvent;
use super::models::Limit;
use super::models::Message;
use super::models::MessageContent;
use super::ports::MessageEventPublisher;
use super::ports::MessageRepository;
use super::ports::MessageService;
use crate::domain::channel::models::Membership;
use crate::domain::message::errors::MessageError;
use crate::domain::user::ports::UserResolver;

pub struct Service<MR, UC, EP>
where
    MR: MessageRepository,
    UC: UserResolver,
    EP: MessageEventPublisher,
{
    message_repository: Arc<MR>,
    user_resolver: Arc<UC>,
    event_publisher: Arc<EP>,
}

impl<MR, UC, EP> Service<MR, UC, EP>
where
    MR: MessageRepository,
    UC: UserResolver,
    EP: MessageEventPublisher,
{
    /// Create a new message service with injected dependencies.
    ///
    /// # Arguments
    /// * `message_repository` - Message persistence implementation
    /// * `user_resolver` - Resolver for sender identity (replica-first with remote fallback)
    /// * `event_publisher` - Event publisher implementation
    ///
    /// # Returns
    /// Configured message service instance
    pub fn new(
        message_repository: Arc<MR>,
        user_resolver: Arc<UC>,
        event_publisher: Arc<EP>,
    ) -> Self {
        Self {
            message_repository,
            user_resolver,
            event_publisher,
        }
    }
}

#[async_trait]
impl<MR, UC, EP> MessageService for Service<MR, UC, EP>
where
    MR: MessageRepository + 'static,
    UC: UserResolver + 'static,
    EP: MessageEventPublisher + 'static,
{
    async fn send_message(
        &self,
        membership: Membership,
        content: MessageContent,
    ) -> Result<Message, MessageError> {
        // Channel existence is already proven by `membership`; only sender
        // resolution remains to check.
        let resolved_user = self
            .user_resolver
            .resolve(membership.user_id())
            .await
            .map_err(|e| MessageError::DatabaseError(e.to_string()))?;
        resolved_user.ok_or(MessageError::UserNotFound(membership.user_id()))?;

        let message = Message::new(membership.channel_id(), membership.user_id(), content);

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
        membership: Membership,
        limit: Limit,
        before: Option<chrono::DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError> {
        self.message_repository
            .find_by_channel(membership.channel_id(), limit, before)
            .await
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use mockall::mock;
    use mockall::predicate::*;

    use super::*;
    use crate::domain::channel::models::ChannelId;
    use crate::domain::message::events::MessageDeletedEvent;
    use crate::domain::user::errors::UserError;
    use crate::domain::user::models::User;
    use crate::domain::user::models::UserId;
    use crate::domain::user::models::Username;

    mock! {
        pub TestMessageRepository {}

        #[async_trait]
        impl MessageRepository for TestMessageRepository {
            async fn create(&self, message: Message) -> Result<Message, MessageError>;
            async fn find_by_channel(
                &self,
                channel_id: ChannelId,
                limit: Limit,
                before: Option<chrono::DateTime<Utc>>,
            ) -> Result<Vec<Message>, MessageError>;
            async fn find_by_user(
                &self,
                user_id: UserId,
                limit: Limit,
            ) -> Result<Vec<Message>, MessageError>;
        }
    }

    mock! {
        pub TestUserResolver {}

        #[async_trait]
        impl UserResolver for TestUserResolver {
            async fn resolve(&self, user_id: UserId) -> Result<Option<User>, UserError>;
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
        let mut user_resolver = MockTestUserResolver::new();
        let mut event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();
        let membership = Membership::test_new(user_id, channel_id);

        user_resolver
            .expect_resolve()
            .withf(move |id| *id == user_id)
            .times(1)
            .returning(move |id| Ok(Some(known_user(id))));

        message_repository
            .expect_create()
            .withf(move |m| {
                m.channel_id() == channel_id
                    && m.user_id() == user_id
                    && m.content().as_str() == "What's the deployment status?"
            })
            .times(1)
            .returning(|m| Ok(m));

        event_publisher
            .expect_publish_message_sent()
            .times(1)
            .returning(|_| Ok(()));

        let service = Service::new(
            Arc::new(message_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("What's the deployment status?".to_string()).unwrap();
        let result = service.send_message(membership, content).await;

        assert!(result.is_ok());
        let message = result.unwrap();
        assert_eq!(message.channel_id(), channel_id);
        assert_eq!(message.user_id(), user_id);
        assert_eq!(message.content().as_str(), "What's the deployment status?");
    }

    #[tokio::test]
    async fn send_message_returns_user_not_found_when_sender_missing() {
        let message_repository = MockTestMessageRepository::new();
        let mut user_resolver = MockTestUserResolver::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();
        let membership = Membership::test_new(user_id, channel_id);

        user_resolver
            .expect_resolve()
            .times(1)
            .returning(|_| Ok(None));

        let service = Service::new(
            Arc::new(message_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let content = MessageContent::new("This sender was deleted".to_string()).unwrap();
        let result = service.send_message(membership, content).await;

        assert!(matches!(result, Err(MessageError::UserNotFound(_))));
    }

    #[tokio::test]
    async fn get_channel_messages_returns_messages_in_order() {
        let mut message_repository = MockTestMessageRepository::new();
        let user_resolver = MockTestUserResolver::new();
        let event_publisher = MockTestEventPublisher::new();

        let user_id = UserId::new();
        let channel_id = ChannelId::new();
        let membership = Membership::test_new(user_id, channel_id);

        let messages = vec![
            Message::new(
                channel_id,
                user_id,
                MessageContent::new("First deploy attempt".to_string()).unwrap(),
            ),
            Message::new(
                channel_id,
                user_id,
                MessageContent::new("Rollback complete".to_string()).unwrap(),
            ),
        ];

        let returned = messages.clone();
        let limit = Limit::new(10).unwrap();
        message_repository
            .expect_find_by_channel()
            .withf(move |ch, l, before| *ch == channel_id && *l == limit && before.is_none())
            .times(1)
            .returning(move |_, _, _| Ok(returned.clone()));

        let service = Service::new(
            Arc::new(message_repository),
            Arc::new(user_resolver),
            Arc::new(event_publisher),
        );

        let result = service.get_channel_messages(membership, limit, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }
}
