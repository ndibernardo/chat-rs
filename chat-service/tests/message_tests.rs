mod common;

use common::TestApp;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn test_get_channel_messages_empty() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create a channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "test-channel"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // Get messages (should be empty)
    let response = app
        .get_authenticated(&format!("/api/channels/{}/messages", channel_id), &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_messages_from_nonexistent_channel() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let fake_uuid = uuid::Uuid::new_v4().to_string();
    let response = app
        .get_authenticated(&format!("/api/channels/{}/messages", fake_uuid), &token)
        .send()
        .await
        .expect("Failed to execute request");

    // The service may return OK with empty array or NOT_FOUND
    // depending on implementation - both are acceptable
    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "Expected OK or NOT_FOUND, got {:?}",
        status
    );

    if status == StatusCode::OK {
        let body: serde_json::Value = response.json().await.expect("Failed to parse response");
        assert!(body.is_array());
    }
}

#[tokio::test]
async fn test_get_messages_with_invalid_channel_id() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .get_authenticated("/api/channels/invalid-uuid/messages", &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_get_messages_with_limit_parameter() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create a channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "test-limit"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // Get messages with limit parameter
    let response = app
        .get_authenticated(
            &format!("/api/channels/{}/messages?limit=10", channel_id),
            &token,
        )
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body.is_array());
}

#[tokio::test]
async fn test_get_messages_with_before_parameter() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create a channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "test-before"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // Get messages with before parameter
    let before_time = chrono::Utc::now().to_rfc3339();
    let response = app
        .get_authenticated(
            &format!(
                "/api/channels/{}/messages?before={}",
                channel_id, before_time
            ),
            &token,
        )
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body.is_array());
}

#[tokio::test]
async fn test_get_messages_with_limit_and_before() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create a channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "test-pagination"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // Get messages with both limit and before parameters
    let before_time = chrono::Utc::now().to_rfc3339();
    let response = app
        .get_authenticated(
            &format!(
                "/api/channels/{}/messages?limit=20&before={}",
                channel_id, before_time
            ),
            &token,
        )
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body.is_array());
}

// Note: Since messages are sent via WebSocket, we can't easily test message creation
// via HTTP endpoints. For comprehensive message testing, see websocket_tests.rs
// or test the message service directly through the repository layer.

#[tokio::test]
async fn test_message_retrieval_workflow() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // 1. Create a channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "message-test"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(create_response.status(), StatusCode::OK);

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // 2. Get messages (should be empty initially)
    let list_response = app
        .get_authenticated(&format!("/api/channels/{}/messages", channel_id), &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(list_response.status(), StatusCode::OK);

    let list_body: serde_json::Value = list_response
        .json()
        .await
        .expect("Failed to parse response");
    assert!(list_body.is_array());
    assert_eq!(list_body.as_array().unwrap().len(), 0);

    // 3. Try different pagination options
    let limit_response = app
        .get_authenticated(
            &format!("/api/channels/{}/messages?limit=5", channel_id),
            &token,
        )
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(limit_response.status(), StatusCode::OK);

    let before = chrono::Utc::now().to_rfc3339();
    let before_response = app
        .get_authenticated(
            &format!("/api/channels/{}/messages?before={}", channel_id, before),
            &token,
        )
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(before_response.status(), StatusCode::OK);
}
