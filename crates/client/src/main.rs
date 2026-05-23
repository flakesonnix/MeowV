use std::env;

use anyhow::{bail, Result};
use protocol::{decode_server_line, encode_line, ClientMessage};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args: Vec<String> = env::args().collect();
    let name = read_flag(&args, "--name").unwrap_or_else(|| "dummy-client".to_string());
    let message = read_flag(&args, "--message").unwrap_or_else(|| "hello from client".to_string());
    let addr = read_flag(&args, "--addr").unwrap_or_else(|| "127.0.0.1:7000".to_string());

    let stream = TcpStream::connect(&addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(encode_line(&ClientMessage::Login { name: name.clone() })?.as_bytes())
        .await?;
    writer_half
        .write_all(encode_line(&ClientMessage::Chat { message })?.as_bytes())
        .await?;

    while let Some(line) = lines.next_line().await? {
        let packet = decode_server_line(&line)?;
        info!(packet = ?packet, "received packet");
    }

    Ok(())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
}

fn read_flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|window| window[0] == name).map(|window| window[1].clone())
}

#[allow(dead_code)]
fn require_flag(args: &[String], name: &str) -> Result<String> {
    match read_flag(args, name) {
        Some(value) => Ok(value),
        None => bail!("missing required flag: {name}"),
    }
}
