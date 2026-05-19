mod common;

use common::TestApp;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn create_user_returns_created_user() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["data"]["username"], "miles-davis");
    assert_eq!(body["data"]["email"], "miles.davis@example.com");
    assert!(body["data"]["id"].is_string());
    assert!(body["data"]["created_at"].is_string());
}

#[tokio::test]
async fn create_user_returns_conflict_for_duplicate_username() {
    let app = TestApp::spawn().await;

    app.post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "john.coltrane@example.com",
            "password": "G1ant-St3ps_1960!"
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
async fn create_user_returns_conflict_for_duplicate_email() {
    let app = TestApp::spawn().await;

    app.post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "john-coltrane",
            "email": "miles.davis@example.com",
            "password": "G1ant-St3ps_1960!"
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
async fn create_user_returns_unprocessable_for_short_username() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "mj",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
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
async fn create_user_returns_unprocessable_for_invalid_email() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "not-an-email",
            "password": "K1nd-0f-Blue_1959!"
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
async fn authenticate_returns_token_for_valid_credentials() {
    let app = TestApp::spawn().await;

    app.post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "miles-davis",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["token"].is_string());
    assert!(!body["data"]["token"].as_str().unwrap().is_empty());
    assert_eq!(body["data"]["user"]["username"], "miles-davis");
    assert_eq!(body["data"]["user"]["email"], "miles.davis@example.com");
}

#[tokio::test]
async fn authenticate_returns_unauthorized_for_wrong_password() {
    let app = TestApp::spawn().await;

    app.post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "miles-davis",
            "password": "wr0ng-p4ssw0rd!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"].is_string());
}

#[tokio::test]
async fn authenticate_returns_unauthorized_for_unknown_username() {
    let app = TestApp::spawn().await;

    let response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "chet-baker",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert!(body["data"]["message"].is_string());
}

#[tokio::test]
async fn get_user_returns_user_for_authenticated_request() {
    let app = TestApp::spawn().await;

    let create_response = app
        .post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let create_body: serde_json::Value = create_response
        .json()
        .await
        .expect("Failed to parse response");
    let user_id = create_body["data"]["id"].as_str().unwrap();

    let auth_response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "miles-davis",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let auth_body: serde_json::Value = auth_response
        .json()
        .await
        .expect("Failed to parse response");
    let token = auth_body["data"]["token"].as_str().unwrap();

    let response = app
        .get_authenticated(&format!("/api/users/{}", user_id), token)
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["data"]["id"], user_id);
    assert_eq!(body["data"]["username"], "miles-davis");
    assert_eq!(body["data"]["email"], "miles.davis@example.com");
}

#[tokio::test]
async fn get_user_returns_not_found_for_missing_id() {
    let app = TestApp::spawn().await;

    app.post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let auth_response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "miles-davis",
            "password": "K1nd-0f-Blue_1959!"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let auth_body: serde_json::Value = auth_response
        .json()
        .await
        .expect("Failed to parse response");
    let token = auth_body["data"]["token"].as_str().unwrap();

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
async fn full_user_workflow_creates_authenticates_updates_and_deletes() {
    let app = TestApp::spawn().await;

    let create_response = app
        .post("/api/users")
        .json(&json!({
            "username": "miles-davis",
            "email": "miles.davis@example.com",
            "password": "K1nd-0f-Blue_1959!"
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

    let login_response = app
        .post("/api/auth/login")
        .json(&json!({
            "username": "miles-davis",
            "password": "K1nd-0f-Blue_1959!"
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
    assert_eq!(user_body["data"]["username"], "miles-davis");

    let update_response = app
        .patch_authenticated(&format!("/api/users/{}", user_id), &token)
        .json(&json!({
            "email": "miles.dewey.davis@example.com"
        }))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(update_response.status(), StatusCode::OK);

    let update_body: serde_json::Value = update_response
        .json()
        .await
        .expect("Failed to parse response");
    assert_eq!(update_body["data"]["email"], "miles.dewey.davis@example.com");

    let invalid_response = app
        .get_authenticated(&format!("/api/users/{}", user_id), "invalid-token")
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
}
