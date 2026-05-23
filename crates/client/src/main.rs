use std::env;

use anyhow::{Context, Result};
use protocol::{decode_server_line, encode_line, ClientMessage, PROTOCOL_VERSION};
use serde::Deserialize;
use server_browser::{filter_current_protocol, LocalJsonServerListSource, ServerListSource};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tracing::info;

#[derive(Debug, Clone, Deserialize)]
struct ClientConfig {
    addr: String,
    name: String,
    message: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:7000".to_string(),
            name: "dummy-client".to_string(),
            message: "hello from client".to_string(),
        }
    }
}

impl ClientConfig {
    fn load(args: &[String]) -> Result<Self> {
        let mut cfg = Self::default();

        if let Some(path) =
            read_flag(args, "--config").or_else(|| env::var("MEOWV_CLIENT_CONFIG").ok())
        {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read client config file: {path}"))?;
            cfg = toml::from_str(&raw).context("failed to parse client config TOML")?;
        }

        if let Ok(addr) = env::var("MEOWV_CLIENT_ADDR") {
            cfg.addr = addr;
        }

        if let Ok(name) = env::var("MEOWV_CLIENT_NAME") {
            cfg.name = name;
        }

        if let Ok(message) = env::var("MEOWV_CLIENT_MESSAGE") {
            cfg.message = message;
        }

        if let Some(addr) = read_flag(args, "--addr") {
            cfg.addr = addr;
        }

        if let Some(name) = read_flag(args, "--name") {
            cfg.name = name;
        }

        if let Some(message) = read_flag(args, "--message") {
            cfg.message = message;
        }

        Ok(cfg)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args: Vec<String> = env::args().collect();

    if let Some(path) = read_flag(&args, "--server-list") {
        print_server_list(&path)?;
        return Ok(());
    }

    let config = ClientConfig::load(&args)?;

    let stream = TcpStream::connect(&config.addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: config.name.clone(),
                protocol_version: PROTOCOL_VERSION,
            })?
            .as_bytes(),
        )
        .await?;
    writer_half
        .write_all(
            encode_line(&ClientMessage::Chat {
                message: config.message,
            })?
            .as_bytes(),
        )
        .await?;

    while let Some(line) = lines.next_line().await? {
        let packet = decode_server_line(&line)?;
        info!(packet = ?packet, "received packet");
    }

    Ok(())
}

fn print_server_list(path: &str) -> Result<()> {
    let source = LocalJsonServerListSource::new(path);
    let entries = source.load()?;
    let entries = filter_current_protocol(&entries);

    println!(
        "{:<24} {:<21} {:<9} {:<8} {:<10} {}",
        "NAME", "ADDRESS", "PLAYERS", "PROTO", "EDITION", "TAGS"
    );

    for entry in entries {
        println!(
            "{:<24} {:<21} {:<9} {:<8} {:<10} {}",
            entry.name,
            format!("{}:{}", entry.address, entry.port),
            format!("{}/{}", entry.current_players, entry.max_players),
            entry.protocol_version,
            format_edition(&entry.edition_compatibility),
            entry.tags.join(",")
        );
    }

    Ok(())
}

fn format_edition(edition: &server_browser::EditionCompatibility) -> &'static str {
    match edition {
        server_browser::EditionCompatibility::Legacy => "legacy",
        server_browser::EditionCompatibility::Enhanced => "enhanced",
        server_browser::EditionCompatibility::Any => "any",
        server_browser::EditionCompatibility::Unknown => "unknown",
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
}

fn read_flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}
