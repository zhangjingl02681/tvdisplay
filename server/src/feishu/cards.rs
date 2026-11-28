use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonAction {
    pub draft_id: String,
    pub force_popup: bool,
}

pub fn confirmation_card(draft_id: &str, text: &str, has_image: bool) -> Value {
    let summary = if text.is_empty() {
        "（没有文字）".to_string()
    } else {
        truncate_chars(text, 80)
    };
    let image_line = if has_image { "带图片：是" } else { "带图片：否" };
    json!({
        "schema": "2.0",
        "header": {
            "title": { "tag": "plain_text", "content": "发布到电视" },
            "template": "blue"
        },
        "body": {
            "elements": [
                {
                    "tag": "div",
                    "text": { "tag": "plain_text", "content": format!("{summary}\n{image_line}") }
                },
                button("强制弹出", "primary", draft_id, true),
                button("普通显示", "default", draft_id, false)
            ]
        }
    })
}

pub fn result_card(text: &str, has_image: bool, status: &str) -> Value {
    let summary = if text.is_empty() {
        "（没有文字）".to_string()
    } else {
        truncate_chars(text, 80)
    };
    let image_line = if has_image { "带图片：是" } else { "带图片：否" };
    json!({
        "schema": "2.0",
        "header": {
            "title": { "tag": "plain_text", "content": "发布到电视" },
            "template": "green"
        },
        "body": {
            "elements": [
                {
                    "tag": "div",
                    "text": { "tag": "plain_text", "content": format!("{summary}\n{image_line}\n{status}") }
                }
            ]
        }
    })
}

pub fn parse_button_value(value: &Value) -> Option<ButtonAction> {
    Some(ButtonAction {
        draft_id: value.get("draft_id")?.as_str()?.to_string(),
        force_popup: value.get("force_popup")?.as_bool()?,
    })
}

pub fn callback_response(toast_type: &str, toast: &str, card: Option<Value>) -> Value {
    let mut body = json!({
        "toast": { "type": toast_type, "content": toast }
    });
    if let Some(card) = card {
        body["card"] = json!({ "type": "raw", "data": card });
    }
    body
}

fn button(label: &str, typ: &str, draft_id: &str, force_popup: bool) -> Value {
    json!({
        "tag": "button",
        "type": typ,
        "text": { "tag": "plain_text", "content": label },
        "behaviors": [{
            "type": "callback",
            "value": { "draft_id": draft_id, "force_popup": force_popup }
        }]
    })
}

fn truncate_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}
