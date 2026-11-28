use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[derive(Debug, Error)]
pub enum FeishuError {
    #[error("获取飞书访问凭证失败")]
    Token(String),
    #[error("无法下载图片，请重试")]
    Download,
    #[error("图片不能超过 10 MB")]
    ImageTooLarge,
    #[error("回复飞书消息失败")]
    Reply(String),
    #[error("飞书连接已断开")]
    Connection(&'static str),
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

pub struct FeishuApi {
    app_id: String,
    app_secret: String,
    open_base: String,
    accounts_base: String,
    http: Client,
    token: Mutex<Option<CachedToken>>,
}

impl FeishuApi {
    pub fn new(app_id: String, app_secret: String, open_base: String, accounts_base: String) -> Self {
        Self {
            app_id,
            app_secret,
            open_base: open_base.trim_end_matches('/').to_string(),
            accounts_base: accounts_base.trim_end_matches('/').to_string(),
            http: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
            token: Mutex::new(None),
        }
    }

    pub async fn tenant_access_token(&self) -> Result<String, FeishuError> {
        if let Some(cached) = self.token.lock().await.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(300) {
                return Ok(cached.value.clone());
            }
        }
        let url = format!("{}/oauth/v3/token", self.accounts_base);
        let response = self
            .http
            .post(url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!(
                "grant_type=client_credentials&client_id={}&client_secret={}",
                urlencoding(&self.app_id),
                urlencoding(&self.app_secret)
            ))
            .send()
            .await
            .map_err(|err| FeishuError::Token(err.to_string()))?;
        let body: TokenResponse = response
            .json()
            .await
            .map_err(|err| FeishuError::Token(err.to_string()))?;
        if body.code.unwrap_or(1) != 0 || body.access_token.is_none() {
            return Err(FeishuError::Token(body.error_description.unwrap_or_default()));
        }
        let value = body.access_token.unwrap();
        let expires_in = body.expires_in.unwrap_or(7200);
        *self.token.lock().await = Some(CachedToken {
            value: value.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in as u64),
        });
        Ok(value)
    }

    pub async fn download_message_image(
        &self,
        message_id: &str,
        file_key: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, FeishuError> {
        let token = self.tenant_access_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type=image",
            self.open_base
        );
        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|_| FeishuError::Download)?;
        if !response.status().is_success() {
            return Err(FeishuError::Download);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| FeishuError::Download)?;
            if bytes.len() + chunk.len() > max_bytes {
                return Err(FeishuError::ImageTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub async fn reply_text(&self, message_id: &str, text: &str) -> Result<(), FeishuError> {
        let content = serde_json::to_string(&json!({ "text": text })).unwrap();
        self.reply(message_id, "text", content).await
    }

    pub async fn reply_card(&self, message_id: &str, card: &Value) -> Result<(), FeishuError> {
        let content = serde_json::to_string(card).unwrap();
        self.reply(message_id, "interactive", content).await
    }

    async fn reply(&self, message_id: &str, msg_type: &str, content: String) -> Result<(), FeishuError> {
        let token = self.tenant_access_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/messages/{message_id}/reply",
            self.open_base
        );
        let response = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({ "msg_type": msg_type, "content": content }))
            .send()
            .await
            .map_err(|err| FeishuError::Reply(err.to_string()))?;
        let body: Value = response.json().await.map_err(|err| FeishuError::Reply(err.to_string()))?;
        if body.get("code").and_then(Value::as_i64) != Some(0) {
            return Err(FeishuError::Reply("non-zero code".into()));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    code: Option<i64>,
    access_token: Option<String>,
    expires_in: Option<i64>,
    error_description: Option<String>,
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub fn reconnect_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.min(20);
    (1000u64 << shift).min(30_000)
}

#[derive(Clone, Debug)]
pub struct WsEndpoint {
    pub url: String,
    pub ping_interval: Duration,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Header {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
}

pub fn encode_frame(frame: &Frame) -> Vec<u8> {
    frame.encode_to_vec()
}

pub fn decode_frame(bytes: &[u8]) -> Result<Frame, FeishuError> {
    Frame::decode(bytes).map_err(|_| FeishuError::Connection("frame"))
}

pub fn ping_frame(service_id: i32) -> Frame {
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: 0,
        headers: vec![Header {
            key: "type".into(),
            value: "ping".into(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

pub fn ack_frame(mut frame: Frame, biz_rt_ms: i64, body: Option<&Value>) -> Frame {
    frame.headers.push(Header {
        key: "biz_rt".into(),
        value: biz_rt_ms.to_string(),
    });
    let encoded = match body {
        Some(body) => {
            let raw = serde_json::to_vec(body).unwrap();
            base64::engine::general_purpose::STANDARD.encode(raw)
        }
        None => base64::engine::general_purpose::STANDARD.encode(b"null"),
    };
    let payload = json!({ "code": 200, "headers": Value::Null, "data": encoded });
    frame.payload = Some(serde_json::to_vec(&payload).unwrap());
    frame
}

pub trait LongConnectionHandler: Send + Sync + 'static {
    fn on_event(
        &self,
        event_type: String,
        payload: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = Option<Value>> + Send + '_>>;
}

impl FeishuApi {
    pub async fn fetch_ws_endpoint(&self) -> Result<WsEndpoint, FeishuError> {
        let url = format!("{}/callback/ws/endpoint", self.open_base);
        let response = self
            .http
            .post(url)
            .header("locale", "zh")
            .json(&json!({ "AppID": self.app_id, "AppSecret": self.app_secret }))
            .send()
            .await
            .map_err(|_| FeishuError::Connection("endpoint"))?;
        let body: Value = response
            .json()
            .await
            .map_err(|_| FeishuError::Connection("endpoint"))?;
        if body.get("code").and_then(Value::as_i64) != Some(0) {
            return Err(FeishuError::Connection("endpoint"));
        }
        let ws_url = body["data"]["URL"]
            .as_str()
            .ok_or(FeishuError::Connection("endpoint"))?
            .to_string();
        let ping_interval = body["data"]["ClientConfig"]["PingInterval"]
            .as_u64()
            .filter(|value| *value > 0)
            .unwrap_or(120);
        Ok(WsEndpoint {
            url: ws_url,
            ping_interval: Duration::from_secs(ping_interval),
        })
    }
}

struct PartialMessage {
    parts: Vec<Option<Vec<u8>>>,
    started: Instant,
}

pub async fn run_session<H: LongConnectionHandler>(
    endpoint: WsEndpoint,
    handler: H,
) -> Result<(), FeishuError> {
    let parsed = url::Url::parse(&endpoint.url).map_err(|_| FeishuError::Connection("url"))?;
    let service_id = parsed
        .query_pairs()
        .find(|(key, _)| key == "service_id")
        .map(|(_, value)| value.parse().unwrap_or(0))
        .unwrap_or(0);
    let (ws, _) = connect_async(endpoint.url.as_str())
        .await
        .map_err(|_| FeishuError::Connection("dial"))?;
    let (mut sink, mut stream) = ws.split();
    let mut ping = tokio::time::interval(endpoint.ping_interval);
    // interval 的第一次 tick 会立刻完成。先丢掉它，避免把心跳当成事件应答。
    ping.tick().await;
    let mut chunks: HashMap<String, PartialMessage> = HashMap::new();
    loop {
        tokio::select! {
            _ = ping.tick() => {
                let _ = sink
                    .send(WsMessage::Binary(encode_frame(&ping_frame(service_id)).into()))
                    .await;
            }
            incoming = stream.next() => {
                let Some(incoming) = incoming else {
                    return Err(FeishuError::Connection("closed"));
                };
                let Ok(WsMessage::Binary(bytes)) = incoming else {
                    continue;
                };
                let mut frame = decode_frame(&bytes).map_err(|_| FeishuError::Connection("frame"))?;
                if frame.method == 0 {
                    if let Some(seconds) = frame
                        .payload
                        .as_ref()
                        .and_then(|raw| serde_json::from_slice::<Value>(raw).ok())
                        .and_then(|value| value.get("PingInterval").and_then(Value::as_u64))
                        .filter(|value| *value > 0)
                    {
                        ping = tokio::time::interval(Duration::from_secs(seconds));
                        ping.tick().await;
                    }
                    continue;
                }
                let now = Instant::now();
                chunks.retain(|_, partial| now.duration_since(partial.started) < Duration::from_secs(5));
                let sum = frame_header(&frame, "sum")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(1);
                let seq = frame_header(&frame, "seq")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let mut payload_bytes = frame.payload.clone().unwrap_or_default();
                if sum > 1 {
                    let id = frame_header(&frame, "message_id").unwrap_or_default();
                    let partial = chunks.entry(id.clone()).or_insert_with(|| PartialMessage {
                        parts: vec![None; sum],
                        started: now,
                    });
                    if seq < partial.parts.len() {
                        partial.parts[seq] = Some(payload_bytes);
                    }
                    if partial.parts.iter().any(|part| part.is_none()) {
                        continue;
                    }
                    payload_bytes = partial.parts.iter().flatten().flatten().copied().collect();
                    chunks.remove(&id);
                }
                let typ = frame_header(&frame, "type").unwrap_or_default();
                if typ != "event" && typ != "card" {
                    continue;
                }
                let payload: Value = serde_json::from_slice(&payload_bytes).unwrap_or(Value::Null);
                let event_type = payload
                    .pointer("/header/event_type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let started = Instant::now();
                let response = handler.on_event(event_type, payload).await;
                let biz_rt = started.elapsed().as_millis() as i64;
                frame = ack_frame(frame, biz_rt, response.as_ref());
                sink.send(WsMessage::Binary(encode_frame(&frame).into()))
                    .await
                    .map_err(|_| FeishuError::Connection("ack"))?;
            }
        }
    }
}

fn frame_header(frame: &Frame, key: &str) -> Option<String> {
    frame
        .headers
        .iter()
        .find(|item| item.key == key)
        .map(|item| item.value.clone())
}
