use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub const TEXT_MAX_CHARS: usize = 4000;

const JPEG_MAGIC: &[u8] = b"\xff\xd8\xff";
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentValidationError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayContent {
    pub text: String,
    pub force_popup: bool,
    pub mime: Option<String>,
    pub revision: i64,
    pub updated_at: Option<String>,
    pub image: Option<Vec<u8>>,
}

impl DisplayContent {
    pub fn has_image(&self) -> bool {
        self.image.as_ref().is_some_and(|bytes| !bytes.is_empty())
    }

    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsContentMessage {
    #[serde(rename = "type")]
    pub type_: String,
    pub revision: i64,
    pub text: String,
    pub force_popup: bool,
    pub image_mime: Option<String>,
    pub image_base64: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentMeta {
    text: String,
    force_popup: bool,
    mime: Option<String>,
    revision: i64,
    updated_at: Option<String>,
    image_name: Option<String>,
}

pub struct ContentStore {
    root: PathBuf,
    meta_path: PathBuf,
    lock: Mutex<()>,
}

impl ContentStore {
    pub fn new(root: PathBuf) -> Self {
        fs::create_dir_all(&root).expect("create content data directory");
        let meta_path = root.join("content.json");
        Self {
            root,
            meta_path,
            lock: Mutex::new(()),
        }
    }

    pub fn load(&self) -> DisplayContent {
        let _guard = self.lock.lock().expect("content lock");
        self.load_unlocked()
    }

    pub fn publish(
        &self,
        image: Option<&[u8]>,
        text: &str,
        force_popup: bool,
        max_bytes: usize,
    ) -> Result<DisplayContent, ContentValidationError> {
        let _guard = self.lock.lock().expect("content lock");
        let has_text = !text.is_empty();
        let mime = match image {
            Some(bytes) => Some(validate_image(bytes, max_bytes)?),
            None => None,
        };
        if mime.is_none() && !has_text {
            return Err(ContentValidationError {
                message: "请发送图片或文字".into(),
            });
        }
        let current = self.load_unlocked();
        let revision = current.revision + 1;
        let updated_at = now_utc_seconds();
        match mime {
            Some(mime) => {
                let name = image_name(mime);
                write_bytes(&self.root.join(name), image.unwrap());
                let other = if name == "image.png" {
                    "image.jpg"
                } else {
                    "image.png"
                };
                let leftover = self.root.join(other);
                if leftover.exists() {
                    let _ = fs::remove_file(leftover);
                }
                self.write_meta(&ContentMeta {
                    text: text.to_string(),
                    force_popup,
                    mime: Some(mime.to_string()),
                    revision,
                    updated_at: Some(updated_at),
                    image_name: Some(name.to_string()),
                });
            }
            None => {
                for name in ["image.jpg", "image.png"] {
                    let leftover = self.root.join(name);
                    if leftover.exists() {
                        let _ = fs::remove_file(leftover);
                    }
                }
                self.write_meta(&ContentMeta {
                    text: text.to_string(),
                    force_popup,
                    mime: None,
                    revision,
                    updated_at: Some(updated_at),
                    image_name: None,
                });
            }
        }
        Ok(self.load_unlocked())
    }

    pub fn set_force_popup(&self, force_popup: bool) -> DisplayContent {
        let _guard = self.lock.lock().expect("content lock");
        let current = self.load_unlocked();
        let mut meta = self.read_meta();
        let image_name = if current.has_image() {
            meta.image_name.clone()
        } else {
            None
        };
        meta.text = current.text;
        meta.force_popup = force_popup;
        meta.mime = current.mime;
        meta.revision = current.revision;
        meta.updated_at = current.updated_at;
        meta.image_name = image_name;
        self.write_meta(&meta);
        self.load_unlocked()
    }

    pub fn message(&self) -> WsContentMessage {
        let content = self.load();
        let image_base64 = content.image.as_ref().map(|bytes| {
            base64::engine::general_purpose::STANDARD.encode(bytes)
        });
        WsContentMessage {
            type_: "content".into(),
            revision: content.revision,
            text: content.text,
            force_popup: content.force_popup,
            image_mime: content.mime,
            image_base64,
            updated_at: content.updated_at,
        }
    }

    fn load_unlocked(&self) -> DisplayContent {
        let meta = self.read_meta();
        let mut mime = meta.mime.filter(|value| !value.is_empty());
        let mut image = None;
        if let Some(name) = meta.image_name.as_deref().filter(|name| !name.is_empty()) {
            let path = self.root.join(name);
            if path.is_file() {
                image = fs::read(path).ok();
            } else {
                mime = None;
            }
        }
        DisplayContent {
            text: meta.text,
            force_popup: meta.force_popup,
            mime,
            revision: meta.revision,
            updated_at: meta.updated_at,
            image,
        }
    }

    fn read_meta(&self) -> ContentMeta {
        let Ok(raw) = fs::read_to_string(&self.meta_path) else {
            return ContentMeta::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    fn write_meta(&self, meta: &ContentMeta) {
        let payload = serde_json::to_vec_pretty(meta).expect("content meta is serializable");
        write_bytes(&self.meta_path, &payload);
    }
}

impl Default for ContentMeta {
    fn default() -> Self {
        Self {
            text: String::new(),
            force_popup: false,
            mime: None,
            revision: 0,
            updated_at: None,
            image_name: None,
        }
    }
}

pub fn sniff_image_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(PNG_MAGIC) {
        Some("image/png")
    } else if data.starts_with(JPEG_MAGIC) {
        Some("image/jpeg")
    } else {
        None
    }
}

pub fn validate_image(data: &[u8], max_bytes: usize) -> Result<&'static str, ContentValidationError> {
    if data.is_empty() {
        return Err(ContentValidationError {
            message: "请发送图片或文字".into(),
        });
    }
    if data.len() > max_bytes {
        return Err(ContentValidationError {
            message: "图片不能超过 10 MB".into(),
        });
    }
    sniff_image_mime(data).ok_or(ContentValidationError {
        message: "只支持 JPEG 或 PNG".into(),
    })
}

pub fn normalize_text(text: Option<&str>, max_chars: usize) -> Result<String, ContentValidationError> {
    let value = text.unwrap_or("").replace("\r\n", "\n");
    let value = value.trim().to_string();
    if value.chars().count() > max_chars {
        return Err(ContentValidationError {
            message: "说明文字不能超过 4000 字".into(),
        });
    }
    Ok(value)
}

pub fn parse_force_popup(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "on" | "yes")
    )
}

fn image_name(mime: &str) -> &'static str {
    if mime == "image/png" {
        "image.png"
    } else {
        "image.jpg"
    }
}

fn write_bytes(path: &Path, data: &[u8]) {
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&temporary, data).expect("write temp content file");
    fs::rename(&temporary, path).expect("replace content file");
}

fn now_utc_seconds() -> String {
    let now = OffsetDateTime::now_utc().replace_offset(UtcOffset::UTC);
    // Python timespec=seconds 不带小数，偏移是 +00:00 而不是 Z。
    let format = time::format_description::parse_borrowed::<1>(
        "[year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]",
    )
    .expect("timestamp format");
    now.format(&format).unwrap_or_else(|_| {
        now.format(&Rfc3339).unwrap_or_default().replace('Z', "+00:00")
    })
}
