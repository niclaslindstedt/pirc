mod config;

use config::ServerConfig;
use std::path::PathBuf;
use std::process;

fn parse_config_path() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            if i + 1 < args.len() {
                return Some(PathBuf::from(&args[i + 1]));
            } else {
                eprintln!("error: --config requires a path argument");
                process::exit(1);
            }
        }
        i += 1;
    }
    None
}

fn main() {
    let config_path = parse_config_path();

    let config = match ServerConfig::load(config_path.as_deref()) {
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

    let cluster_status = if config.cluster.enabled {
        format!("enabled (node: {})", config.cluster.node_id.as_deref().unwrap_or("unknown"))
    } else {
        "disabled".to_string()
    };

    println!(
        "pircd starting on {}:{} (cluster: {})",
        config.network.bind_address, config.network.port, cluster_status
    );
}
