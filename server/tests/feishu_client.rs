use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tvdisplay::feishu::client::{FeishuApi, FeishuError};

#[derive(Clone, Default)]
struct Mock {
    token_calls: Arc<Mutex<u32>>,
    replies: Arc<Mutex<Vec<Value>>>,
}

#[derive(Deserialize)]
struct ResourceQuery { r#type: String }

async fn token(State(mock): State<Mock>, body: String) -> Json<Value> {
    *mock.token_calls.lock().unwrap() += 1;
    assert!(body.contains("grant_type=client_credentials"));
    assert!(body.contains("client_id=cli_test"));
    assert!(body.contains("client_secret=app-secret"));
    Json(json!({
        "code": 0,
        "access_token": "t-test-token",
        "expires_in": 7200,
        "token_type": "Bearer"
    }))
}

async fn resource(
    State(_mock): State<Mock>,
    Path((message_id, file_key)): Path<(String, String)>,
    Query(query): Query<ResourceQuery>,
    headers: HeaderMap,
) -> Result<Bytes, StatusCode> {
    assert_eq!(query.r#type, "image");
    assert_eq!(headers.get("authorization").unwrap(), "Bearer t-test-token");
    if message_id == "om_ok" && file_key == "img_ok" {
        return Ok(Bytes::from_static(b"\xff\xd8\xff\xd9"));
    }
    Err(StatusCode::BAD_REQUEST)
}

async fn reply(State(mock): State<Mock>, Path(message_id): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(message_id, "om_ok");
    mock.replies.lock().unwrap().push(body);
    Json(json!({"code": 0, "msg": "ok", "data": {"message_id": "om_reply"}}))
}

async fn spawn_mock() -> (String, Mock) {
    let mock = Mock::default();
    let app = Router::new()
        .route("/oauth/v3/token", post(token))
        .route("/open-apis/im/v1/messages/{message_id}/resources/{file_key}", get(resource))
        .route("/open-apis/im/v1/messages/{message_id}/reply", post(reply))
        .with_state(mock.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    (format!("http://127.0.0.1:{}", addr.port()), mock)
}

fn api(base: &str) -> FeishuApi {
    FeishuApi::new(
        "cli_test".into(),
        "app-secret".into(),
        base.to_string(),
        base.to_string(),
    )
}

#[tokio::test]
async fn token_is_cached_and_used_for_download_and_reply() {
    let (base, mock) = spawn_mock().await;
    let api = api(&base);
    let image = api.download_message_image("om_ok", "img_ok", 1024).await.unwrap();
    assert_eq!(image, b"\xff\xd8\xff\xd9");
    api.reply_text("om_ok", "请发送图片或文字").await.unwrap();
    api.reply_card("om_ok", &json!({"schema":"2.0"})).await.unwrap();
    assert_eq!(*mock.token_calls.lock().unwrap(), 1);
    let replies = mock.replies.lock().unwrap().clone();
    assert_eq!(replies[0]["msg_type"], "text");
    let content: Value = serde_json::from_str(replies[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["text"], "请发送图片或文字");
    assert_eq!(replies[1]["msg_type"], "interactive");
}

#[tokio::test]
async fn download_failure_maps_to_download_error() {
    let (base, _) = spawn_mock().await;
    let api = api(&base);
    let err = api.download_message_image("om_missing", "img_x", 1024).await.unwrap_err();
    assert!(matches!(err, FeishuError::Download));
    assert!(!format!("{err}").contains("app-secret"));
}

#[tokio::test]
async fn oversize_download_is_image_too_large() {
    let (base, _) = spawn_mock().await;
    let api = api(&base);
    let err = api.download_message_image("om_ok", "img_ok", 1).await.unwrap_err();
    assert!(matches!(err, FeishuError::ImageTooLarge));
}
