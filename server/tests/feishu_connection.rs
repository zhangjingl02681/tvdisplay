use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tvdisplay::feishu::client::{
    ack_frame, decode_frame, encode_frame, ping_frame, reconnect_backoff_ms, FeishuApi, Frame,
    Header, LongConnectionHandler,
};

#[test]
fn backoff_matches_the_spec() {
    assert_eq!(reconnect_backoff_ms(0), 1000);
    assert_eq!(reconnect_backoff_ms(1), 2000);
    assert_eq!(reconnect_backoff_ms(2), 4000);
    assert_eq!(reconnect_backoff_ms(4), 16000);
    assert_eq!(reconnect_backoff_ms(5), 30000);
    assert_eq!(reconnect_backoff_ms(20), 30000);
}

#[test]
fn ping_and_ack_frames_roundtrip() {
    let encoded = encode_frame(&ping_frame(42));
    let decoded = decode_frame(&encoded).unwrap();
    assert_eq!(decoded.method, 0);
    assert_eq!(decoded.service, 42);
    assert_eq!(header(&decoded, "type").as_deref(), Some("ping"));

    let mut event = Frame {
        seq_id: 1,
        log_id: 2,
        service: 42,
        method: 1,
        headers: vec![
            Header { key: "type".into(), value: "event".into() },
            Header { key: "message_id".into(), value: "m1".into() },
            Header { key: "sum".into(), value: "1".into() },
            Header { key: "seq".into(), value: "0".into() },
            Header { key: "trace_id".into(), value: "t1".into() },
        ],
        payload_encoding: None,
        payload_type: None,
        payload: Some(br#"{"header":{"event_type":"im.message.receive_v1"}}"#.to_vec()),
        log_id_new: None,
    };
    event = ack_frame(event, 3, Some(&json!({"toast":{"content":"ok"}})));
    let payload: serde_json::Value = serde_json::from_slice(event.payload.as_ref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);
    assert!(payload["data"].as_str().is_some());
    assert_eq!(header(&event, "biz_rt").as_deref(), Some("3"));
}

fn header(frame: &Frame, key: &str) -> Option<String> {
    frame.headers.iter().find(|item| item.key == key).map(|item| item.value.clone())
}

struct RecordingHandler {
    received: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LongConnectionHandler for RecordingHandler {
    fn on_event(
        &self,
        event_type: String,
        _payload: serde_json::Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<serde_json::Value>> + Send + '_>>
    {
        let received = self.received.clone();
        Box::pin(async move {
            received.lock().unwrap().push(event_type);
            None
        })
    }
}

#[tokio::test]
async fn fetch_endpoint_reads_url_and_ping_interval() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/callback/ws/endpoint",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "URL": "wss://example.invalid/ws?device_id=dev&service_id=9",
                        "ClientConfig": { "PingInterval": 30, "ReconnectInterval": 999 }
                    }
                }))
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    let api = FeishuApi::new(
        "cli_test".into(),
        "app-secret".into(),
        format!("http://{addr}"),
        format!("http://{addr}"),
    );
    let endpoint = api.fetch_ws_endpoint().await.unwrap();
    assert!(endpoint.url.contains("device_id=dev"));
    assert_eq!(endpoint.ping_interval, Duration::from_secs(30));
}

#[tokio::test]
async fn session_acks_event_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let frame = Frame {
            seq_id: 1,
            log_id: 1,
            service: 9,
            method: 1,
            headers: vec![
                Header { key: "type".into(), value: "event".into() },
                Header { key: "message_id".into(), value: "m-event".into() },
                Header { key: "sum".into(), value: "1".into() },
                Header { key: "seq".into(), value: "0".into() },
                Header { key: "trace_id".into(), value: "tr".into() },
            ],
            payload_encoding: None,
            payload_type: None,
            payload: Some(json!({"schema":"2.0","header":{"event_type":"im.message.receive_v1"},"event":{}}).to_string().into_bytes()),
            log_id_new: None,
        };
        ws.send(Message::Binary(encode_frame(&frame).into())).await.unwrap();
        let reply = loop {
            match ws.next().await {
                Some(Ok(Message::Binary(bytes))) => break bytes,
                Some(Ok(_)) => continue,
                other => panic!("expected ack, got {other:?}"),
            }
        };
        let _ = ack_tx.send(reply.to_vec());
        let _ = ready_tx.send(());
    });

    // 等待服务器就绪：上面 accept 在连接后才运行，所以先连
    let url = format!("ws://{addr}/?device_id=dev&service_id=9");
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let handler = RecordingHandler { received: received.clone() };
    let session = tokio::spawn(async move {
        tvdisplay::feishu::client::run_session(
            tvdisplay::feishu::client::WsEndpoint {
                url,
                ping_interval: Duration::from_secs(120),
            },
            handler,
        )
        .await
    });
    let ack_bytes = tokio::time::timeout(Duration::from_secs(3), ack_rx)
        .await
        .expect("ack")
        .unwrap();
    let ack = decode_frame(&ack_bytes).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(ack.payload.as_ref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);
    assert!(received.lock().unwrap().contains(&"im.message.receive_v1".to_string()));
    session.abort();
    let _ = ready_rx.await;
}
