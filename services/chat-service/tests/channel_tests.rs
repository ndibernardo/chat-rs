mod common;

use common::TestApp;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn test_create_public_channel_success() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "general",
            "description": "General discussion channel"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["channel_type"], "public");
    assert_eq!(body["name"], "general");
    assert_eq!(body["description"], "General discussion channel");
    assert!(body["id"].is_string());
    assert!(body["created_by"].is_string());
    assert!(body["created_at"].is_string());
}

#[tokio::test]
async fn test_create_public_channel_without_description() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "random"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["channel_type"], "public");
    assert_eq!(body["name"], "random");
    assert!(body["description"].is_null());
}

#[tokio::test]
async fn test_create_private_channel_success() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "private",
            "name": "team-internal",
            "description": "Private team channel",
            "members": [
                uuid::Uuid::new_v4().to_string(),
                uuid::Uuid::new_v4().to_string()
            ]
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["channel_type"], "private");
    assert_eq!(body["name"], "team-internal");
    assert_eq!(body["description"], "Private team channel");
}

#[tokio::test]
async fn test_create_direct_channel_success() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "direct",
            "participant_id": uuid::Uuid::new_v4().to_string()
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["channel_type"], "direct");
    assert!(body["name"].is_null());
    assert!(body["description"].is_null());
}

#[tokio::test]
async fn test_create_direct_channel_rejects_self_dm() {
    let app = TestApp::spawn().await;
    let (token, user_id) = app.create_test_token();

    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "direct",
            "participant_id": user_id.to_string()
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_create_direct_channel_rejects_duplicate_pair_in_either_order() {
    let app = TestApp::spawn().await;
    let (token_a, user_a) = app.create_test_token();
    let (token_b, user_b) = app.create_test_token();

    let first = app
        .post_authenticated("/api/channels", &token_a)
        .json(&json!({
            "channel_type": "direct",
            "participant_id": user_b.to_string()
        }))
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(first.status(), StatusCode::CREATED);

    // Same pair, same direction: already exists.
    let second = app
        .post_authenticated("/api/channels", &token_a)
        .json(&json!({
            "channel_type": "direct",
            "participant_id": user_b.to_string()
        }))
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(second.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Same pair, reversed direction: still already exists.
    let third = app
        .post_authenticated("/api/channels", &token_b)
        .json(&json!({
            "channel_type": "direct",
            "participant_id": user_a.to_string()
        }))
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(third.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_create_channel_with_empty_name() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": ""
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["error"].as_str().unwrap().contains("Channel name"));
}

#[tokio::test]
async fn test_create_channel_with_too_long_name() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let long_name = "a".repeat(101);
    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": long_name
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["error"].as_str().unwrap().contains("100 characters"));
}

#[tokio::test]
async fn test_create_channel_with_duplicate_name() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create first channel
    app.post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "duplicate"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // Try to create channel with same name
    let response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "duplicate"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["error"].as_str().unwrap().contains("already exists"));
}

#[tokio::test]
async fn test_get_channel_by_id() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create a channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "platform-releases",
            "description": "Platform release announcements"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // Get channel by ID
    let response = app
        .get_authenticated(&format!("/api/channels/{}", channel_id), &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["id"], channel_id);
    assert_eq!(body["name"], "platform-releases");
    assert_eq!(body["description"], "Platform release announcements");
}

#[tokio::test]
async fn test_get_channel_not_found() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let fake_uuid = uuid::Uuid::new_v4().to_string();
    let response = app
        .get_authenticated(&format!("/api/channels/{}", fake_uuid), &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_get_channel_returns_forbidden_for_non_member_of_private_channel() {
    let app = TestApp::spawn().await;
    let (creator_token, creator_id) = app.create_test_token();
    let (outsider_token, _outsider_id) = app.create_test_token();

    let create_response = app
        .post_authenticated("/api/channels", &creator_token)
        .json(&json!({
            "channel_type": "private",
            "name": "leadership-only",
            "description": "Restricted channel",
            "members": [creator_id.to_string()]
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap();

    // The creator (a member) can read the channel.
    let member_response = app
        .get_authenticated(&format!("/api/channels/{}", channel_id), &creator_token)
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(member_response.status(), StatusCode::OK);

    // An unrelated authenticated user cannot.
    let outsider_response = app
        .get_authenticated(&format!("/api/channels/{}", channel_id), &outsider_token)
        .send()
        .await
        .expect("Failed to execute request");
    assert_eq!(outsider_response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_get_channel_with_invalid_uuid() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .get_authenticated("/api/channels/invalid-uuid", &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_list_public_channels() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // Create multiple public channels
    app.post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "platform-infra"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    app.post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "data-science"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // Create a private channel (should not appear in public list)
    app.post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "private",
            "name": "infra-alerts",
            "members": [uuid::Uuid::new_v4().to_string()]
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // List public channels with a different user
    let (list_token, _list_user_id) = app.create_test_token();
    let response = app
        .get_authenticated("/api/channels/public", &list_token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body.is_array());

    let channels = body.as_array().unwrap();
    assert!(channels.len() >= 2);

    // Verify all returned channels are public
    for channel in channels {
        assert_eq!(channel["channel_type"], "public");
    }

    // Verify our created channels are in the list
    let names: Vec<&str> = channels
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"platform-infra"));
    assert!(names.contains(&"data-science"));
    assert!(!names.contains(&"infra-alerts"));
}

#[tokio::test]
async fn test_list_public_channels_empty() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    let response = app
        .get_authenticated("/api/channels/public", &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_full_channel_workflow() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();

    // 1. Create a public channel
    let create_response = app
        .post_authenticated("/api/channels", &token)
        .json(&json!({
            "channel_type": "public",
            "name": "product-updates",
            "description": "Product roadmap and release updates"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let channel_id = create_body["id"].as_str().unwrap().to_string();

    // 2. Get the channel by ID
    let get_response = app
        .get_authenticated(&format!("/api/channels/{}", channel_id), &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(get_response.status(), StatusCode::OK);

    let get_body: serde_json::Value = get_response.json().await.expect("Failed to parse response");
    assert_eq!(get_body["id"], channel_id);
    assert_eq!(get_body["name"], "product-updates");

    // 3. List public channels and verify it's there
    let (list_token, _list_user_id) = app.create_test_token();
    let list_response = app
        .get_authenticated("/api/channels/public", &list_token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(list_response.status(), StatusCode::OK);

    let list_body: serde_json::Value = list_response
        .json()
        .await
        .expect("Failed to parse response");
    let channels = list_body.as_array().unwrap();
    let found = channels.iter().any(|c| c["id"] == channel_id);
    assert!(found);
}
