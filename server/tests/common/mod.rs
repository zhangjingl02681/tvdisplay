use std::collections::HashMap;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tvdisplay::config::{FeishuMode, Settings};
use tvdisplay::http::{build_state, router, AppState};

pub const PASSWORD: &str = "unit-test-password";
pub const JPEG: &[u8] = b"\xff\xd8\xff\xd9";
#[allow(dead_code)]
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest-of-png";

pub struct TestApp {
    pub base: String,
    #[allow(dead_code)]
    pub state: AppState,
    pub client: reqwest::Client,
    _dir: TempDir,
}

pub fn test_settings(dir: &std::path::Path) -> Settings {
    Settings {
        admin_password: PASSWORD.to_string(),
        data_dir: dir.to_path_buf(),
        host: "127.0.0.1".into(),
        port: 0,
        auth_timeout_seconds: 0.4,
        max_image_bytes: 64,
        text_max_chars: 20,
        feishu: FeishuMode::Disabled,
    }
}

pub async fn spawn_app() -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let settings = test_settings(dir.path());
    let state = build_state(settings);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    TestApp {
        base: format!("http://127.0.0.1:{}", addr.port()),
        state,
        client,
        _dir: dir,
    }
}

impl TestApp {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub async fn login(&self) -> reqwest::Response {
        self.client
            .post(self.url("/login"))
            .form(&HashMap::from([("password", PASSWORD)]))
            .send()
            .await
            .unwrap()
    }
}
