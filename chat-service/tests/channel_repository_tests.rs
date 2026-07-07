mod common;

use chat_service::domain::channel::models::Channel;
use chat_service::domain::channel::ports::ChannelRepository as _;
use chat_service::domain::user::models::UserId;
use chat_service::outbound::postgres::ChannelRepository;
use common::TestDb;

/// Exercises direct-channel membership directly against Postgres:
/// participants live in `channel_members` (not a separate participants
/// table/columns), so `find_by_user` and `find_by_id` must round-trip them
/// through the same membership relation private channels use.
#[tokio::test]
async fn direct_channel_participants_round_trip_through_channel_members() {
    let test_database = TestDb::new().await;
    let repository = ChannelRepository::new(test_database.pg_pool.clone());

    let creator = UserId::new();
    let other = UserId::new();

    let created = repository
        .create(Channel::new_direct(creator, other).unwrap())
        .await
        .expect("Failed to create direct channel");

    // find_by_id round-trips both participants, regardless of who created it.
    let fetched = repository
        .find_by_id(created.id())
        .await
        .expect("query failed")
        .expect("channel not found");
    let participants = fetched.participants().expect("expected a direct channel");
    assert!(participants.contains(&creator));
    assert!(participants.contains(&other));

    // find_by_user (backed by a single channel_members join) finds the
    // channel for both the creator and the other participant.
    let creator_channels = repository
        .find_by_user(creator)
        .await
        .expect("query failed");
    assert!(creator_channels.iter().any(|c| c.id() == created.id()));

    let other_channels = repository
        .find_by_user(other)
        .await
        .expect("query failed");
    assert!(other_channels.iter().any(|c| c.id() == created.id()));
}

#[tokio::test]
async fn duplicate_direct_channel_pair_is_rejected_at_the_repository() {
    let test_database = TestDb::new().await;
    let repository = ChannelRepository::new(test_database.pg_pool.clone());

    let creator = UserId::new();
    let other = UserId::new();

    repository
        .create(Channel::new_direct(creator, other).unwrap())
        .await
        .expect("first direct channel should be created");

    let result = repository
        .create(Channel::new_direct(other, creator).unwrap())
        .await;

    assert!(
        matches!(
            result,
            Err(chat_service::domain::channel::errors::ChannelError::DirectChannelAlreadyExists)
        ),
        "expected DirectChannelAlreadyExists, got {:?}",
        result
    );
}
