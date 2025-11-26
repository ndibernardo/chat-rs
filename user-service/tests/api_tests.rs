mod common;

use common::TestApp;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn test_create_user_success() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["data"]["username"], "nicola");
    assert_eq!(body["data"]["email"], "nicola@example.com");
    assert!(body["data"]["id"].is_string());
    assert!(body["data"]["created_at"].is_string());
}

#[tokio::test]
async fn test_create_user_duplicate_username() {
    let app = TestApp::spawn().await;

    // Create first user
    app.post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // Try to create user with same username but different email
    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .contains("already exists"));
}

#[tokio::test]
async fn test_create_user_duplicate_email() {
    let app = TestApp::spawn().await;

    // Create first user
    app.post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // Try to create user with different username but same email
    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "nicola2",
            "email_address": "nicola@example.com",
            "password": "pass_word!2"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .contains("already exists"));
}

#[tokio::test]
async fn test_create_user_invalid_username() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "n",
            "email_address": "nicola@example.com",
            "password": "pass_word"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .contains("minimum 3 characters"));
}

#[tokio::test]
async fn test_create_user_invalid_email() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "not-an-email",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("email"));
}

#[tokio::test]
async fn test_authenticate_success() {
    let app = TestApp::spawn().await;

    // Create user
    app.post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // Authenticate
    let response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "nicola",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["token"].is_string());
    assert!(!body["data"]["token"].as_str().unwrap().is_empty());
    assert_eq!(body["data"]["user"]["username"], "nicola");
    assert_eq!(body["data"]["user"]["email"], "nicola@example.com");
}

#[tokio::test]
async fn test_authenticate_wrong_password() {
    let app = TestApp::spawn().await;

    // Create user
    app.post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "Correct_Password!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    // Try to authenticate with wrong password
    let response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "nicola",
            "password": "Wrong_Password!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"].is_string());
}

#[tokio::test]
async fn test_authenticate_nonexistent_user() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "nonexistent",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"].is_string());
}

#[tokio::test]
async fn test_get_user_by_id() {
    let app = TestApp::spawn().await;

    // Create a user
    let create_response = app
        .post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let user_id = create_body["data"]["id"].as_str().unwrap();

    // Authenticate to get token
    let auth_response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "nicola",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let auth_body: serde_json::Value = auth_response
        .json()
        .await
        .expect("Failed to parse response");
    let token = auth_body["data"]["token"].as_str().unwrap();

    // Get user by ID
    let response = app
        .get_authenticated(&format!("/api/users/{}", user_id), token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["data"]["id"], user_id);
    assert_eq!(body["data"]["username"], "nicola");
    assert_eq!(body["data"]["email"], "nicola@example.com");
}

#[tokio::test]
async fn test_get_user_not_found() {
    let app = TestApp::spawn().await;

    // Create a user and get token
    app.post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let auth_response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "nicola",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let auth_body: serde_json::Value = auth_response
        .json()
        .await
        .expect("Failed to parse response");
    let token = auth_body["data"]["token"].as_str().unwrap();

    // Try to get non-existent user
    let fake_uuid = uuid::Uuid::new_v4().to_string();
    let response = app
        .get_authenticated(&format!("/api/users/{}", fake_uuid), token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"].is_string());
}

#[tokio::test]
async fn test_full_user_workflow() {
    let app = TestApp::spawn().await;

    // 1. Create user
    let create_response = app
        .post("/api/users")
        .json(&json!({
            "username": "nicola",
            "email_address": "nicola@example.com",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let user_id = create_body["data"]["id"].as_str().unwrap().to_string();

    // 2. Login
    let login_response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "nicola",
            "password": "pass_word!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(login_response.status(), StatusCode::OK);

    let login_body: serde_json::Value = login_response
        .json()
        .await
        .expect("Failed to parse response");
    let token = login_body["data"]["token"].as_str().unwrap().to_string();

    // 3. Access protected endpoint - get user by ID
    let user_response = app
        .get_authenticated(&format!("/api/users/{}", user_id), &token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(user_response.status(), StatusCode::OK);

    let user_body: serde_json::Value = user_response
        .json()
        .await
        .expect("Failed to parse response");
    assert_eq!(user_body["data"]["username"], "nicola");

    // 4. Update user
    let update_response = app
        .patch_authenticated(&format!("/api/users/{}", user_id), &token)
        .json(&json!({
            "email_address": "updated@example.com"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(update_response.status(), StatusCode::OK);

    // 5. Try to access with invalid token - should fail
    let invalid_response = app
        .get_authenticated(&format!("/api/users/{}", user_id), "invalid")
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
}
