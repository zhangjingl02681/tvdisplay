use std::path::PathBuf;

pub const PLACEHOLDER_PASSWORDS: &[&str] = &[
    "change-me",
    "password",
    "admin",
    "请改成一长串随机密码",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeishuSettings {
    pub app_id: String,
    pub app_secret: String,
    pub allowed_chat_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeishuMode {
    Disabled,
    Enabled(FeishuSettings),
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub admin_password: String,
    pub data_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub auth_timeout_seconds: f64,
    pub max_image_bytes: usize,
    pub text_max_chars: usize,
    pub feishu: FeishuMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ConfigError {}
