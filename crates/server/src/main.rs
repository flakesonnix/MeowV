use anyhow::Result;
use server::{init_logging, run, ServerConfig};

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    run(ServerConfig::load()?).await
}
