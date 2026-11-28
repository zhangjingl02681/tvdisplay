use serde_json::json;
use tvdisplay::feishu::cards::{
    callback_response, confirmation_card, parse_button_value, result_card,
};

#[test]
fn confirmation_card_has_two_callback_buttons() {
    let card = confirmation_card("draft-1", "今晚菜单", true);
    assert_eq!(card["schema"], "2.0");
    let rendered = card.to_string();
    assert!(rendered.contains("今晚菜单"));
    assert!(rendered.contains("带图片：是"));
    assert!(rendered.contains("强制弹出"));
    assert!(rendered.contains("普通显示"));
    let buttons: Vec<_> = card["body"]["elements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|element| element["tag"] == "button")
        .collect();
    assert_eq!(buttons.len(), 2);
    let values: Vec<_> = buttons
        .iter()
        .map(|button| button["behaviors"][0]["value"].clone())
        .collect();
    assert!(values.contains(&json!({"draft_id":"draft-1","force_popup":true})));
    assert!(values.contains(&json!({"draft_id":"draft-1","force_popup":false})));
}

#[test]
fn parse_button_value_reads_force_popup() {
    let action = parse_button_value(&json!({"draft_id":"d1","force_popup":false})).unwrap();
    assert_eq!(action.draft_id, "d1");
    assert!(!action.force_popup);
}

#[test]
fn callback_response_includes_toast_and_raw_card() {
    let card = result_card("今晚菜单", true, "发送成功");
    let response = callback_response("success", "发送成功", Some(card.clone()));
    assert_eq!(response["toast"]["type"], "success");
    assert_eq!(response["toast"]["content"], "发送成功");
    assert_eq!(response["card"]["type"], "raw");
    assert_eq!(response["card"]["data"]["schema"], "2.0");
    assert!(response["card"]["data"].to_string().contains("发送成功"));
    assert!(!response["card"]["data"].to_string().contains("强制弹出"));
}
