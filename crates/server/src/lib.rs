use std::{collections::HashMap, env, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use protocol::{decode_client_line, encode_line, ClientMessage, EntityState, Position, ServerMessage};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{broadcast, RwLock},
    task::JoinHandle,
    time,
};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub tick_rate: u64,
    pub motd: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:7000".to_string(),
            tick_rate: 10,
            motd: "welcome to meowv milestone 0".to_string(),
        }
    }
}

impl ServerConfig {
    pub fn load() -> Result<Self> {
        let mut cfg = Self::default();

        if let Ok(path) = env::var("MEOWV_CONFIG") {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read config file: {path}"))?;
            cfg = toml::from_str(&raw).context("failed to parse config TOML")?;
        }

        if let Ok(bind) = env::var("MEOWV_SERVER_BIND") {
            cfg.bind = bind;
        }

        if let Ok(tick_rate) = env::var("MEOWV_TICK_RATE") {
            cfg.tick_rate = tick_rate.parse().context("invalid MEOWV_TICK_RATE")?;
        }

        Ok(cfg)
    }
}

#[derive(Debug, Clone)]
struct ClientInfo {
    name: String,
    entity_id: u32,
}

#[derive(Default)]
struct SharedState {
    clients: RwLock<HashMap<Uuid, ClientInfo>>,
}

pub async fn run(config: ServerConfig) -> Result<()> {
    let listener = TcpListener::bind(&config.bind).await?;
    info!(bind = %config.bind, tick_rate = config.tick_rate, "server listening");
    run_with_listener(listener, config).await
}

pub async fn run_with_listener(listener: TcpListener, config: ServerConfig) -> Result<()> {
    let state = Arc::new(SharedState::default());
    let (tx, _) = broadcast::channel(256);

    spawn_tick_loop(config.clone(), state.clone(), tx.clone());

    loop {
        let (stream, addr) = listener.accept().await?;
        info!(%addr, "client connected");

        let state = state.clone();
        let tx = tx.clone();
        let config = config.clone();

        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, state, tx, config).await {
                warn!(error = %err, "client session ended with error");
            }
        });
    }
}

pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();
}

fn spawn_tick_loop(
    config: ServerConfig,
    state: Arc<SharedState>,
    tx: broadcast::Sender<ServerMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let tick_ms = (1000 / config.tick_rate.max(1)).max(1);
        let mut interval = time::interval(Duration::from_millis(tick_ms));
        let mut tick: u64 = 0;

        loop {
            interval.tick().await;
            tick = tick.wrapping_add(1);

            let clients = state.clients.read().await;
            if clients.is_empty() {
                continue;
            }

            let entities = clients
                .iter()
                .map(|(client_id, client)| EntityState {
                    entity_id: client.entity_id,
                    owner_id: *client_id,
                    position: Position {
                        x: tick as f32,
                        y: client.entity_id as f32,
                        z: 0.0,
                    },
                    tick,
                })
                .collect();

            let _ = tx.send(ServerMessage::EntitySnapshot { entities });
        }
    })
}

async fn handle_client(
    stream: TcpStream,
    state: Arc<SharedState>,
    tx: broadcast::Sender<ServerMessage>,
    config: ServerConfig,
) -> Result<()> {
    let client_id = Uuid::new_v4();
    let entity_id = next_entity_id(&client_id);
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();
    let mut rx = tx.subscribe();
    let name = match lines.next_line().await? {
        Some(line) => match decode_client_line(&line)? {
            ClientMessage::Login { name } => name,
            _ => {
                send_direct(
                    &mut writer_half,
                    &ServerMessage::Error {
                        message: "first packet must be login".to_string(),
                    },
                )
                .await?;
                return Ok(());
            }
        },
        None => return Ok(()),
    };

    state.clients.write().await.insert(
        client_id,
        ClientInfo {
            name: name.clone(),
            entity_id,
        },
    );

    send_direct(
        &mut writer_half,
        &ServerMessage::Welcome {
            client_id,
            motd: config.motd,
        },
    )
    .await?;

    let _ = tx.send(ServerMessage::ChatBroadcast {
        from: "server".to_string(),
        message: format!("{name} joined"),
    });

    let writer_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(message) => {
                    if let Err(err) = send_direct(&mut writer_half, &message).await {
                        return Err(err);
                    }
                }
                Err(err) => {
                    error!(error = %err, "broadcast receive failed");
                    return Ok(());
                }
            }
        }
    });

    while let Some(line) = lines.next_line().await? {
        match decode_client_line(&line)? {
            ClientMessage::Login { .. } => {
                warn!(%client_id, "ignoring duplicate login packet");
            }
            ClientMessage::Chat { message } => {
                let from = {
                    let clients = state.clients.read().await;
                    clients
                        .get(&client_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| "unknown".to_string())
                };

                let _ = tx.send(ServerMessage::ChatBroadcast { from, message });
            }
        }
    }

    state.clients.write().await.remove(&client_id);
    let _ = tx.send(ServerMessage::ChatBroadcast {
        from: "server".to_string(),
        message: format!("{name} left"),
    });

    writer_task.abort();
    Ok(())
}

async fn send_direct<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &ServerMessage) -> Result<()> {
    writer.write_all(encode_line(msg)?.as_bytes()).await?;
    Ok(())
}

fn next_entity_id(client_id: &Uuid) -> u32 {
    let bytes = client_id.as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
