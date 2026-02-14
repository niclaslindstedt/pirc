mod app;
pub mod client_command;
pub mod command_parser;
mod config;
mod connection_state;
mod registration;
mod tui;

use app::App;
use config::ClientConfig;
use std::path::PathBuf;
use std::process;

fn parse_config_path() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            if i + 1 < args.len() {
                return Some(PathBuf::from(&args[i + 1]));
            }
            eprintln!("error: --config requires a path argument");
            process::exit(1);
        }
        i += 1;
    }
    None
}

#[tokio::main]
async fn main() {
    let config_path = parse_config_path();

    let config = match ClientConfig::load(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = config.validate() {
        eprintln!("error: {e}");
        process::exit(1);
    }

    let app = App::new(config);

    if let Err(e) = app.run().await {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
