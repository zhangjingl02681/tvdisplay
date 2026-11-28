use serde_json::json;
use std::time::Instant;
use tempfile::tempdir;
use tvdisplay::content::{ContentStore, MAX_IMAGE_BYTES};
use tvdisplay::feishu::drafts::{Draft, DraftStore};
use tvdisplay::feishu::{
    classify_receive_event, handle_card_action, ClassifyResult,
};
use tvdisplay::hub::TvHub;

fn receive_event(chat_id: &str, chat_type: &str, message_type: &str, content: &str, sender: &str) -> serde_json::Value {
    json!({
        "schema": "2.0",
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_type": sender },
            "message": {
                "message_id": "om_1",
                "chat_id": chat_id,
                "chat_type": chat_type,
                "message_type": message_type,
                "content": content
            }
        }
    })
}

#[test]
fn classifies_allowed_group_text_and_image_and_post() {
    let allowed = vec!["oc_allow".to_string()];
    match classify_receive_event(&receive_event("oc_allow", "group", "text", r#"{"text":"  你好 "}"#, "user"), &allowed) {
        ClassifyResult::Valid { text, image_key, .. } => {
            assert_eq!(text, "  你好 ");
            assert!(image_key.is_none());
        }
        other => panic!("{other:?}"),
    }
    match classify_receive_event(&receive_event("oc_allow", "group", "image", r#"{"image_key":"img_1"}"#, "user"), &allowed) {
        ClassifyResult::Valid { image_key, text, .. } => {
            assert_eq!(image_key.as_deref(), Some("img_1"));
            assert!(text.is_empty());
        }
        other => panic!("{other:?}"),
    }
    let post = r#"{"zh_cn":{"title":"","content":[[{"tag":"text","text":"菜单"}],[{"tag":"img","image_key":"img_2"}]]}}"#;
    match classify_receive_event(&receive_event("oc_allow", "group", "post", post, "user"), &allowed) {
        ClassifyResult::Valid { text, image_key, .. } => {
            assert!(text.contains("菜单"));
            assert_eq!(image_key.as_deref(), Some("img_2"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ignores_private_chats_other_groups_bots_and_stickers() {
    let allowed = vec!["oc_allow".to_string()];
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "p2p", "text", r#"{"text":"hi"}"#, "user"), &allowed), ClassifyResult::Ignore));
    assert!(matches!(classify_receive_event(&receive_event("oc_other", "group", "text", r#"{"text":"hi"}"#, "user"), &allowed), ClassifyResult::Ignore));
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "text", r#"{"text":"hi"}"#, "bot"), &allowed), ClassifyResult::Ignore));
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "sticker", r#"{}"#, "user"), &allowed), ClassifyResult::Ignore));
}

#[test]
fn videos_and_files_are_unsupported() {
    let allowed = vec!["oc_allow".to_string()];
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "media", r#"{}"#, "user"), &allowed), ClassifyResult::Unsupported { .. }));
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "file", r#"{}"#, "user"), &allowed), ClassifyResult::Unsupported { .. }));
}

#[tokio::test]
async fn card_click_publishes_once_and_then_reports_already_sent() {
    let dir = tempdir().unwrap();
    let store = std::sync::Arc::new(ContentStore::new(dir.path().to_path_buf()));
    let hub = TvHub::new();
    let mut drafts = DraftStore::new();
    let now = Instant::now();
    drafts.insert(
        Draft { id: "d1".into(), text: "今晚菜单".into(), image: Some(b"\xff\xd8\xff\xd9".to_vec()), created_at: now },
        now,
    );
    let payload = json!({
        "header": { "event_type": "card.action.trigger" },
        "event": { "action": { "value": { "draft_id": "d1", "force_popup": true }, "tag": "button" } }
    });
    let first = handle_card_action(&payload, &mut drafts, now, store.as_ref(), &hub, MAX_IMAGE_BYTES).await;
    assert_eq!(first["toast"]["content"], "已保存，但电视当前不在线");
    let content = store.load();
    assert_eq!(content.revision, 1);
    assert_eq!(content.text, "今晚菜单");
    assert!(content.force_popup);
    let second = handle_card_action(&payload, &mut drafts, now, store.as_ref(), &hub, MAX_IMAGE_BYTES).await;
    assert_eq!(second["toast"]["content"], "这条已经发送过了");
    assert_eq!(store.load().revision, 1);
}

#[tokio::test]
async fn expired_card_does_not_publish() {
    let dir = tempdir().unwrap();
    let store = std::sync::Arc::new(ContentStore::new(dir.path().to_path_buf()));
    let hub = TvHub::new();
    let drafts = DraftStore::new();
    let now = Instant::now();
    let payload = json!({
        "header": { "event_type": "card.action.trigger" },
        "event": { "action": { "value": { "draft_id": "missing", "force_popup": false }, "tag": "button" } }
    });
    let mut drafts = drafts;
    let response = handle_card_action(&payload, &mut drafts, now, store.as_ref(), &hub, MAX_IMAGE_BYTES).await;
    assert_eq!(response["toast"]["content"], "这条内容已过期，请重新发送");
    assert_eq!(store.load().revision, 0);
}
