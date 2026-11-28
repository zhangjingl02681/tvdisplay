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
