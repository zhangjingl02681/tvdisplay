pub fn health_urls(host: &str, port: &str) -> Vec<String> {
    if matches!(host, "::" | "0.0.0.0" | "") {
        return vec![
            format!("http://[::1]:{port}/health"),
            format!("http://127.0.0.1:{port}/health"),
        ];
    }
    if host.contains(':') {
        vec![format!("http://[{host}]:{port}/health")]
    } else {
        vec![format!("http://{host}:{port}/health")]
    }
}
