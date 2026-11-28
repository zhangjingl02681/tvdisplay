#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("health") {
        let host = std::env::var("HOST")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "::".into());
        let port = std::env::var("PORT")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "8080".into());
        let code = tvdisplay::health::probe_health(&host, &port).await;
        std::process::exit(code);
    }
    match tvdisplay::config::load_settings_from_env() {
        Ok(settings) => {
            if let Err(err) = tvdisplay::run_server(settings).await {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("{}", err.message);
            std::process::exit(1);
        }
    }
}
