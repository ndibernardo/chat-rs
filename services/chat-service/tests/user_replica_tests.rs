mod common;

use chat_service::domain::user::models::User;
use chat_service::domain::user::models::UserId;
use chat_service::domain::user::models::Username;
use chat_service::domain::user::ports::UserReplicaRepository as _;
use chat_service::outbound::postgres::user_replica::UserReplicaRepository;
use chrono::Utc;
use common::TestDb;

#[tokio::test]
async fn test_upsert_new_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId::new();
    let user = User::new(
        user_id,
        Username::new("susan_clark".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    let result = user_replica_repository.upsert(user.clone()).await;
    assert!(result.is_ok(), "Failed to upsert user: {:?}", result);

    let retrieved_user = user_replica_repository
        .get(user_id)
        .await
        .expect("Failed to get user");

    assert!(retrieved_user.is_some());
    let retrieved_user = retrieved_user.unwrap();
    assert_eq!(retrieved_user.id(), user_id);
    assert_eq!(retrieved_user.username().as_str(), "susan_clark");
}

#[tokio::test]
async fn test_upsert_existing_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId::new();
    let created_at = Utc::now();

    let user = User::new(
        user_id,
        Username::new("patti_smith".to_string()).expect("Invalid username"),
        created_at,
        created_at,
    );

    user_replica_repository
        .upsert(user.clone())
        .await
        .expect("Failed to insert user");

    let updated_user = User::new(
        user_id,
        Username::new("patti_smith_group".to_string()).expect("Invalid username"),
        created_at,
        Utc::now(),
    );

    let result = user_replica_repository.upsert(updated_user.clone()).await;
    assert!(result.is_ok(), "Failed to update user: {:?}", result);

    let retrieved_user = user_replica_repository
        .get(user_id)
        .await
        .expect("Failed to get user")
        .expect("User not found");

    assert_eq!(retrieved_user.username().as_str(), "patti_smith_group");
}

#[tokio::test]
async fn test_delete_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId::new();

    let user = User::new(
        user_id,
        Username::new("james_walker".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    user_replica_repository
        .upsert(user.clone())
        .await
        .expect("Failed to insert user");

    let result = user_replica_repository.delete(user_id).await;
    assert!(result.is_ok(), "Failed to delete user: {:?}", result);

    let retrieved_user = user_replica_repository
        .get(user_id)
        .await
        .expect("Failed to query user");

    assert!(retrieved_user.is_none(), "User should have been deleted");
}

#[tokio::test]
async fn test_delete_nonexistent_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId::new();

    let result = user_replica_repository.delete(user_id).await;
    assert!(
        result.is_ok(),
        "Delete should succeed even if user doesn't exist"
    );
}

#[tokio::test]
async fn test_get_many_users() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id_1 = UserId::new();
    let user_id_2 = UserId::new();
    let user_id_3 = UserId::new();

    let user_1 = User::new(
        user_id_1,
        Username::new("jane_doe".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );
    let user_2 = User::new(
        user_id_2,
        Username::new("alice_turner".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );
    let user_3 = User::new(
        user_id_3,
        Username::new("daniel_scott".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert jane_doe");
    user_replica_repository
        .upsert(user_2)
        .await
        .expect("Failed to insert alice_turner");
    user_replica_repository
        .upsert(user_3)
        .await
        .expect("Failed to insert daniel_scott");

    let user_ids = vec![user_id_1, user_id_2, user_id_3];
    let users = user_replica_repository
        .get_many(&user_ids)
        .await
        .expect("Failed to get users");

    assert_eq!(users.len(), 3);
    assert!(
        users
            .iter()
            .any(|user| user.username().as_str() == "jane_doe")
    );
    assert!(
        users
            .iter()
            .any(|user| user.username().as_str() == "alice_turner")
    );
    assert!(
        users
            .iter()
            .any(|user| user.username().as_str() == "daniel_scott")
    );
}

#[tokio::test]
async fn test_get_many_partial_match() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id_1 = UserId::new();
    let user_id_2 = UserId::new(); // This one won't be inserted

    let user_1 = User::new(
        user_id_1,
        Username::new("laura_adams".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert laura_adams");

    let user_ids = vec![user_id_1, user_id_2];
    let users = user_replica_repository
        .get_many(&user_ids)
        .await
        .expect("Failed to get users");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username().as_str(), "laura_adams");
}

#[tokio::test]
async fn test_upsert_preserves_unique_constraints() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id_1 = UserId::new();
    let user_1 = User::new(
        user_id_1,
        Username::new("billie_holiday".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert billie_holiday");

    let user_id_2 = UserId::new();
    let user_2 = User::new(
        user_id_2,
        Username::new("billie_holiday".to_string()).expect("Invalid username"), // Duplicate username
        Utc::now(),
        Utc::now(),
    );

    let result = user_replica_repository.upsert(user_2).await;
    assert!(
        result.is_err(),
        "Should fail due to duplicate username constraint"
    );
}
