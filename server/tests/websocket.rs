use base64::Engine;
mod common;
use common::{spawn_app, JPEG, PASSWORD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::{protocol::CloseFrame, Message};

fn ws_url(http_base: &str) -> String {
    format!("{}/ws/tv", http_base.replacen("http://", "ws://", 1))
}

async fn connect(url: &str) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (stream, _) = tokio_tungstenite::connect_async(url).await.expect("ws connect");
    stream
}

async fn auth_and_recv(
    stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    password: &str,
) -> Value {
    stream
        .send(Message::Text(json!({"type":"auth","password": password}).to_string().into()))
        .await
        .unwrap();
    let msg = stream.next().await.unwrap().unwrap();
    serde_json::from_str(msg.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn bad_password_is_rejected() {
    let app = spawn_app().await;
    let mut stream = connect(&ws_url(&app.base)).await;
    stream
        .send(Message::Text(json!({"type":"auth","password":"wrong-password"}).to_string().into()))
        .await
        .unwrap();
    let close = wait_close(&mut stream).await;
    assert_eq!(close.code, 1008.into());
}

#[tokio::test]
async fn query_string_password_does_not_authenticate() {
    let app = spawn_app().await;
    let url = format!("{}?password={PASSWORD}", ws_url(&app.base));
    let mut stream = connect(&url).await;
    let close = timeout(Duration::from_secs(2), wait_close(&mut stream)).await;
    assert!(close.is_ok(), "connection without an auth frame must be closed");
    assert_eq!(close.unwrap().code, 1008.into());
}

#[tokio::test]
async fn connect_receives_current_content_after_offline() {
    let app = spawn_app().await;
    app.login().await;
    app.client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("text", "补上的内容")
                .text("force_popup", "on")
                .part(
                    "image",
                    reqwest::multipart::Part::bytes(JPEG.to_vec())
                        .file_name("a.jpg")
                        .mime_str("image/jpeg")
                        .unwrap(),
                ),
        )
        .send()
        .await
        .unwrap();
    let mut stream = connect(&ws_url(&app.base)).await;
    let message = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(message["type"], "content");
    assert_eq!(message["text"], "补上的内容");
    assert_eq!(message["force_popup"], true);
    assert_eq!(message["revision"], 1);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(message["image_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(decoded, JPEG);
}

#[tokio::test]
async fn empty_catalog_is_sent_when_nothing_published() {
    let app = spawn_app().await;
    let mut stream = connect(&ws_url(&app.base)).await;
    let message = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(message["revision"], 0);
    assert!(message["image_base64"].is_null());
    assert!(message["image_mime"].is_null());
    assert_eq!(message["text"], "");
}

#[tokio::test]
async fn text_only_content_has_null_image_fields() {
    let app = spawn_app().await;
    app.login().await;
    app.client
        .post(app.url("/content"))
        .multipart(reqwest::multipart::Form::new().text("text", "纯文字"))
        .send()
        .await
        .unwrap();
    let mut stream = connect(&ws_url(&app.base)).await;
    let message = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(message["revision"], 1);
    assert_eq!(message["text"], "纯文字");
    assert!(message["image_base64"].is_null());
    assert!(message["image_mime"].is_null());
}

#[tokio::test]
async fn publish_pushes_to_the_connected_tv() {
    let app = spawn_app().await;
    app.login().await;
    let mut stream = connect(&ws_url(&app.base)).await;
    let first = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(first["revision"], 0);
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["tv_online"], true);
    app.client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("text", "新海报")
                .part(
                    "image",
                    reqwest::multipart::Part::bytes(JPEG.to_vec())
                        .file_name("a.jpg")
                        .mime_str("image/jpeg")
                        .unwrap(),
                ),
        )
        .send()
        .await
        .unwrap();
    let pushed = next_json(&mut stream).await;
    assert_eq!(pushed["text"], "新海报");
}

#[tokio::test]
async fn latest_connection_replaces_the_previous_one() {
    let app = spawn_app().await;
    let mut first = connect(&ws_url(&app.base)).await;
    let _ = auth_and_recv(&mut first, PASSWORD).await;
    let mut second = connect(&ws_url(&app.base)).await;
    let _ = auth_and_recv(&mut second, PASSWORD).await;
    let close = wait_close(&mut first).await;
    assert_eq!(close.code, 4000.into());
    app.login().await;
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["tv_online"], true);
}

#[tokio::test]
async fn toggle_push_keeps_revision() {
    let app = spawn_app().await;
    app.login().await;
    app.client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new().part(
                "image",
                reqwest::multipart::Part::bytes(JPEG.to_vec())
                    .file_name("a.jpg")
                    .mime_str("image/jpeg")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    let mut stream = connect(&ws_url(&app.base)).await;
    let first = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(first["revision"], 1);
    app.client
        .post(app.url("/force-popup"))
        .form(&[("force_popup", "on")])
        .send()
        .await
        .unwrap();
    let pushed = next_json(&mut stream).await;
    assert_eq!(pushed["revision"], 1);
    assert_eq!(pushed["force_popup"], true);
}

async fn next_json(
    stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> Value {
    let msg = timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("timeout")
        .unwrap()
        .unwrap();
    serde_json::from_str(msg.to_text().unwrap()).unwrap()
}

async fn wait_close(
    stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> CloseFrame {
    loop {
        match timeout(Duration::from_secs(3), stream.next()).await {
            Ok(Some(Ok(Message::Close(Some(frame))))) => return frame,
            Ok(Some(Ok(Message::Close(None)))) => {
                return CloseFrame {
                    code: 1008.into(),
                    reason: "".into(),
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(_))) | Ok(None) | Err(_) => {
                panic!("expected a close frame")
            }
        }
    }
}
