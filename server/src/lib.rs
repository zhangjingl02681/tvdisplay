pub mod auth;
pub mod config;
pub mod content;
pub mod feishu;
pub mod health;
pub mod http;
pub mod hub;
pub mod logging;

pub async fn run_server(
    settings: crate::config::Settings,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut secrets = vec![settings.admin_password.clone()];
    if let crate::config::FeishuMode::Enabled(feishu) = &settings.feishu {
        secrets.push(feishu.app_secret.clone());
    }
    crate::logging::install_redaction(&secrets);

    let mut state = crate::http::build_state(settings.clone());
    match &settings.feishu {
        crate::config::FeishuMode::Disabled => {
            tracing::info!("飞书未配置");
        }
        crate::config::FeishuMode::Enabled(feishu) => {
            let api = std::sync::Arc::new(crate::feishu::client::FeishuApi::new(
                feishu.app_id.clone(),
                feishu.app_secret.clone(),
                "https://open.feishu.cn".into(),
                "https://accounts.feishu.cn".into(),
            ));
            state.feishu_api = Some(api);
            let bot_state = state.clone();
            let feishu = feishu.clone();
            tokio::spawn(async move {
                crate::feishu::run_feishu_bot(bot_state, feishu).await;
            });
        }
    }

    let bind = bind_addr(&settings.host, settings.port);
    tracing::info!("listening on {} port {}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, crate::http::router(state)).await?;
    Ok(())
}

pub fn bind_addr(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
