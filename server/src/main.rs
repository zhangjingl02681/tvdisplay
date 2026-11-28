#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("health") {
        eprintln!("health subcommand is not implemented yet");
        std::process::exit(2);
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
