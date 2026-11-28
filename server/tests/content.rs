use base64::Engine;
use serde_json::json;
use tempfile::tempdir;
use tvdisplay::content::{
    normalize_text, parse_force_popup, validate_image, ContentStore, ContentValidationError,
    MAX_IMAGE_BYTES,
};

const JPEG: &[u8] = b"\xff\xd8\xff\xd9";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

#[test]
fn default_image_limit_is_ten_mebibytes() {
    assert_eq!(MAX_IMAGE_BYTES, 10 * 1024 * 1024);
}

#[test]
fn jpeg_and_png_magic_are_accepted_regardless_of_name() {
    assert_eq!(validate_image(JPEG, MAX_IMAGE_BYTES).unwrap(), "image/jpeg");
    assert_eq!(validate_image(PNG, MAX_IMAGE_BYTES).unwrap(), "image/png");
}

#[test]
fn empty_gif_and_oversize_are_rejected() {
    let empty = validate_image(b"", MAX_IMAGE_BYTES).unwrap_err();
    assert_eq!(empty.message, "请发送图片或文字");
    let gif = validate_image(b"GIF89a-not-a-photo", MAX_IMAGE_BYTES).unwrap_err();
    assert_eq!(gif.message, "只支持 JPEG 或 PNG");
    let huge = validate_image(b"\xff\xd8\xffxxxxxxxx", 10).unwrap_err();
    assert!(huge.message.contains("10 MB"));
}

#[test]
fn text_is_optional_and_bounded() {
    assert_eq!(normalize_text(None, 4000).unwrap(), "");
    assert_eq!(normalize_text(Some("  你好 \r\n"), 4000).unwrap(), "你好");
    let err = normalize_text(Some(&"字".repeat(12)), 8).unwrap_err();
    assert!(err.message.contains("4000") || err.message.contains("8") || err.message.contains("说明文字"));
}

#[test]
fn force_popup_parser() {
    assert!(parse_force_popup(Some("on")));
    assert!(parse_force_popup(Some("true")));
    assert!(!parse_force_popup(None));
    assert!(!parse_force_popup(Some("off")));
}

#[test]
fn store_roundtrip_keeps_image_text_and_toggle() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    let saved = store.publish(Some(JPEG), "今晚菜单", true, MAX_IMAGE_BYTES).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.text, "今晚菜单");
    assert!(saved.force_popup);
    assert_eq!(saved.mime.as_deref(), Some("image/jpeg"));
    assert_eq!(saved.image.as_deref(), Some(JPEG));

    let reloaded = ContentStore::new(dir.path().to_path_buf()).load();
    assert_eq!(reloaded.image.as_deref(), Some(JPEG));
    assert_eq!(reloaded.text, "今晚菜单");
    assert!(reloaded.force_popup);
    assert_eq!(reloaded.revision, 1);
}

#[test]
fn replacing_png_removes_previous_jpeg() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(JPEG), "", false, MAX_IMAGE_BYTES).unwrap();
    store.publish(Some(PNG), "第二张", false, MAX_IMAGE_BYTES).unwrap();
    assert!(!dir.path().join("image.jpg").exists());
    assert_eq!(std::fs::read(dir.path().join("image.png")).unwrap(), PNG);
    assert_eq!(store.load().revision, 2);
}

#[test]
fn text_only_publish_removes_leftover_images() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(JPEG), "旧图", false, MAX_IMAGE_BYTES).unwrap();
    let saved = store.publish(None, "只有文字", true, MAX_IMAGE_BYTES).unwrap();
    assert_eq!(saved.revision, 2);
    assert_eq!(saved.text, "只有文字");
    assert!(saved.image.is_none());
    assert!(saved.mime.is_none());
    assert!(!dir.path().join("image.jpg").exists());
    assert!(!dir.path().join("image.png").exists());
    let message = store.message();
    assert_eq!(message.revision, 2);
    assert_eq!(message.text, "只有文字");
    assert!(message.image_mime.is_none());
    assert!(message.image_base64.is_none());
}

#[test]
fn publish_rejects_when_both_image_and_text_are_missing() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    let err: ContentValidationError = store.publish(None, "", false, MAX_IMAGE_BYTES).unwrap_err();
    assert_eq!(err.message, "请发送图片或文字");
}

#[test]
fn toggle_persists_without_bumping_revision() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(JPEG), "字幕", false, MAX_IMAGE_BYTES).unwrap();
    let updated = store.set_force_popup(true);
    assert!(updated.force_popup);
    assert_eq!(updated.revision, 1);
    assert!(ContentStore::new(dir.path().to_path_buf()).load().force_popup);
}

#[test]
fn toggle_can_be_saved_before_any_content() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.set_force_popup(true);
    let loaded = store.load();
    assert!(loaded.force_popup);
    assert!(!loaded.has_image());
    assert_eq!(loaded.revision, 0);
}

#[test]
fn message_includes_base64_image() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(PNG), "", true, MAX_IMAGE_BYTES).unwrap();
    let message = store.message();
    assert_eq!(message.type_, "content");
    assert!(message.force_popup);
    assert_eq!(message.image_mime.as_deref(), Some("image/png"));
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(message.image_base64.as_ref().unwrap())
        .unwrap();
    assert_eq!(decoded, PNG);
}

#[test]
fn empty_message_when_nothing_published() {
    let dir = tempdir().unwrap();
    let message = ContentStore::new(dir.path().to_path_buf()).message();
    assert_eq!(message.revision, 0);
    assert_eq!(message.text, "");
    assert!(message.image_base64.is_none());
    assert!(message.image_mime.is_none());
    assert!(!message.force_popup);
}

#[test]
fn load_reads_existing_python_content_json() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("image.jpg"), JPEG).unwrap();
    std::fs::write(
        dir.path().join("content.json"),
        serde_json::to_vec_pretty(&json!({
            "text": "旧服务写下的内容",
            "force_popup": true,
            "mime": "image/jpeg",
            "revision": 4,
            "updated_at": "2026-11-26T06:00:00+00:00",
            "image_name": "image.jpg"
        }))
        .unwrap(),
    )
    .unwrap();
    let loaded = ContentStore::new(dir.path().to_path_buf()).load();
    assert_eq!(loaded.revision, 4);
    assert_eq!(loaded.text, "旧服务写下的内容");
    assert!(loaded.force_popup);
    assert_eq!(loaded.mime.as_deref(), Some("image/jpeg"));
    assert_eq!(loaded.image.as_deref(), Some(JPEG));
    assert_eq!(loaded.updated_at.as_deref(), Some("2026-11-26T06:00:00+00:00"));
}
