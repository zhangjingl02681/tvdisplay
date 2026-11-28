use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn compose_binds_host_network_without_a_tunnel() {
    let compose = std::fs::read_to_string(repo_root().join("docker-compose.yml")).unwrap();
    let lowered = compose.to_lowercase();
    assert!(compose.contains("network_mode: host"));
    assert!(!lowered.contains("cloudflare"));
    assert!(!lowered.contains("ngrok"));
    assert!(!lowered.contains("tailscale"));
    let dockerfile = std::fs::read_to_string(repo_root().join("Dockerfile")).unwrap();
    assert!(dockerfile.contains("8080"));
    assert!(dockerfile.contains("tvdisplay health") || dockerfile.contains("\"tvdisplay\", \"health\""));
    assert!(!dockerfile.to_lowercase().contains("python:3.12"));
}

#[test]
fn env_example_has_feishu_placeholders_only() {
    let text = std::fs::read_to_string(repo_root().join(".env.example")).unwrap();
    assert!(text.contains("FEISHU_APP_ID="));
    assert!(text.contains("FEISHU_APP_SECRET="));
    assert!(text.contains("FEISHU_ALLOWED_CHAT_IDS="));
    assert!(text.contains("cli_xxxxxxxx") || text.contains("替换成应用密钥"));
    assert!(!text.contains("cli_a"));
}

#[test]
fn deploy_doc_is_chinese_and_covers_ipv6_http_https_and_feishu() {
    let text = std::fs::read_to_string(repo_root().join("docs/deploy-raspberry-pi.md")).unwrap();
    for needle in [
        "树莓派",
        "IPv6",
        "明文",
        "AAAA",
        "不使用 Cloudflare Tunnel",
        "内网穿透",
        "FEISHU_APP_ID",
        "长连接",
        "企业自建",
        "会话 ID",
        "oc_",
        "im.message.receive_v1",
        "card.action.trigger",
        "linux/arm64",
        "1 GB",
        "cargo test",
    ] {
        assert!(text.contains(needle), "missing: {needle}");
    }
    assert!(text.contains("[2001:db8::1]") || text.contains("方括号"));
    assert!(text.contains("Let’s Encrypt") || text.contains("Let's Encrypt"));
    let readme = std::fs::read_to_string(repo_root().join("README.md")).unwrap();
    assert!(readme.contains("遥显"));
    assert!(readme.contains("cargo test"));
    assert!(!readme.contains("pytest"));
}

#[test]
fn python_backend_is_removed() {
    let root = repo_root();
    assert!(!root.join("server/pyproject.toml").exists());
    assert!(!root.join("server/tvdisplay/app.py").exists());
    assert!(!root.join("server/tests/test_admin.py").exists());
    assert!(root.join("server/Cargo.toml").exists());
    assert!(root.join("server/src/main.rs").exists());
}
