mod common;

use common::TestApp;
use serde_json::json;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::StatusCode;

/// Creates a public channel via the REST API and returns its id.
async fn create_public_channel(app: &TestApp, token: &str, name: &str) -> String {
    let response = app
        .post_authenticated("/api/channels", token)
        .json(&json!({
            "channel_type": "public",
            "name": name,
        }))
        .send()
        .await
        .expect("Failed to execute request");

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn websocket_upgrade_succeeds_with_bearer_subprotocol() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();
    let channel_id = create_public_channel(&app, &token, "incident-response").await;

    let url = format!("ws://127.0.0.1:{}/ws/channels/{}", app.port, channel_id);
    let mut request = url.into_client_request().expect("Valid WS URL");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("bearer, {token}").parse().unwrap(),
    );

    let (_stream, response) = connect_async(request)
        .await
        .expect("WebSocket handshake should succeed with a valid bearer subprotocol");

    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(
        response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok()),
        Some("bearer")
    );
}

#[tokio::test]
async fn websocket_upgrade_rejects_token_passed_as_query_string() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();
    let channel_id = create_public_channel(&app, &token, "incident-response").await;

    // No Sec-WebSocket-Protocol header at all — the old query-string
    // `?token=` scheme must no longer authenticate the connection.
    let url = format!(
        "ws://127.0.0.1:{}/ws/channels/{}?token={}",
        app.port, channel_id, token
    );
    let request = url.into_client_request().expect("Valid WS URL");

    let result = connect_async(request).await;

    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        other => panic!("Expected handshake to be rejected with 401, got {other:?}"),
    }
}

#[tokio::test]
async fn websocket_upgrade_rejects_missing_sec_websocket_protocol_header() {
    let app = TestApp::spawn().await;
    let (token, _user_id) = app.create_test_token();
    let channel_id = create_public_channel(&app, &token, "incident-response").await;

    let url = format!("ws://127.0.0.1:{}/ws/channels/{}", app.port, channel_id);
    let request = url.into_client_request().expect("Valid WS URL");

    let result = connect_async(request).await;

    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        other => panic!("Expected handshake to be rejected with 401, got {other:?}"),
    }
}
