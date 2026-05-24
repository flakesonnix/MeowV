mod admin;
mod config;
mod diagnostics;
mod enforcement;
mod event_log;
mod heartbeat_planner;
mod session;
mod session_registry;
mod shutdown;
mod status;

pub use config::{
    AdminSection, ConfigError, DiagnosticsFormat, DiagnosticsSection, EnforcementSection,
    HeartbeatSection, JoinGateConfigMode, JoinGateSection, LogFormat, LogLevel, LoggingSection,
    CapabilityPolicy, ProtocolSection, ResourcesSection, ServerConfig, ServerSection,
    SignatureSection,
};
pub use enforcement::{SessionEnforcementDecision, SessionEnforcementPolicy, evaluate_enforcement};
pub use heartbeat_planner::{
    HeartbeatDecision, HeartbeatPlannerInput, HeartbeatPolicy, MISSED_HEARTBEAT_DISCONNECT_THRESHOLD,
    MISSED_SERVER_PONG_DISCONNECT_THRESHOLD, ServerHeartbeatDecision, ServerHeartbeatPlannerInput,
    evaluate_heartbeat, evaluate_server_heartbeat,
};
pub use session_registry::{
    SessionId, SessionRegistry, SessionRegistryEntry, SessionRegistrySnapshot,
};
pub use shutdown::{ShutdownReason, ShutdownSummary, build_shutdown_summary};
pub use status::ServerRuntimeStatus;

use config::DiagnosticsFormat as Fmt;
use diagnostics::SessionDiagnostics;
use event_log::{SessionEventKind, SessionEventLog};
use session::{SessionState, SessionStateError, SessionStateMachine};
use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use protocol::{
    AnnouncedResource, AnnouncedResourceFile, ClientMessage, DisconnectReason, EntityState,
    JoinGateDecision, JoinGateOutcome, PROTOCOL_VERSION, Position, ProtocolCapability,
    ProtocolCompatibilityProfile, ProtocolNegotiationStatus, ProtocolVersionRange,
    ResourceAnnouncement, ResourceJoinDecision, ResourcePolicyEvaluation, ResourceRequirementLevel,
    ServerMessage, all_login_capabilities, build_join_gate_decision, capability_gate_report,
    current_capability_negotiation_policy, current_protocol_profile, decode_client_line,
    encode_line, evaluate_capability_negotiation, evaluate_resource_policy,
    negotiate_protocol_dry_run, shared_capabilities,
};
use resource_manifest::build_pack_index;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{RwLock, broadcast, mpsc, oneshot},
    task::JoinHandle,
    time,
};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub name: String,
    pub entity_id: u32,
    pub last_announcement: Option<ResourceAnnouncement>,
    pub shared_caps: Vec<ProtocolCapability>,
}

pub struct SharedState {
    pub clients: RwLock<HashMap<Uuid, ClientInfo>>,
    pub registry: Arc<std::sync::Mutex<session_registry::SessionRegistry>>,
    pub shutdown: std::sync::Mutex<shutdown::ShutdownState>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            clients: RwLock::default(),
            registry: Arc::new(std::sync::Mutex::new(
                session_registry::SessionRegistry::new(),
            )),
            shutdown: std::sync::Mutex::new(shutdown::ShutdownState::new()),
        }
    }
}

/// RAII guard: removes the session from the registry when dropped.
/// Handles cleanup on all exit paths including early returns via `?`.
struct SessionGuard {
    id: session_registry::SessionId,
    registry: Arc<std::sync::Mutex<session_registry::SessionRegistry>>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.remove_session(&self.id);
        }
    }
}

pub async fn run(config: ServerConfig) -> Result<()> {
    let listener = TcpListener::bind(&config.server.bind_addr).await?;
    info!(
        bind = %config.server.bind_addr,
        tick_rate = config.server.tick_rate,
        name = %config.server.name,
        "server listening"
    );
    info!(
        "server lifecycle config:\n{}",
        config.to_lifecycle_summary_text(),
    );
    run_with_listener(listener, config).await
}

pub async fn run_with_listener(listener: TcpListener, config: ServerConfig) -> Result<()> {
    run_with_listener_and_state(listener, config, Arc::new(SharedState::default())).await
}

pub async fn run_with_listener_and_state(
    listener: TcpListener,
    config: ServerConfig,
    state: Arc<SharedState>,
) -> Result<()> {
    let (tx, _) = broadcast::channel(256);

    state
        .registry
        .lock()
        .unwrap()
        .set_heartbeat_policy(config.heartbeat.policy.clone());

    spawn_tick_loop(config.clone(), state.clone(), tx.clone());

    if config.admin.local_stdin_enabled {
        let (quit_tx, quit_rx) = oneshot::channel::<()>();
        tokio::spawn(admin_stdin_loop(quit_tx, config.clone(), state.clone()));
        let result = tokio::select! {
            result = accept_loop(&listener, state.clone(), tx, &config) => result,
            _ = quit_rx => {
                info!("server shutdown requested via admin command");
                Ok(())
            }
        };
        let reg_snap = state.registry.lock().unwrap().snapshot();
        let reason = state
            .shutdown
            .lock()
            .unwrap()
            .reason()
            .unwrap_or(shutdown::ShutdownReason::AdminQuit);
        let summary = shutdown::build_shutdown_summary(&config, &reg_snap, reason);
        info!(
            reason = %summary.reason,
            "server shutdown: final summary\n--- status ---\n{}\n--- sessions ---\n{}",
            summary.status_dump,
            summary.registry_dump,
        );
        result
    } else {
        accept_loop(&listener, state, tx, &config).await
    }
}

async fn accept_loop(
    listener: &TcpListener,
    state: Arc<SharedState>,
    tx: broadcast::Sender<ServerMessage>,
    config: &ServerConfig,
) -> Result<()> {
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

async fn admin_stdin_loop(
    quit_tx: oneshot::Sender<()>,
    config: ServerConfig,
    state: Arc<SharedState>,
) {
    use admin::{AdminCommandParseError, handle_admin_command_with_context, parse_admin_command};

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => match parse_admin_command(&line) {
                Ok(cmd) => {
                    let reg_snap = state.registry.lock().unwrap().snapshot();
                    let current_status = status::ServerRuntimeStatus::from_config(&config)
                        .with_session_counts(
                            reg_snap.connected_sessions,
                            reg_snap.ready_dry_run_sessions,
                            reg_snap.failed_sessions,
                        );
                    let result = handle_admin_command_with_context(
                        cmd,
                        Some(&current_status),
                        Some(&reg_snap),
                    );
                    info!(message = %result.message, "admin");
                    if result.should_quit {
                        state
                            .shutdown
                            .lock()
                            .unwrap()
                            .request(shutdown::ShutdownReason::AdminQuit);
                        let _ = quit_tx.send(());
                        return;
                    }
                }
                Err(AdminCommandParseError::Empty) => {}
                Err(e) => {
                    info!(error = %e, "admin command error");
                }
            },
        }
    }
}

pub fn init_logging(logging: &LoggingSection) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(logging.level.as_str()));

    match logging.format {
        LogFormat::Text => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(logging.show_targets)
                .try_init();
        }
        LogFormat::Json => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(logging.show_targets)
                .json()
                .try_init();
        }
    }
    info!(
        level = logging.level.as_str(),
        format = ?logging.format,
        show_targets = logging.show_targets,
        "logging initialized"
    );
}

fn spawn_tick_loop(
    config: ServerConfig,
    state: Arc<SharedState>,
    tx: broadcast::Sender<ServerMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let tick_ms = (1000 / config.server.tick_rate.max(1)).max(1);
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

    let session_id = state.registry.lock().unwrap().create_session();
    let _session_guard = SessionGuard {
        id: session_id,
        registry: state.registry.clone(),
    };

    event_log.record(
        SessionEventKind::Connected,
        SessionState::Connected,
        "client connected",
    );
    info!(%client_id, state = ?session.state(), "session: connected");
    let (name, shared_caps, capability_negotiation) = match lines.next_line().await? {
        Some(line) => match decode_client_line(&line) {
            Err(err) => {
                send_direct(
                    &mut writer_half,
                    &ServerMessage::Disconnect {
                        reason: DisconnectReason::InvalidHandshake,
                        message: format!("invalid login payload: {err}"),
                    },
                )
                .await?;
                return Ok(());
            }
            Ok(message) => match message {
            ClientMessage::Login {
                name,
                protocol_version,
                capabilities,
            } => {
                if let Err(e) = session.on_hello_received() {
                    session.fail(e.to_string());
                    event_log.record(
                        SessionEventKind::Failed,
                        SessionState::Failed,
                        format!("{e}"),
                    );
                    state.registry.lock().unwrap().update_session(
                        &session_id,
                        session.state().clone(),
                        event_log.len(),
                    );
                    if config.diagnostics.print_session_diagnostics {
                        let diag = SessionDiagnostics::from_parts(&session, &event_log)
                            .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                        let text = match config.diagnostics.format {
                            Fmt::Text => diag.to_text(),
                            Fmt::JsonStub => diag.to_json_stub(),
                        };
                        info!(%client_id, "session diagnostics (failed):\n{text}");
                    }
                    send_direct(
                        &mut writer_half,
                        &ServerMessage::Disconnect {
                            reason: DisconnectReason::InvalidHandshake,
                            message: format!("session state error: {e}"),
                        },
                    )
                    .await?;
                    info!(%client_id, state = ?session.state(), "session: failed on hello transition");
                    return Ok(());
                } else {
                    event_log.record(
                        SessionEventKind::HelloReceived,
                        SessionState::HelloReceived,
                        format!(
                            "login from {name} (required_caps={} optional_caps={} feature_flags={})",
                            capabilities.required.len(),
                            capabilities.optional.len(),
                            capabilities
                                .feature_flags
                                .as_ref()
                                .map(|flags| flags.len())
                                .unwrap_or(0)
                        ),
                    );
                    state.registry.lock().unwrap().update_session(
                        &session_id,
                        session.state().clone(),
                        event_log.len(),
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
                    state.registry.lock().unwrap().update_session(
                        &session_id,
                        session.state().clone(),
                        event_log.len(),
                    );
                    if config.diagnostics.print_session_diagnostics {
                        let diag = SessionDiagnostics::from_parts(&session, &event_log)
                            .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                        let text = match config.diagnostics.format {
                            Fmt::Text => diag.to_text(),
                            Fmt::JsonStub => diag.to_json_stub(),
                        };
                        info!(%client_id, "session diagnostics (failed):\n{text}");
                    }
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
                {
                    let mut reg = state.registry.lock().unwrap();
                    reg.set_protocol_version(&session_id, protocol_version);
                    reg.set_login_capabilities(&session_id, capabilities.clone());
                    reg.update_session(&session_id, session.state().clone(), event_log.len());
                }
                info!(%client_id, state = ?session.state(), "session: version checked");

                let server_profile = current_protocol_profile();
                let client_profile = ProtocolCompatibilityProfile {
                    version_range: ProtocolVersionRange {
                        min: protocol_version,
                        max: protocol_version,
                    },
                    capabilities: all_login_capabilities(&capabilities),
                };
                let negotiation = negotiate_protocol_dry_run(&client_profile, &server_profile);
                let caps = shared_capabilities(&client_profile, &server_profile);
                let capability_policy = current_capability_negotiation_policy();
                let capability_negotiation =
                    evaluate_capability_negotiation(&capabilities, &capability_policy);
                info!(
                    client_version = protocol_version,
                    server_version = PROTOCOL_VERSION,
                    required_capability_count = capabilities.required.len(),
                    optional_capability_count = capabilities.optional.len(),
                    feature_flag_count = capabilities.feature_flags.as_ref().map(|flags| flags.len()).unwrap_or(0),
                    negotiation_status = ?negotiation.status,
                    capability_negotiation = capability_negotiation.decision.to_text(),
                    capability_warning_count = capability_negotiation.warnings.len(),
                    capability_violation_count = capability_negotiation.violations.len(),
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
                if config.protocol.capability_policy == CapabilityPolicy::Strict
                    && capability_negotiation.decision
                        == protocol::CapabilityNegotiationDecision::WouldReject
                {
                    let reason = capability_negotiation
                        .violations
                        .first()
                        .map(|violation| violation.to_text())
                        .unwrap_or_else(|| {
                            "capability negotiation rejected under strict policy".to_string()
                        });
                    session.fail(reason.clone());
                    event_log.record(
                        SessionEventKind::Failed,
                        SessionState::Failed,
                        format!("capability enforcement: {reason}"),
                    );
                    {
                        let mut reg = state.registry.lock().unwrap();
                        reg.set_capability_negotiation(&session_id, capability_negotiation.clone());
                        reg.update_session(&session_id, SessionState::Failed, event_log.len());
                    }
                    if config.diagnostics.print_session_diagnostics {
                        let diag = SessionDiagnostics::from_parts(&session, &event_log)
                            .with_capability_negotiation(&capability_negotiation)
                            .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                        let text = match config.diagnostics.format {
                            Fmt::Text => diag.to_text(),
                            Fmt::JsonStub => diag.to_json_stub(),
                        };
                        info!(%client_id, "session diagnostics (capability strict reject):\n{text}");
                    }
                    send_direct(
                        &mut writer_half,
                        &ServerMessage::Disconnect {
                            reason: DisconnectReason::InvalidHandshake,
                            message: format!("capability policy strict reject: {reason}"),
                        },
                    )
                    .await?;
                    info!(%client_id, reason = %reason, "session: strict capability policy rejected login");
                    return Ok(());
                }
                if let Err(e) = session.on_negotiation_logged() {
                    warn!(%client_id, error = %e, "session: unexpected negotiation transition error");
                    if config.enforcement.mode == SessionEnforcementPolicy::Strict {
                        session.fail(format!("negotiation transition failed: {e}"));
                        if handle_enforcement(
                            &session,
                            &mut event_log,
                            &session_id,
                            &state.registry,
                            &config,
                            client_id,
                            &mut writer_half,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                    }
                } else {
                    event_log.record(
                        SessionEventKind::ProtocolNegotiationDryRun,
                        SessionState::NegotiationDryRunLogged,
                        format!(
                            "negotiation log processed for {name}; capability_negotiation={} warnings={} violations={}",
                            capability_negotiation.decision.to_text(),
                            capability_negotiation.warnings.len(),
                            capability_negotiation.violations.len()
                        ),
                    );
                    state.registry.lock().unwrap().set_capability_negotiation(
                        &session_id,
                        capability_negotiation.clone(),
                    );
                    state.registry.lock().unwrap().update_session(
                        &session_id,
                        session.state().clone(),
                        event_log.len(),
                    );
                    info!(%client_id, state = ?session.state(), "session: negotiation logged");
                }

                (name, caps, capability_negotiation)
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
        }},
        None => return Ok(()),
    };

    state.clients.write().await.insert(
        client_id,
        ClientInfo {
            name: name.clone(),
            entity_id,
            last_announcement: build_example_resource_announcement(
                &config.resources.announcement_resource_dir,
            ),
            shared_caps: shared_caps.clone(),
        },
    );

    send_direct(
        &mut writer_half,
        &ServerMessage::Welcome {
            client_id,
            motd: config.server.motd.clone(),
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
            if config.enforcement.mode == SessionEnforcementPolicy::Strict {
                session.fail(format!("resource announcement transition failed: {e}"));
                if handle_enforcement(
                    &session,
                    &mut event_log,
                    &session_id,
                    &state.registry,
                    &config,
                    client_id,
                    &mut writer_half,
                )
                .await?
                {
                    return Ok(());
                }
            }
        } else {
            event_log.record(
                SessionEventKind::ResourceAnnouncementSent,
                SessionState::ResourceAnnouncementSent,
                "resource announcement sent to client",
            );
            state.registry.lock().unwrap().update_session(
                &session_id,
                session.state().clone(),
                event_log.len(),
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

    // Server-initiated heartbeat scheduler.
    let srv_ping_enabled = config.heartbeat.server_ping_interval_ms > 0;
    let srv_ping_dur =
        Duration::from_millis(config.heartbeat.server_ping_interval_ms.max(1));
    // interval_at schedules the first tick at now+dur, avoiding the immediate t=0 fire.
    let mut srv_ping_interval = time::interval_at(
        time::Instant::now() + srv_ping_dur,
        srv_ping_dur,
    );
    srv_ping_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let mut srv_ping_sequence: u64 = 1;

    loop {
        tokio::select! {
            maybe_line = lines.next_line() => {
                let line = match maybe_line? {
                    Some(l) => l,
                    None => break,
                };
                match decode_client_line(&line)? {
            ClientMessage::Ping { sequence } => {
                // Heartbeat ping: reply with Pong echoing the sequence.
                // Record minimal event for observability but do not change session state.
                event_log.record(
                    SessionEventKind::PingReceived,
                    session.state().clone(),
                    format!("heartbeat: received ping {}", sequence),
                );
                // Use broadcast channel so the message is delivered via the writer task.
                let _ = tx.send(ServerMessage::Pong { sequence });
                event_log.record(
                    SessionEventKind::PongSent,
                    session.state().clone(),
                    format!("heartbeat: sent pong {}", sequence),
                );
                state.registry.lock().unwrap().update_session_heartbeat_counts(
                    &session_id,
                    event_log.count_kind(SessionEventKind::PingReceived),
                    event_log.count_kind(SessionEventKind::PongSent),
                );
            }
            ClientMessage::ServerPong { sequence } => {
                event_log.record(
                    SessionEventKind::ServerPongReceived,
                    session.state().clone(),
                    format!("server heartbeat: received ServerPong {}", sequence),
                );
                state.registry.lock().unwrap().update_server_heartbeat_counts(
                    &session_id,
                    event_log.count_kind(SessionEventKind::ServerPingSent),
                    event_log.count_kind(SessionEventKind::ServerPongReceived),
                );
                info!(%client_id, sequence, "received server_pong");
            }
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
                        if config.enforcement.mode == SessionEnforcementPolicy::Strict {
                            session.fail(format!("availability report transition failed: {e}"));
                            event_log.record(
                                SessionEventKind::Failed,
                                SessionState::Failed,
                                format!("enforcement: availability report transition failed: {e}"),
                            );
                            state.registry.lock().unwrap().update_session(
                                &session_id,
                                SessionState::Failed,
                                event_log.len(),
                            );
                            if config.diagnostics.print_session_diagnostics {
                        let diag = SessionDiagnostics::from_parts(&session, &event_log)
                            .with_capability_negotiation(&capability_negotiation)
                            .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                                let text = match config.diagnostics.format {
                                    Fmt::Text => diag.to_text(),
                                    Fmt::JsonStub => diag.to_json_stub(),
                                };
                                info!(%client_id, "session diagnostics (enforcement):\n{text}");
                            }
                            let _ = client_tx.send(ServerMessage::Disconnect {
                                reason: DisconnectReason::InvalidHandshake,
                                message: format!(
                                    "enforcement: availability report transition failed: {e}"
                                ),
                            });
                            info!(%client_id, "enforcement: session disconnected due to availability report failure");
                            break;
                        }
                    } else {
                        event_log.record(
                            SessionEventKind::AvailabilityReportReceived,
                            SessionState::AvailabilityReportReceived,
                            "resource availability report received from client",
                        );
                        state.registry.lock().unwrap().update_session(
                            &session_id,
                            session.state().clone(),
                            event_log.len(),
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
                            state.registry.lock().unwrap().update_session(
                                &session_id,
                                session.state().clone(),
                                event_log.len(),
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
                            state.registry.lock().unwrap().update_session(
                                &session_id,
                                session.state().clone(),
                                event_log.len(),
                            );
                            info!(
                                %client_id,
                                state = ?session.state(),
                                "session: resource policy would block (dry-run only, not enforced)"
                            );
                        }
                        Err(e) => {
                            warn!(%client_id, error = %e, "session: unexpected policy transition error");
                            if config.enforcement.mode == SessionEnforcementPolicy::Strict {
                                session.fail(format!("policy transition failed: {e}"));
                                event_log.record(
                                    SessionEventKind::Failed,
                                    SessionState::Failed,
                                    format!("enforcement: policy transition failed: {e}"),
                                );
                                state.registry.lock().unwrap().update_session(
                                    &session_id,
                                    SessionState::Failed,
                                    event_log.len(),
                                );
                                if config.diagnostics.print_session_diagnostics {
                                    let diag = SessionDiagnostics::from_parts(&session, &event_log)
                                        .with_capability_negotiation(&capability_negotiation)
                                        .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                                    let text = match config.diagnostics.format {
                                        Fmt::Text => diag.to_text(),
                                        Fmt::JsonStub => diag.to_json_stub(),
                                    };
                                    info!(%client_id, "session diagnostics (enforcement):\n{text}");
                                }
                                let _ = client_tx.send(ServerMessage::Disconnect {
                                    reason: DisconnectReason::InvalidHandshake,
                                    message: format!("enforcement: policy transition failed: {e}"),
                                });
                                info!(%client_id, "enforcement: session disconnected due to policy transition failure");
                                break;
                            }
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
                        if config.enforcement.mode == SessionEnforcementPolicy::Strict {
                            session.fail(format!("join gate transition failed: {e}"));
                            event_log.record(
                                SessionEventKind::Failed,
                                SessionState::Failed,
                                format!("enforcement: join gate transition failed: {e}"),
                            );
                            state.registry.lock().unwrap().update_session(
                                &session_id,
                                SessionState::Failed,
                                event_log.len(),
                            );
                            if config.diagnostics.print_session_diagnostics {
                                let diag = SessionDiagnostics::from_parts(&session, &event_log)
                                    .with_capability_negotiation(&capability_negotiation)
                                    .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                                let text = match config.diagnostics.format {
                                    Fmt::Text => diag.to_text(),
                                    Fmt::JsonStub => diag.to_json_stub(),
                                };
                                info!(%client_id, "session diagnostics (enforcement):\n{text}");
                            }
                            let _ = client_tx.send(ServerMessage::Disconnect {
                                reason: DisconnectReason::InvalidHandshake,
                                message: format!("enforcement: join gate transition failed: {e}"),
                            });
                            info!(%client_id, "enforcement: session disconnected due to join gate transition failure");
                            break;
                        }
                    } else {
                        event_log.record(
                            SessionEventKind::JoinGateDryRunSent,
                            SessionState::JoinGateDryRunSent,
                            "join gate dry-run decision sent to client",
                        );
                        state.registry.lock().unwrap().update_session(
                            &session_id,
                            session.state().clone(),
                            event_log.len(),
                        );
                        info!(%client_id, state = ?session.state(), "session: join gate dry-run sent");
                    }

                    if let Err(e) = session.mark_ready_dry_run() {
                        warn!(%client_id, error = %e, "session: unexpected ready transition error");
                        if config.enforcement.mode == SessionEnforcementPolicy::Strict {
                            session.fail(format!("ready transition failed: {e}"));
                            event_log.record(
                                SessionEventKind::Failed,
                                SessionState::Failed,
                                format!("enforcement: ready transition failed: {e}"),
                            );
                            state.registry.lock().unwrap().update_session(
                                &session_id,
                                SessionState::Failed,
                                event_log.len(),
                            );
                            if config.diagnostics.print_session_diagnostics {
                                let diag = SessionDiagnostics::from_parts(&session, &event_log)
                                    .with_capability_negotiation(&capability_negotiation)
                                    .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                                let text = match config.diagnostics.format {
                                    Fmt::Text => diag.to_text(),
                                    Fmt::JsonStub => diag.to_json_stub(),
                                };
                                info!(%client_id, "session diagnostics (enforcement):\n{text}");
                            }
                            let _ = client_tx.send(ServerMessage::Disconnect {
                                reason: DisconnectReason::InvalidHandshake,
                                message: format!("enforcement: ready transition failed: {e}"),
                            });
                            info!(%client_id, "enforcement: session disconnected due to ready transition failure");
                            break;
                        }
                    } else {
                        event_log.record(
                            SessionEventKind::ReadyDryRun,
                            SessionState::ReadyDryRun,
                            "handshake pipeline complete (dry-run)",
                        );
                        state.registry.lock().unwrap().update_session(
                            &session_id,
                            session.state().clone(),
                            event_log.len(),
                        );
                        info!(%client_id, state = ?session.state(), "session: ready (dry-run)");
                        if config.diagnostics.print_session_diagnostics {
                            let diag = SessionDiagnostics::from_parts(&session, &event_log)
                                .with_capability_negotiation(&capability_negotiation)
                                .with_enforcement(&config.enforcement.mode)
                            .with_heartbeat_policy(&config.heartbeat.policy);
                            let text = match config.diagnostics.format {
                                Fmt::Text => diag.to_text(),
                                Fmt::JsonStub => diag.to_json_stub(),
                            };
                            info!(%client_id, "session diagnostics:\n{text}");
                        }
                    }
                } else {
                    warn!(%client_id, "resource availability report received before announcement was stored");
                }
            }
                } // close match decode_client_line
            } // close maybe_line arm
            _ = srv_ping_interval.tick(), if srv_ping_enabled => {
                let _ = client_tx.send(ServerMessage::ServerPing { sequence: srv_ping_sequence });
                event_log.record(
                    SessionEventKind::ServerPingSent,
                    session.state().clone(),
                    format!("server heartbeat: sent ServerPing {}", srv_ping_sequence),
                );
                let pings_sent = event_log.count_kind(SessionEventKind::ServerPingSent);
                let pongs_received = event_log.count_kind(SessionEventKind::ServerPongReceived);
                state.registry.lock().unwrap().update_server_heartbeat_counts(
                    &session_id,
                    pings_sent,
                    pongs_received,
                );
                info!(%client_id, sequence = srv_ping_sequence, "sent server_ping");
                srv_ping_sequence = srv_ping_sequence.saturating_add(1);

                if config.heartbeat.policy == HeartbeatPolicy::Strict {
                    let srv_hb_input = ServerHeartbeatPlannerInput {
                        pings_sent: pings_sent as u64,
                        pongs_received: pongs_received as u64,
                    };
                    if evaluate_server_heartbeat(&srv_hb_input, &HeartbeatPolicy::Strict)
                        == ServerHeartbeatDecision::WouldDisconnect
                    {
                        let missed = pings_sent.saturating_sub(pongs_received);
                        let reason = format!(
                            "server heartbeat enforcement: {} missed pong(s), threshold {}",
                            missed, MISSED_SERVER_PONG_DISCONNECT_THRESHOLD
                        );
                        session.fail(reason.clone());
                        warn!(
                            %client_id,
                            pings_sent,
                            pongs_received,
                            missed,
                            "server heartbeat enforcement: missed pong threshold reached, disconnecting"
                        );
                        event_log.record(
                            SessionEventKind::Failed,
                            SessionState::Failed,
                            reason.clone(),
                        );
                        state.registry.lock().unwrap().update_session(
                            &session_id,
                            SessionState::Failed,
                            event_log.len(),
                        );
                        if config.diagnostics.print_session_diagnostics {
                            let diag = SessionDiagnostics::from_parts(&session, &event_log)
                                .with_capability_negotiation(&capability_negotiation)
                                .with_enforcement(&config.enforcement.mode)
                                .with_heartbeat_policy(&config.heartbeat.policy);
                            let text = match config.diagnostics.format {
                                Fmt::Text => diag.to_text(),
                                Fmt::JsonStub => diag.to_json_stub(),
                            };
                            info!(%client_id, "session diagnostics (server heartbeat enforcement):\n{text}");
                        }
                        // Best-effort: Disconnect is queued via client_tx, but writer_task.abort()
                        // (after the loop) may preempt the writer before it flushes the frame.
                        // TCP close via writer_half drop on abort is the authoritative signal.
                        let _ = client_tx.send(ServerMessage::Disconnect {
                            reason: DisconnectReason::InvalidHandshake,
                            message: format!(
                                "server heartbeat: {} missed pong(s) exceeded threshold of {}",
                                missed, MISSED_SERVER_PONG_DISCONNECT_THRESHOLD
                            ),
                        });
                        info!(%client_id, missed, "server heartbeat enforcement: session disconnected");
                        break;
                    }
                }
            }
        } // close select!
    } // close loop

    state.clients.write().await.remove(&client_id);
    let _ = tx.send(ServerMessage::ChatBroadcast {
        from: "server".to_string(),
        message: format!("{name} left"),
    });

    // Aborting writer_task drops writer_half, closing the TCP write half and delivering
    // EOF to the client. This is the authoritative connection close for all loop exit paths.
    writer_task.abort();
    state
        .registry
        .lock()
        .unwrap()
        .update_session_event_count(&session_id, event_log.len());
    info!(
        %client_id,
        event_count = event_log.len(),
        final_state = ?session.state(),
        "session audit log complete"
    );
    Ok(())
}

/// Evaluate session enforcement and disconnect if required under Strict policy.
///
/// Under `ReportOnly`, this function logs the enforcement decision in
/// diagnostics if the session has failed, then returns `Ok(false)`.
///
/// Under `Strict`, if the enforcement decision is not `Allow`:
/// - Records a `Failed` event in the event log
/// - Updates the session registry
/// - Prints diagnostics with enforcement context
/// - Sends a `Disconnect` message to the client
/// - Returns `Ok(true)` to signal the caller to stop processing
///
/// Returns `Ok(false)` if no enforcement action is taken.
///
/// Called only during the handshake phase, before writer_task is spawned.
/// `writer` is the raw TCP write half so Disconnect delivery is direct and guaranteed.
async fn handle_enforcement(
    session: &SessionStateMachine,
    event_log: &mut SessionEventLog,
    session_id: &session_registry::SessionId,
    registry: &Arc<std::sync::Mutex<session_registry::SessionRegistry>>,
    config: &ServerConfig,
    client_id: Uuid,
    writer: &mut (impl AsyncWriteExt + Unpin),
) -> Result<bool> {
    let policy = &config.enforcement.mode;
    let decision = evaluate_enforcement(session.state(), session.failure_reason(), policy);

    match policy {
        SessionEnforcementPolicy::ReportOnly => {
            if config.diagnostics.print_session_diagnostics
                && *session.state() == SessionState::Failed
            {
                let diag =
                    SessionDiagnostics::from_parts(session, event_log)
                        .with_enforcement(policy)
                        .with_heartbeat_policy(&config.heartbeat.policy);
                let text = match config.diagnostics.format {
                    Fmt::Text => diag.to_text(),
                    Fmt::JsonStub => diag.to_json_stub(),
                };
                info!(%client_id, "session diagnostics (enforcement context):\n{text}");
            }
            Ok(false)
        }
        SessionEnforcementPolicy::Strict => {
            if decision == SessionEnforcementDecision::Allow {
                return Ok(false);
            }

            event_log.record(
                SessionEventKind::Failed,
                SessionState::Failed,
                session.failure_reason().unwrap_or("enforcement triggered"),
            );

            {
                let mut reg = registry.lock().unwrap();
                reg.update_session(session_id, SessionState::Failed, event_log.len());
            }

            if config.diagnostics.print_session_diagnostics {
                let diag =
                    SessionDiagnostics::from_parts(session, event_log)
                        .with_enforcement(policy)
                        .with_heartbeat_policy(&config.heartbeat.policy);
                let text = match config.diagnostics.format {
                    Fmt::Text => diag.to_text(),
                    Fmt::JsonStub => diag.to_json_stub(),
                };
                info!(%client_id, "session diagnostics (enforcement):\n{text}");
            }

            let (reason, msg) = map_decision_to_disconnect(&decision);
            send_direct(
                writer,
                &ServerMessage::Disconnect {
                    reason,
                    message: msg,
                },
            )
            .await?;

            info!(
                %client_id,
                decision = %decision.to_text(),
                "enforcement: session disconnected"
            );

            Ok(true)
        }
    }
}

fn map_decision_to_disconnect(decision: &SessionEnforcementDecision) -> (DisconnectReason, String) {
    match decision {
        SessionEnforcementDecision::Allow => unreachable!(),
        SessionEnforcementDecision::WouldDisconnectInvalidFirstMessage => (
            DisconnectReason::InvalidHandshake,
            "invalid first message".to_string(),
        ),
        SessionEnforcementDecision::WouldDisconnectVersionMismatch { client, server } => (
            DisconnectReason::ProtocolMismatch,
            format!("protocol mismatch: client={client} server={server}"),
        ),
        SessionEnforcementDecision::WouldDisconnectCapabilityGateFailure { capability } => (
            DisconnectReason::InvalidHandshake,
            format!("capability gate failure: {capability}"),
        ),
        SessionEnforcementDecision::WouldDisconnectInvalidStateTransition { from, to } => (
            DisconnectReason::InvalidHandshake,
            format!("invalid state transition: {from} -> {to}"),
        ),
        SessionEnforcementDecision::WouldMarkSessionFailed { reason } => {
            (DisconnectReason::InvalidHandshake, reason.clone())
        }
    }
}

async fn send_direct<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &ServerMessage) -> Result<()> {
    writer.write_all(encode_line(msg)?.as_bytes()).await?;
    Ok(())
}

fn next_entity_id(client_id: &Uuid) -> u32 {
    let bytes = client_id.as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn build_example_resource_announcement(resource_dir: &str) -> Option<ResourceAnnouncement> {
    let path = std::path::Path::new(resource_dir);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(resource_dir)
    };
    let index = build_pack_index(resolved).ok()?;
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
