//! Exercises chat-service's `RemoteUserLookup` gRPC client directly against a
//! real user-service instance, verifying that NotFound and success are
//! distinguished at the gRPC status level rather than conflated into a
//! generic error.

use chat_service::domain::user::ports::RemoteUserLookup;
use chat_service::outbound::grpc::UserServiceClient;

fn user_service_grpc_url() -> String {
    std::env::var("USER_SERVICE_GRPC_URL").unwrap_or_else(|_| "http://localhost:50052".to_string())
}

fn user_service_http_url() -> String {
    std::env::var("USER_SERVICE_HTTP_URL").unwrap_or_else(|_| "http://localhost:3001".to_string())
}

#[tokio::test]
async fn get_user_returns_none_for_nonexistent_user() {
    let client = UserServiceClient::new(&user_service_grpc_url())
        .await
        .expect("Failed to connect to user-service gRPC");

    let random_id = chat_service::domain::user::models::UserId::new();
    let result = client.get_user(random_id).await;

    assert!(result.is_ok(), "expected Ok(None), got {:?}", result);
    assert!(
        result.unwrap().is_none(),
        "expected None for a user that was never created"
    );
}

#[tokio::test]
async fn get_user_returns_resolved_user_for_existing_user() {
    let http_client = reqwest::Client::new();
    let unique = uuid::Uuid::new_v4().to_string().replace('-', "");
    let create_response = http_client
        .post(format!("{}/api/users", user_service_http_url()))
        .json(&serde_json::json!({
            "username": format!("grpctest{}", &unique[..12]),
            "email": format!("grpctest-{}@example.com", unique),
            "password": "Sup3r-S3cret-Pass!"
        }))
        .send()
        .await
        .expect("Failed to create test user via HTTP");

    assert_eq!(create_response.status(), reqwest::StatusCode::CREATED);
    let created: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let user_id_str = created["data"]["id"]
        .as_str()
        .expect("Missing data.id in response")
        .to_string();

    let grpc_client = UserServiceClient::new(&user_service_grpc_url())
        .await
        .expect("Failed to connect to user-service gRPC");

    let user_id = chat_service::domain::user::models::UserId::from_string(&user_id_str)
        .expect("Failed to parse user id");
    let result = grpc_client.get_user(user_id).await;

    assert!(result.is_ok(), "expected Ok(Some(..)), got {:?}", result);
    let resolved = result
        .unwrap()
        .expect("expected Some(ResolvedUser) for an existing user");
    assert_eq!(resolved.id(), user_id);
}
