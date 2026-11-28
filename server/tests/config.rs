use std::collections::HashMap;
use std::path::PathBuf;
use tvdisplay::config::{load_settings, PLACEHOLDER_PASSWORDS};
use tvdisplay::health::health_urls;

#[test]
fn load_settings_reads_host_port_and_directory() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("HOST".into(), "::".into());
    env.insert("PORT".into(), "8080".into());
    env.insert("DATA_DIR".into(), "/tmp/tvdisplay-data".into());
    let settings = load_settings(&env).expect("settings should load");
    assert_eq!(settings.host, "::");
    assert_eq!(settings.port, 8080);
    assert_eq!(settings.data_dir, PathBuf::from("/tmp/tvdisplay-data"));
    assert_eq!(settings.admin_password, "a-long-random-secret");
    assert_eq!(settings.auth_timeout_seconds, 10.0);
    assert_eq!(settings.max_image_bytes, 10 * 1024 * 1024);
    assert_eq!(settings.text_max_chars, 4000);
}

#[test]
fn missing_password_exits_without_leaking_a_value() {
    let err = load_settings(&HashMap::new()).expect_err("must refuse");
    assert!(err.message.contains("ADMIN_PASSWORD"));
    assert!(!err.message.contains("change-me"));
}

#[test]
fn placeholder_passwords_are_rejected() {
    for placeholder in PLACEHOLDER_PASSWORDS {
        let mut env = HashMap::new();
        env.insert("ADMIN_PASSWORD".into(), (*placeholder).into());
        env.insert("DATA_DIR".into(), "/tmp/x".into());
        let err = load_settings(&env).expect_err(placeholder);
        assert!(
            !err.message.contains(placeholder),
            "exit text leaked placeholder {placeholder}"
        );
    }
}

#[test]
fn bad_port_is_rejected() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("PORT".into(), "nope".into());
    assert!(load_settings(&env).is_err());
}

#[test]
fn health_urls_cover_ipv6_and_localhost() {
    assert_eq!(
        health_urls("::", "8080"),
        vec![
            "http://[::1]:8080/health".to_string(),
            "http://127.0.0.1:8080/health".to_string(),
        ]
    );
    assert_eq!(
        health_urls("127.0.0.1", "8080"),
        vec!["http://127.0.0.1:8080/health".to_string()]
    );
    assert_eq!(
        health_urls("2001:db8::1", "9090"),
        vec!["http://[2001:db8::1]:9090/health".to_string()]
    );
}

#[test]
fn all_feishu_vars_missing_disables_feishu() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    let settings = load_settings(&env).expect("web and tv must still start");
    assert_eq!(settings.feishu, tvdisplay::config::FeishuMode::Disabled);
}

#[test]
fn partial_feishu_config_is_rejected_without_leaking_secret() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("FEISHU_APP_ID".into(), "cli_test".into());
    env.insert("FEISHU_APP_SECRET".into(), "super-secret-value".into());
    let err = load_settings(&env).expect_err("partial feishu config");
    assert!(err.message.contains("FEISHU_ALLOWED_CHAT_IDS"));
    assert!(!err.message.contains("super-secret-value"));
}

#[test]
fn complete_feishu_config_trims_chat_ids() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("FEISHU_APP_ID".into(), "cli_test".into());
    env.insert("FEISHU_APP_SECRET".into(), "super-secret-value".into());
    env.insert(
        "FEISHU_ALLOWED_CHAT_IDS".into(),
        " oc_aaa ,oc_bbb ".into(),
    );
    let settings = load_settings(&env).unwrap();
    match settings.feishu {
        tvdisplay::config::FeishuMode::Enabled(feishu) => {
            assert_eq!(feishu.app_id, "cli_test");
            assert_eq!(feishu.app_secret, "super-secret-value");
            assert_eq!(feishu.allowed_chat_ids, vec!["oc_aaa", "oc_bbb"]);
        }
        tvdisplay::config::FeishuMode::Disabled => panic!("expected enabled"),
    }
}

#[tokio::test]
async fn probe_health_returns_zero_on_ok() {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().route("/health", axum::routing::get(|| async { axum::Json(serde_json::json!({"ok": true})) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let code = tvdisplay::health::probe_health("127.0.0.1", &port.to_string()).await;
    assert_eq!(code, 0);
}

#[tokio::test]
async fn probe_health_returns_one_when_nothing_listens() {
    let code = tvdisplay::health::probe_health("127.0.0.1", "1").await;
    assert_eq!(code, 1);
}

