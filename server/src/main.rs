fn main() {
    let mut args = std::env::args();
    let _exe = args.next();
    match args.next().as_deref() {
        Some("health") => {
            eprintln!("health subcommand is not implemented yet");
            std::process::exit(2);
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
        None => {
            eprintln!("server entry is not implemented yet");
            std::process::exit(2);
        }
    }
}
