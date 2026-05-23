use anyhow::Result;
use server::{ServerConfig, init_logging, run};

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let config_path = parse_config_flag();
    run(ServerConfig::load_with_env(config_path.as_deref())?).await
}

fn parse_config_flag() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--config" {
            return iter.next().cloned();
        }
    }
    None
}
