mod common;

use chat_service::domain::user::models::User;
use chat_service::domain::user::models::UserId;
use chat_service::domain::user::models::Username;
use chat_service::domain::user::ports::UserReplicaRepository;
use chat_service::outbound::repositories::user_replica::PostgresUserReplicaRepository;
use chrono::Utc;
use common::TestDb;
use uuid::Uuid;

#[tokio::test]
async fn test_upsert_new_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId(Uuid::new_v4());
    let user = User {
        id: user_id,
        username: Username::new("john_doe".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Insert new user
    let result = user_replica_repository.upsert(user.clone()).await;
    assert!(result.is_ok(), "Failed to upsert user: {:?}", result);

    // Verify user was inserted
    let retrieved_user = user_replica_repository
        .get(user_id)
        .await
        .expect("Failed to get user");

    assert!(retrieved_user.is_some());
    let retrieved_user = retrieved_user.unwrap();
    assert_eq!(retrieved_user.id, user_id);
    assert_eq!(retrieved_user.username.as_str(), "john_doe");
}

#[tokio::test]
async fn test_upsert_existing_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId(Uuid::new_v4());
    let created_at = Utc::now();

    // Insert initial user
    let user = User {
        id: user_id,
        username: Username::new("john_doe".to_string()).expect("Invalid username"),
        created_at,
        updated_at: created_at,
    };

    user_replica_repository
        .upsert(user.clone())
        .await
        .expect("Failed to insert user");

    // Update user with new data
    let updated_user = User {
        id: user_id,
        username: Username::new("john_updated".to_string()).expect("Invalid username"),
        created_at,
        updated_at: Utc::now(),
    };

    let result = user_replica_repository.upsert(updated_user.clone()).await;
    assert!(result.is_ok(), "Failed to update user: {:?}", result);

    // Verify user was updated
    let retrieved_user = user_replica_repository
        .get(user_id)
        .await
        .expect("Failed to get user")
        .expect("User not found");

    assert_eq!(retrieved_user.username.as_str(), "john_updated");
}

#[tokio::test]
async fn test_delete_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId(Uuid::new_v4());

    // Insert user
    let user = User {
        id: user_id,
        username: Username::new("john_doe".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    user_replica_repository
        .upsert(user.clone())
        .await
        .expect("Failed to insert user");

    // Delete user
    let result = user_replica_repository.delete(user_id).await;
    assert!(result.is_ok(), "Failed to delete user: {:?}", result);

    // Verify user was deleted
    let retrieved_user = user_replica_repository
        .get(user_id)
        .await
        .expect("Failed to query user");

    assert!(retrieved_user.is_none(), "User should have been deleted");
}

#[tokio::test]
async fn test_delete_nonexistent_user() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id = UserId(Uuid::new_v4());

    // Delete non-existent user (should not fail, just log)
    let result = user_replica_repository.delete(user_id).await;
    assert!(
        result.is_ok(),
        "Delete should succeed even if user doesn't exist"
    );
}

#[tokio::test]
async fn test_get_many_users() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    // Insert multiple users
    let user_id_1 = UserId(Uuid::new_v4());
    let user_id_2 = UserId(Uuid::new_v4());
    let user_id_3 = UserId(Uuid::new_v4());

    let user_1 = User {
        id: user_id_1,
        username: Username::new("user1".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let user_2 = User {
        id: user_id_2,
        username: Username::new("user2".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let user_3 = User {
        id: user_id_3,
        username: Username::new("user3".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert user1");
    user_replica_repository
        .upsert(user_2)
        .await
        .expect("Failed to insert user2");
    user_replica_repository
        .upsert(user_3)
        .await
        .expect("Failed to insert user3");

    // Get multiple users
    let user_ids = vec![user_id_1, user_id_2, user_id_3];
    let users = user_replica_repository
        .get_many(&user_ids)
        .await
        .expect("Failed to get users");

    assert_eq!(users.len(), 3);
    assert!(users.iter().any(|user| user.username.as_str() == "user1"));
    assert!(users.iter().any(|user| user.username.as_str() == "user2"));
    assert!(users.iter().any(|user| user.username.as_str() == "user3"));
}

#[tokio::test]
async fn test_get_many_partial_match() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    // Insert only one user
    let user_id_1 = UserId(Uuid::new_v4());
    let user_id_2 = UserId(Uuid::new_v4()); // This one won't be inserted

    let user_1 = User {
        id: user_id_1,
        username: Username::new("user1".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert user1");

    // Request both user IDs
    let user_ids = vec![user_id_1, user_id_2];
    let users = user_replica_repository
        .get_many(&user_ids)
        .await
        .expect("Failed to get users");

    // Should only return the one that exists
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username.as_str(), "user1");
}

#[tokio::test]
async fn test_upsert_preserves_unique_constraints() {
    let test_database = TestDb::new().await;
    let user_replica_repository = PostgresUserReplicaRepository::new(test_database.pg_pool.clone());

    // Insert first user
    let user_id_1 = UserId(Uuid::new_v4());
    let user_1 = User {
        id: user_id_1,
        username: Username::new("john_doe".to_string()).expect("Invalid username"),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert user1");

    // Try to insert second user with same username (different ID)
    let user_id_2 = UserId(Uuid::new_v4());
    let user_2 = User {
        id: user_id_2,
        username: Username::new("john_doe".to_string()).expect("Invalid username"), // Duplicate username
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let result = user_replica_repository.upsert(user_2).await;
    assert!(
        result.is_err(),
        "Should fail due to duplicate username constraint"
    );
}
