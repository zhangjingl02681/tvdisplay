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

use std::time::Duration;

pub async fn probe_health(host: &str, port: &str) -> i32 {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(client) => client,
        Err(_) => return 1,
    };
    for url in health_urls(host, port) {
        if let Ok(response) = client.get(&url).send().await {
            if response.status() == reqwest::StatusCode::OK {
                return 0;
            }
        }
    }
    1
}
