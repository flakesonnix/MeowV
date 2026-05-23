mod diagnostics;
mod event_log;
mod session;

use event_log::{SessionEventKind, SessionEventLog};
use session::{SessionState, SessionStateError, SessionStateMachine};
use std::{collections::HashMap, env, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use protocol::{
    AnnouncedResource, AnnouncedResourceFile, ClientMessage, DisconnectReason, EntityState,
    JoinGateDecision, JoinGateOutcome, PROTOCOL_VERSION, Position, ProtocolCapability,
    ProtocolCompatibilityProfile, ProtocolNegotiationStatus, ProtocolVersionRange,
    ResourceAnnouncement, ResourceJoinDecision, ResourcePolicyEvaluation, ResourceRequirementLevel,
    ServerMessage, build_join_gate_decision, capability_gate_report, current_protocol_profile,
    decode_client_line, encode_line, evaluate_resource_policy, negotiate_protocol_dry_run,
    shared_capabilities,
};
use resource_manifest::build_pack_index;
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{RwLock, broadcast, mpsc},
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
    last_announcement: Option<ResourceAnnouncement>,
    shared_caps: Vec<ProtocolCapability>,
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
    let (client_tx, mut client_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let mut session = SessionStateMachine::new();
    let mut event_log = SessionEventLog::new();
    event_log.record(
        SessionEventKind::Connected,
        SessionState::Connected,
        "client connected",
    );
    info!(%client_id, state = ?session.state(), "session: connected");
    let (name, shared_caps) = match lines.next_line().await? {
        Some(line) => match decode_client_line(&line)? {
            ClientMessage::Login {
                name,
                protocol_version,
            } => {
                if let Err(e) = session.on_hello_received() {
                    warn!(%client_id, error = %e, "session: unexpected hello transition error");
                } else {
                    event_log.record(
                        SessionEventKind::HelloReceived,
                        SessionState::HelloReceived,
                        format!("login from {name}"),
                    );
                    info!(%client_id, state = ?session.state(), "session: hello received");
                }

                if let Err(_) = session.on_version_checked(protocol_version) {
                    event_log.record(
                        SessionEventKind::Failed,
                        SessionState::Failed,
                        format!(
                            "protocol mismatch: client={protocol_version} server={PROTOCOL_VERSION}"
                        ),
                    );
                    send_direct(
                        &mut writer_half,
                        &ServerMessage::Disconnect {
                            reason: DisconnectReason::ProtocolMismatch,
                            message: format!(
                                "protocol mismatch: client={protocol_version} server={PROTOCOL_VERSION}"
                            ),
                        },
                    )
                    .await?;
                    info!(%client_id, state = ?session.state(), "session: failed on version mismatch");
                    return Ok(());
                }
                event_log.record(
                    SessionEventKind::VersionChecked,
                    SessionState::VersionChecked,
                    format!("protocol version {protocol_version} matched"),
                );
                info!(%client_id, state = ?session.state(), "session: version checked");

                let server_profile = current_protocol_profile();
                let client_profile = ProtocolCompatibilityProfile {
                    version_range: ProtocolVersionRange {
                        min: protocol_version,
                        max: protocol_version,
                    },
                    capabilities: Vec::new(),
                };
                let negotiation = negotiate_protocol_dry_run(&client_profile, &server_profile);
                let caps = shared_capabilities(&client_profile, &server_profile);
                info!(
                    client_version = protocol_version,
                    server_version = PROTOCOL_VERSION,
                    negotiation_status = ?negotiation.status,
                    shared_capability_count = caps.len(),
                    "protocol handshake: exact-match policy active, negotiation dry-run computed"
                );
                if negotiation.status != ProtocolNegotiationStatus::ExactMatch {
                    info!(
                        client_version = protocol_version,
                        server_version = PROTOCOL_VERSION,
                        reason = %negotiation.reason,
                        "protocol negotiation dry-run: non-exact overlap detected"
                    );
                }
                if let Err(e) = session.on_negotiation_logged() {
                    warn!(%client_id, error = %e, "session: unexpected negotiation transition error");
                } else {
                    event_log.record(
                        SessionEventKind::ProtocolNegotiationDryRun,
                        SessionState::NegotiationDryRunLogged,
                        format!("negotiation status: {:?}", negotiation.status),
                    );
                    info!(%client_id, state = ?session.state(), "session: negotiation dry-run logged");
                }

                (name, caps)
            }
            _ => {
                send_direct(
                    &mut writer_half,
                    &ServerMessage::Disconnect {
                        reason: DisconnectReason::InvalidHandshake,
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
            last_announcement: build_example_resource_announcement(),
            shared_caps: shared_caps.clone(),
        },
    );

    send_direct(
        &mut writer_half,
        &ServerMessage::Welcome {
            client_id,
            motd: config.motd,
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await?;

    if let Some(announcement) = state
        .clients
        .read()
        .await
        .get(&client_id)
        .and_then(|client| client.last_announcement.clone())
    {
        let gate = capability_gate_report(ProtocolCapability::ResourceAnnouncement, &shared_caps);
        event_log.record(
            SessionEventKind::CapabilityGateChecked,
            session.state().clone(),
            format!("ResourceAnnouncement gate: supported={}", gate.supported),
        );
        info!(
            %client_id,
            capability = "ResourceAnnouncement",
            supported = gate.supported,
            reason = %gate.reason,
            "capability gate check: resource announcement (dry-run, report-only)"
        );
        send_direct(
            &mut writer_half,
            &ServerMessage::ResourceAnnouncement(announcement),
        )
        .await?;
        if let Err(e) = session.on_resource_announcement_sent() {
            warn!(%client_id, error = %e, "session: unexpected announcement transition error");
        } else {
            event_log.record(
                SessionEventKind::ResourceAnnouncementSent,
                SessionState::ResourceAnnouncementSent,
                "resource announcement sent to client",
            );
            info!(%client_id, state = ?session.state(), "session: resource announcement sent");
        }
    }

    let _ = tx.send(ServerMessage::ChatBroadcast {
        from: "server".to_string(),
        message: format!("{name} joined"),
    });

    let writer_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;

                maybe_message = client_rx.recv() => {
                    match maybe_message {
                        Some(message) => {
                            if let Err(err) = send_direct(&mut writer_half, &message).await {
                                return Err(err);
                            }
                        }
                        None => return Ok(()),
                    }
                }
                result = rx.recv() => {
                    match result {
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
            ClientMessage::ResourceAvailabilityReport(report) => {
                let client_data = {
                    let clients = state.clients.read().await;
                    clients
                        .get(&client_id)
                        .map(|c| (c.last_announcement.clone(), c.shared_caps.clone()))
                };

                if let Some((Some(announcement), caps)) = client_data {
                    if let Err(e) = session.on_availability_report_received() {
                        warn!(%client_id, error = %e, "session: unexpected availability transition error");
                    } else {
                        event_log.record(
                            SessionEventKind::AvailabilityReportReceived,
                            SessionState::AvailabilityReportReceived,
                            "resource availability report received from client",
                        );
                        info!(%client_id, state = ?session.state(), "session: availability report received");
                    }

                    let evaluation = evaluate_resource_policy(&announcement, &report);
                    log_resource_policy_evaluation(client_id, &evaluation);
                    let policy_decision = evaluation.decision.clone();
                    let gate_decision = build_join_gate_decision(evaluation);

                    match session.on_policy_evaluated(&policy_decision) {
                        Ok(()) => {
                            event_log.record(
                                SessionEventKind::ResourcePolicyEvaluated,
                                SessionState::ResourcePolicyEvaluated,
                                format!("policy decision: {:?}", policy_decision),
                            );
                            info!(%client_id, state = ?session.state(), "session: resource policy evaluated");
                        }
                        Err(SessionStateError::PolicyBlockedDryRun) => {
                            event_log.record(
                                SessionEventKind::ResourcePolicyEvaluated,
                                SessionState::ResourcePolicyEvaluated,
                                format!(
                                    "policy decision: {:?} (dry-run, not enforced)",
                                    policy_decision
                                ),
                            );
                            info!(
                                %client_id,
                                state = ?session.state(),
                                "session: resource policy would block (dry-run only, not enforced)"
                            );
                        }
                        Err(e) => {
                            warn!(%client_id, error = %e, "session: unexpected policy transition error");
                        }
                    }

                    let gate = capability_gate_report(ProtocolCapability::JoinGateDryRun, &caps);
                    event_log.record(
                        SessionEventKind::CapabilityGateChecked,
                        session.state().clone(),
                        format!("JoinGateDryRun gate: supported={}", gate.supported),
                    );
                    info!(
                        %client_id,
                        capability = "JoinGateDryRun",
                        supported = gate.supported,
                        reason = %gate.reason,
                        "capability gate check: join gate decision (dry-run, report-only)"
                    );
                    log_join_gate_decision(client_id, &gate_decision);
                    let _ = client_tx.send(ServerMessage::JoinGateDecision(gate_decision));

                    if let Err(e) = session.on_join_gate_sent() {
                        warn!(%client_id, error = %e, "session: unexpected join gate transition error");
                    } else {
                        event_log.record(
                            SessionEventKind::JoinGateDryRunSent,
                            SessionState::JoinGateDryRunSent,
                            "join gate dry-run decision sent to client",
                        );
                        info!(%client_id, state = ?session.state(), "session: join gate dry-run sent");
                    }

                    if let Err(e) = session.mark_ready_dry_run() {
                        warn!(%client_id, error = %e, "session: unexpected ready transition error");
                    } else {
                        event_log.record(
                            SessionEventKind::ReadyDryRun,
                            SessionState::ReadyDryRun,
                            "handshake pipeline complete (dry-run)",
                        );
                        info!(%client_id, state = ?session.state(), "session: ready (dry-run)");
                    }
                } else {
                    warn!(%client_id, "resource availability report received before announcement was stored");
                }
            }
        }
    }

    state.clients.write().await.remove(&client_id);
    let _ = tx.send(ServerMessage::ChatBroadcast {
        from: "server".to_string(),
        message: format!("{name} left"),
    });

    writer_task.abort();
    info!(
        %client_id,
        event_count = event_log.len(),
        final_state = ?session.state(),
        "session audit log complete"
    );
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

fn build_example_resource_announcement() -> Option<ResourceAnnouncement> {
    let resource_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resources/chat");
    let index = build_pack_index(resource_dir).ok()?;
    let files = index
        .files
        .into_iter()
        .map(|file| AnnouncedResourceFile {
            relative_path: file.relative_path.to_string_lossy().into_owned(),
            size_bytes: file.size_bytes,
            sha256: file.sha256,
        })
        .collect();

    Some(ResourceAnnouncement {
        resources: vec![AnnouncedResource {
            name: index.manifest.name,
            version: index.manifest.version,
            files,
            protocol_version: index.manifest.protocol_version,
            requirement_level: ResourceRequirementLevel::Required,
        }],
        signature: None,
    })
}

fn log_resource_policy_evaluation(client_id: Uuid, evaluation: &ResourcePolicyEvaluation) {
    let decision = match evaluation.decision {
        ResourceJoinDecision::Allowed => "allowed",
        ResourceJoinDecision::Blocked => "blocked",
        ResourceJoinDecision::WarningOnly => "warning_only",
    };

    info!(
        %client_id,
        decision,
        missing_required = evaluation.missing_required.len(),
        invalid_required = evaluation.invalid_required.len(),
        missing_optional = evaluation.missing_optional.len(),
        invalid_optional = evaluation.invalid_optional.len(),
        missing_recommended = evaluation.missing_recommended.len(),
        invalid_recommended = evaluation.invalid_recommended.len(),
        "resource policy evaluated"
    );
}

fn log_join_gate_decision(client_id: Uuid, decision: &JoinGateDecision) {
    let outcome = match decision.outcome {
        JoinGateOutcome::WouldAllow => "would_allow",
        JoinGateOutcome::WouldWarn => "would_warn",
        JoinGateOutcome::WouldBlock => "would_block",
    };

    info!(%client_id, outcome, reason = %decision.reason, "join gate dry-run evaluated");
}
