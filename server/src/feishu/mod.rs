pub mod allowlist;
pub mod cards;
pub mod client;
pub mod drafts;

use crate::config::FeishuSettings;
use crate::content::{normalize_text, validate_image, ContentStore};
use crate::http::AppState;
use crate::hub::TvHub;
use allowlist::is_allowed_group;
use cards::{callback_response, confirmation_card, parse_button_value, result_card};
use client::{reconnect_backoff_ms, run_session, FeishuApi, FeishuError, LongConnectionHandler};
use drafts::{Draft, DraftError, DraftStore};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyResult {
    Ignore,
    Unsupported { message_id: String },
    Valid {
        message_id: String,
        chat_id: String,
        text: String,
        image_key: Option<String>,
    },
}

pub fn classify_receive_event(payload: &Value, allowed: &[String]) -> ClassifyResult {
    let event = &payload["event"];
    let sender_type = event["sender"]["sender_type"].as_str().unwrap_or("");
    if sender_type != "user" {
        return ClassifyResult::Ignore;
    }
    let message = &event["message"];
    let message_id = message["message_id"].as_str().unwrap_or("").to_string();
    let chat_id = message["chat_id"].as_str().unwrap_or("");
    let chat_type = message["chat_type"].as_str().unwrap_or("");
    if !is_allowed_group(chat_id, chat_type, allowed) {
        return ClassifyResult::Ignore;
    }
    let message_type = message["message_type"].as_str().unwrap_or("");
    if message_type == "sticker" || message_type == "system" {
        return ClassifyResult::Ignore;
    }
    if message_type == "media" || message_type == "file" || message_type == "audio" {
        return ClassifyResult::Unsupported { message_id };
    }
    let content_raw = message["content"].as_str().unwrap_or("{}");
    let content: Value = serde_json::from_str(content_raw).unwrap_or(Value::Null);
    let (text, image_key) = match message_type {
        "text" => (content["text"].as_str().unwrap_or("").to_string(), None),
        "image" => (
            String::new(),
            content["image_key"].as_str().map(ToOwned::to_owned),
        ),
        "post" => extract_post(&content),
        _ => return ClassifyResult::Unsupported { message_id },
    };
    if text.trim().is_empty() && image_key.is_none() {
        return ClassifyResult::Unsupported { message_id };
    }
    ClassifyResult::Valid {
        message_id,
        chat_id: chat_id.to_string(),
        text,
        image_key,
    }
}

fn extract_post(content: &Value) -> (String, Option<String>) {
    let body = content
        .get("zh_cn")
        .or_else(|| content.as_object().and_then(|map| map.values().next()));
    let mut texts = Vec::new();
    let mut image_key = None;
    if let Some(rows) = body
        .and_then(|value| value.get("content"))
        .and_then(Value::as_array)
    {
        for row in rows {
            let Some(items) = row.as_array() else {
                continue;
            };
            for item in items {
                if item["tag"] == "text" {
                    if let Some(text) = item["text"].as_str() {
                        texts.push(text.to_string());
                    }
                }
                if image_key.is_none() {
                    if let Some(key) = item["image_key"]
                        .as_str()
                        .or_else(|| item["img_key"].as_str())
                    {
                        image_key = Some(key.to_string());
                    }
                }
            }
        }
    }
    (texts.join(""), image_key)
}

pub async fn handle_card_action(
    payload: &Value,
    drafts: &mut DraftStore,
    now: Instant,
    store: &ContentStore,
    hub: &TvHub,
    max_bytes: usize,
) -> Value {
    let Some(action) = payload
        .pointer("/event/action/value")
        .and_then(parse_button_value)
    else {
        return callback_response("error", "请发送图片或文字", None);
    };
    match drafts.take_for_publish(&action.draft_id, now) {
        Err(DraftError::AlreadySent) => callback_response("info", "这条已经发送过了", None),
        Err(DraftError::ExpiredOrMissing) => {
            callback_response("warning", "这条内容已过期，请重新发送", None)
        }
        Ok(draft) => {
            let text = match normalize_text(Some(&draft.text), 4000) {
                Ok(text) => text,
                Err(err) => return callback_response("error", &err.message, None),
            };
            if let Some(image) = draft.image.as_deref() {
                if let Err(err) = validate_image(image, max_bytes) {
                    return callback_response("error", &err.message, None);
                }
            }
            let has_image = draft.image.is_some();
            if let Err(err) = store.publish(
                draft.image.as_deref(),
                &text,
                action.force_popup,
                max_bytes,
            ) {
                return callback_response("error", &err.message, None);
            }
            let payload = serde_json::to_string(&store.message()).expect("ws message");
            let online = hub.push(&payload).await;
            let status = if online {
                "发送成功"
            } else {
                "已保存，但电视当前不在线"
            };
            callback_response(
                "success",
                status,
                Some(result_card(&text, has_image, status)),
            )
        }
    }
}

pub async fn handle_receive(
    payload: Value,
    allowed: &[String],
    api: &FeishuApi,
    drafts: &Mutex<DraftStore>,
    max_bytes: usize,
    seen: &Mutex<HashSet<String>>,
) {
    match classify_receive_event(&payload, allowed) {
        ClassifyResult::Ignore => {}
        ClassifyResult::Unsupported { message_id } => {
            let _ = api.reply_text(&message_id, "请发送图片或文字").await;
        }
        ClassifyResult::Valid {
            message_id,
            text,
            image_key,
            ..
        } => {
            {
                let mut seen = seen.lock().await;
                if !seen.insert(message_id.clone()) {
                    return;
                }
            }
            let normalized = match normalize_text(Some(&text), 4000) {
                Ok(value) => value,
                Err(err) => {
                    let _ = api.reply_text(&message_id, &err.message).await;
                    return;
                }
            };
            let image = match image_key {
                Some(key) => match api.download_message_image(&message_id, &key, max_bytes).await {
                    Ok(bytes) => match validate_image(&bytes, max_bytes) {
                        Ok(_) => Some(bytes),
                        Err(err) => {
                            let _ = api.reply_text(&message_id, &err.message).await;
                            return;
                        }
                    },
                    Err(FeishuError::ImageTooLarge) => {
                        let _ = api.reply_text(&message_id, "图片不能超过 10 MB").await;
                        return;
                    }
                    Err(_) => {
                        let _ = api.reply_text(&message_id, "无法下载图片，请重试").await;
                        return;
                    }
                },
                None => None,
            };
            if image.is_none() && normalized.is_empty() {
                let _ = api.reply_text(&message_id, "请发送图片或文字").await;
                return;
            }
            let draft = Draft {
                id: Uuid::new_v4().to_string(),
                text: normalized,
                image,
                created_at: Instant::now(),
            };
            let card = confirmation_card(&draft.id, &draft.text, draft.image.is_some());
            {
                let mut store = drafts.lock().await;
                let _ = store.insert(draft, Instant::now());
            }
            let _ = api.reply_card(&message_id, &card).await;
        }
    }
}

struct BotHandler {
    state: AppState,
    allowed: Vec<String>,
    seen: Arc<Mutex<HashSet<String>>>,
}

impl LongConnectionHandler for BotHandler {
    fn on_event(
        &self,
        event_type: String,
        payload: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Value>> + Send + '_>> {
        Box::pin(async move {
            match event_type.as_str() {
                "im.message.receive_v1" => {
                    let state = self.state.clone();
                    let allowed = self.allowed.clone();
                    let seen = self.seen.clone();
                    tokio::spawn(async move {
                        let Some(api) = state.feishu_api.as_ref() else {
                            return;
                        };
                        handle_receive(
                            payload,
                            &allowed,
                            api,
                            &state.drafts,
                            state.settings.max_image_bytes,
                            &seen,
                        )
                        .await;
                    });
                    None
                }
                "card.action.trigger" => {
                    let mut drafts = self.state.drafts.lock().await;
                    Some(
                        handle_card_action(
                            &payload,
                            &mut drafts,
                            Instant::now(),
                            self.state.store.as_ref(),
                            &self.state.hub,
                            self.state.settings.max_image_bytes,
                        )
                        .await,
                    )
                }
                _ => None,
            }
        })
    }
}

pub async fn run_feishu_bot(state: AppState, feishu: FeishuSettings) {
    let api = state.feishu_api.clone().expect("feishu api");
    let mut attempt: u32 = 0;
    loop {
        match api.fetch_ws_endpoint().await {
            Ok(endpoint) => {
                tracing::info!("飞书已连接");
                attempt = 0;
                let handler = BotHandler {
                    state: state.clone(),
                    allowed: feishu.allowed_chat_ids.clone(),
                    seen: Arc::new(Mutex::new(HashSet::new())),
                };
                if run_session(endpoint, handler).await.is_err() {
                    tracing::info!("飞书连接已断开");
                }
            }
            Err(_) => {
                tracing::info!("飞书连接已断开");
            }
        }
        let delay = reconnect_backoff_ms(attempt);
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
}
