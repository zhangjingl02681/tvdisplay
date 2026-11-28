use std::collections::HashMap;
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

const FEISHU_KEYS: [&str; 3] = [
    "FEISHU_APP_ID",
    "FEISHU_APP_SECRET",
    "FEISHU_ALLOWED_CHAT_IDS",
];

pub fn load_settings_from_env() -> Result<Settings, ConfigError> {
    let environ: HashMap<String, String> = std::env::vars().collect();
    load_settings(&environ)
}

pub fn load_settings(environ: &HashMap<String, String>) -> Result<Settings, ConfigError> {
    let password = environ.get("ADMIN_PASSWORD").cloned().unwrap_or_default();
    if password.trim().is_empty() {
        return Err(ConfigError {
            message: "请设置环境变量 ADMIN_PASSWORD".into(),
        });
    }
    if PLACEHOLDER_PASSWORDS.contains(&password.as_str()) {
        return Err(ConfigError {
            message: "ADMIN_PASSWORD 还是示例值，请换成你自己的随机密码".into(),
        });
    }

    let host = environ
        .get("HOST")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("::")
        .to_string();

    let raw_port = environ.get("PORT").map(String::as_str).unwrap_or("8080");
    let port: u16 = raw_port.parse().map_err(|_| ConfigError {
        message: "PORT 必须是整数".into(),
    })?;
    if !(1..=65535).contains(&port) {
        return Err(ConfigError {
            message: "PORT 必须在 1 到 65535 之间".into(),
        });
    }

    let data_dir = PathBuf::from(environ.get("DATA_DIR").map(String::as_str).unwrap_or("data"));
    let feishu = load_feishu(environ)?;

    Ok(Settings {
        admin_password: password,
        data_dir,
        host,
        port,
        auth_timeout_seconds: 10.0,
        max_image_bytes: 10 * 1024 * 1024,
        text_max_chars: 4000,
        feishu,
    })
}

fn env_nonempty(environ: &HashMap<String, String>, key: &str) -> Option<String> {
    environ
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn load_feishu(environ: &HashMap<String, String>) -> Result<FeishuMode, ConfigError> {
    let present: Vec<bool> = FEISHU_KEYS
        .iter()
        .map(|key| env_nonempty(environ, key).is_some())
        .collect();
    if present.iter().all(|flag| !flag) {
        return Ok(FeishuMode::Disabled);
    }
    if present.iter().any(|flag| !flag) {
        let missing: Vec<&str> = FEISHU_KEYS
            .iter()
            .zip(present)
            .filter(|(_, present)| !present)
            .map(|(key, _)| *key)
            .collect();
        return Err(ConfigError {
            message: format!("飞书配置不完整，缺少 {}", missing.join("、")),
        });
    }
    let allowed_chat_ids = env_nonempty(environ, "FEISHU_ALLOWED_CHAT_IDS")
        .unwrap()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if allowed_chat_ids.is_empty() {
        return Err(ConfigError {
            message: "飞书配置不完整，缺少 FEISHU_ALLOWED_CHAT_IDS".into(),
        });
    }
    Ok(FeishuMode::Enabled(FeishuSettings {
        app_id: env_nonempty(environ, "FEISHU_APP_ID").unwrap(),
        app_secret: env_nonempty(environ, "FEISHU_APP_SECRET").unwrap(),
        allowed_chat_ids,
    }))
}
