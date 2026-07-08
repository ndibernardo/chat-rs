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
        Username::new("nina_simone".to_string()).expect("Invalid username"),
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
    assert_eq!(retrieved_user.username().as_str(), "nina_simone");
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
        Username::new("charlie_parker".to_string()).expect("Invalid username"),
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
        Username::new("john_coltrane".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );
    let user_2 = User::new(
        user_id_2,
        Username::new("kim_gordon".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );
    let user_3 = User::new(
        user_id_3,
        Username::new("thelonious_monk".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert john_coltrane");
    user_replica_repository
        .upsert(user_2)
        .await
        .expect("Failed to insert kim_gordon");
    user_replica_repository
        .upsert(user_3)
        .await
        .expect("Failed to insert thelonious_monk");

    let user_ids = vec![user_id_1, user_id_2, user_id_3];
    let users = user_replica_repository
        .get_many(&user_ids)
        .await
        .expect("Failed to get users");

    assert_eq!(users.len(), 3);
    assert!(users.iter().any(|user| user.username().as_str() == "john_coltrane"));
    assert!(users.iter().any(|user| user.username().as_str() == "kim_gordon"));
    assert!(users.iter().any(|user| user.username().as_str() == "thelonious_monk"));
}

#[tokio::test]
async fn test_get_many_partial_match() {
    let test_database = TestDb::new().await;
    let user_replica_repository = UserReplicaRepository::new(test_database.pg_pool.clone());

    let user_id_1 = UserId::new();
    let user_id_2 = UserId::new(); // This one won't be inserted

    let user_1 = User::new(
        user_id_1,
        Username::new("ella_fitzgerald".to_string()).expect("Invalid username"),
        Utc::now(),
        Utc::now(),
    );

    user_replica_repository
        .upsert(user_1)
        .await
        .expect("Failed to insert ella_fitzgerald");

    let user_ids = vec![user_id_1, user_id_2];
    let users = user_replica_repository
        .get_many(&user_ids)
        .await
        .expect("Failed to get users");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username().as_str(), "ella_fitzgerald");
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
