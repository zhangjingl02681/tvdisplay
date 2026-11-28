use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;

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
