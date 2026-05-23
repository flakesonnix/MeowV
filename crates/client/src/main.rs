use std::env;

use anyhow::{Context, Result};
use game_edition::{GameEdition, GamePlatform};
use protocol::{
    check_announcement_signature_stub, decode_server_line, encode_line, AnnouncedResource,
    ClientMessage, JoinGateDecision, JoinGateMode, JoinGateOutcome, ResourceAvailabilityEntry,
    ResourceAvailabilityReport, ResourceAvailabilityStatus, ServerMessage,
    SignatureVerificationStatus, PROTOCOL_VERSION,
};
use resource_manifest::{
    build_load_plan_from_root, build_pack_index, default_compatibility_context, discover_resources,
    evaluate_manifest_compatibility, load_manifest_from_path, resolve_load_order,
    verify_cache_for_resource, CacheFileStatus, CompatibilityStatus, ResourceEntrypointKind,
    ResourceManifest, ResourceRuntimePhase, ResourceRuntimeState, ResourceRuntimeStateMachine,
};
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
    resource_cache: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:7000".to_string(),
            name: "dummy-client".to_string(),
            message: "hello from client".to_string(),
            resource_cache: None,
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

        if let Ok(resource_cache) = env::var("MEOWV_RESOURCE_CACHE") {
            cfg.resource_cache = Some(resource_cache);
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

        if let Some(resource_cache) = read_flag(args, "--resource-cache") {
            cfg.resource_cache = Some(resource_cache);
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

    if let Some(path) = read_flag(&args, "--resource-manifest") {
        print_resource_manifest(&path)?;
        return Ok(());
    }

    if let Some(path) = read_flag(&args, "--resource-index") {
        print_resource_index(&path)?;
        return Ok(());
    }

    if let Some((resource_dir, cache_dir)) = read_pair_flag(&args, "--verify-cache") {
        print_cache_verification(&resource_dir, &cache_dir)?;
        return Ok(());
    }

    if let Some(path) = read_flag(&args, "--resource-registry") {
        print_resource_registry(&path)?;
        return Ok(());
    }

    if let Some(path) = read_flag(&args, "--resource-load-plan") {
        print_resource_load_plan(&path)?;
        return Ok(());
    }

    if let Some(path) = read_flag(&args, "--resource-runtime-plan") {
        print_resource_runtime_plan(&path)?;
        return Ok(());
    }

    if let Some(path) = read_flag(&args, "--check-resource-compatibility") {
        print_resource_compatibility(&args, &path)?;
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
        match packet {
            ServerMessage::ResourceAnnouncement(announcement) => {
                let signature_report = check_announcement_signature_stub(&announcement);
                print_signature_report(&signature_report);
                let report =
                    handle_resource_announcement(&announcement, config.resource_cache.as_deref())?;
                writer_half
                    .write_all(
                        encode_line(&ClientMessage::ResourceAvailabilityReport(report))?.as_bytes(),
                    )
                    .await?;
                println!("Resource availability report sent.");
            }
            ServerMessage::JoinGateDecision(decision) => {
                print_join_gate_decision(&decision);
            }
            other => {
                info!(packet = ?other, "received packet");
            }
        }
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

fn print_resource_manifest(path: &str) -> Result<()> {
    let manifest = load_manifest_from_path(path)?;

    println!("Name: {}", manifest.name);
    println!("Version: {}", manifest.version);
    println!(
        "Description: {}",
        manifest.description.as_deref().unwrap_or("<none>")
    );
    println!("Authors: {}", manifest.authors.join(", "));
    println!(
        "License: {}",
        manifest.license.as_deref().unwrap_or("<none>")
    );
    println!("Protocol: {}", manifest.protocol_version);
    println!("Edition: {}", format_manifest_edition(&manifest));
    println!(
        "Server Entrypoint: {}",
        manifest.entrypoints.server.as_deref().unwrap_or("<none>")
    );
    println!(
        "Client Entrypoint: {}",
        manifest.entrypoints.client.as_deref().unwrap_or("<none>")
    );
    println!(
        "Dependencies: {}",
        format_dependencies(&manifest).unwrap_or_else(|| "<none>".to_string())
    );
    println!("Tags: {}", manifest.tags.join(", "));

    Ok(())
}

fn print_resource_index(path: &str) -> Result<()> {
    let index = build_pack_index(path)?;

    println!("Name: {}", index.manifest.name);
    println!("Version: {}", index.manifest.version);
    println!("Files: {}", index.files.len());
    println!("Total Size: {} bytes", index.total_size_bytes);

    for file in &index.files {
        println!(
            "- {} | {} bytes | {}",
            file.relative_path.display(),
            file.size_bytes,
            file.sha256
        );
    }

    Ok(())
}

fn print_cache_verification(resource_dir: &str, cache_dir: &str) -> Result<()> {
    let report = verify_cache_for_resource(resource_dir, cache_dir)?;

    println!("Valid: {}", report.valid_count);
    println!("Missing: {}", report.missing_count);
    println!("Size Mismatch: {}", report.size_mismatch_count);
    println!("Hash Mismatch: {}", report.hash_mismatch_count);

    for entry in &report.entries {
        println!(
            "- {} | {} | expected {} bytes | actual {} bytes | expected {} | actual {}",
            entry.relative_path.display(),
            format_cache_status(&entry.status),
            entry.expected_size_bytes,
            entry
                .actual_size_bytes
                .map(|size| size.to_string())
                .unwrap_or_else(|| "<missing>".to_string()),
            entry.expected_sha256,
            entry.actual_sha256.as_deref().unwrap_or("<missing>")
        );
    }

    println!(
        "Result: {}",
        if report.is_fully_valid {
            "OK"
        } else {
            "FAILED"
        }
    );

    Ok(())
}

fn print_resource_registry(path: &str) -> Result<()> {
    let registry = discover_resources(path)?;
    let load_order = resolve_load_order(&registry)?;

    println!("Discovered Resources: {}", registry.resources.len());

    for resource in registry.resources.values() {
        let dependencies = if resource.manifest.dependencies.is_empty() {
            "<none>".to_string()
        } else {
            resource
                .manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        println!("- {} | deps: {}", resource.name, dependencies);
    }

    println!("Load Order: {}", load_order.resources.join(" -> "));
    Ok(())
}

fn print_resource_load_plan(path: &str) -> Result<()> {
    let plan = build_load_plan_from_root(path)?;

    println!("Resource Load Order: {}", plan.resources.len());
    for resource in &plan.resources {
        println!("- {}", resource.name);
        println!(
            "  Dependencies: {}",
            if resource.dependencies.is_empty() {
                "<none>".to_string()
            } else {
                resource.dependencies.join(", ")
            }
        );
        println!("  Phase: {}", format_runtime_phase(&resource.phase));

        let server_entrypoints = resource
            .entrypoints
            .iter()
            .filter(|entrypoint| entrypoint.kind == ResourceEntrypointKind::Server)
            .map(|entrypoint| entrypoint.path.display().to_string())
            .collect::<Vec<_>>();
        let client_entrypoints = resource
            .entrypoints
            .iter()
            .filter(|entrypoint| entrypoint.kind == ResourceEntrypointKind::Client)
            .map(|entrypoint| entrypoint.path.display().to_string())
            .collect::<Vec<_>>();

        println!(
            "  Server Entrypoints: {}",
            if server_entrypoints.is_empty() {
                "<none>".to_string()
            } else {
                server_entrypoints.join(", ")
            }
        );
        println!(
            "  Client Entrypoints: {}",
            if client_entrypoints.is_empty() {
                "<none>".to_string()
            } else {
                client_entrypoints.join(", ")
            }
        );
    }

    println!("No scripts were executed.");
    Ok(())
}

fn print_resource_runtime_plan(path: &str) -> Result<()> {
    let plan = build_load_plan_from_root(path)?;
    let mut machine = ResourceRuntimeStateMachine::from_load_plan(&plan);

    for resource in &plan.resources {
        machine.validate_resource(&resource.name)?;
        machine.mark_ready(&resource.name)?;
        machine.start_resource_no_exec(&resource.name)?;
    }

    for resource in &plan.resources {
        let status = machine
            .status(&resource.name)
            .expect("resource status must exist");
        println!("- {}", resource.name);
        println!(
            "  Dependencies: {}",
            if resource.dependencies.is_empty() {
                "<none>".to_string()
            } else {
                resource.dependencies.join(", ")
            }
        );
        println!("  Final State: {}", format_runtime_state(&status.state));
        println!(
            "  Message: {}",
            status.message.as_deref().unwrap_or("<none>")
        );
    }

    println!("No scripts were executed.");
    Ok(())
}

fn print_resource_compatibility(args: &[String], path: &str) -> Result<()> {
    let manifest = load_manifest_from_path(path)?;
    let mut context = default_compatibility_context();

    if let Some(raw) = read_flag(args, "--game-edition") {
        context.game_edition = parse_game_edition(&raw)?;
    }

    if let Some(raw) = read_flag(args, "--game-platform") {
        context.platform = parse_game_platform(&raw)?;
    }

    let report = evaluate_manifest_compatibility(&manifest, &context);
    println!(
        "Compatibility Status: {}",
        format_compatibility_status(&report.status)
    );
    if report.issues.is_empty() {
        println!("Issues: <none>");
    } else {
        for issue in report.issues {
            println!("- {}: {}", issue.code, issue.message);
        }
    }

    Ok(())
}

fn handle_resource_announcement(
    announcement: &protocol::ResourceAnnouncement,
    resource_cache: Option<&str>,
) -> Result<ResourceAvailabilityReport> {
    let mut entries = Vec::new();

    for resource in &announcement.resources {
        println!("Announced Resource: {} {}", resource.name, resource.version);
        for file in &resource.files {
            println!(
                "- {} | {} bytes | {}",
                file.relative_path, file.size_bytes, file.sha256
            );
        }

        entries.extend(build_availability_entries(resource, resource_cache)?);
    }

    let is_fully_available = entries
        .iter()
        .all(|entry| entry.status == ResourceAvailabilityStatus::Available);
    let report = ResourceAvailabilityReport {
        resources: entries,
        is_fully_available,
    };

    println!(
        "Resource Availability: {}",
        if report.is_fully_available {
            "all files available"
        } else {
            "missing or mismatched files detected"
        }
    );

    Ok(report)
}

fn build_availability_entries(
    resource: &AnnouncedResource,
    resource_cache: Option<&str>,
) -> Result<Vec<ResourceAvailabilityEntry>> {
    if let Some(cache_dir) = resource_cache {
        let report =
            verify_cache_for_resource(format!("examples/resources/{}", resource.name), cache_dir)?;
        return Ok(report
            .entries
            .into_iter()
            .map(|entry| ResourceAvailabilityEntry {
                resource_name: resource.name.clone(),
                file_path: entry.relative_path.to_string_lossy().into_owned(),
                status: map_cache_status(entry.status),
            })
            .collect());
    }

    Ok(resource
        .files
        .iter()
        .map(|file| ResourceAvailabilityEntry {
            resource_name: resource.name.clone(),
            file_path: file.relative_path.clone(),
            status: ResourceAvailabilityStatus::Missing,
        })
        .collect())
}

fn format_edition(edition: &server_browser::EditionCompatibility) -> &'static str {
    match edition {
        server_browser::EditionCompatibility::Legacy => "legacy",
        server_browser::EditionCompatibility::Enhanced => "enhanced",
        server_browser::EditionCompatibility::Any => "any",
        server_browser::EditionCompatibility::Unknown => "unknown",
    }
}

fn format_manifest_edition(manifest: &ResourceManifest) -> &'static str {
    match manifest.edition_compatibility {
        resource_manifest::EditionCompatibility::Legacy => "legacy",
        resource_manifest::EditionCompatibility::Enhanced => "enhanced",
        resource_manifest::EditionCompatibility::Any => "any",
        resource_manifest::EditionCompatibility::Unknown => "unknown",
    }
}

fn format_cache_status(status: &CacheFileStatus) -> &'static str {
    match status {
        CacheFileStatus::Valid => "valid",
        CacheFileStatus::Missing => "missing",
        CacheFileStatus::SizeMismatch => "size_mismatch",
        CacheFileStatus::HashMismatch => "hash_mismatch",
    }
}

fn format_runtime_phase(phase: &ResourceRuntimePhase) -> &'static str {
    match phase {
        ResourceRuntimePhase::Planned => "planned",
        ResourceRuntimePhase::Validated => "validated",
        ResourceRuntimePhase::Ready => "ready",
        ResourceRuntimePhase::Skipped => "skipped",
    }
}

fn format_runtime_state(state: &ResourceRuntimeState) -> &'static str {
    match state {
        ResourceRuntimeState::Planned => "planned",
        ResourceRuntimeState::Validated => "validated",
        ResourceRuntimeState::Ready => "ready",
        ResourceRuntimeState::Started => "started",
        ResourceRuntimeState::Stopped => "stopped",
        ResourceRuntimeState::Failed => "failed",
    }
}

fn map_cache_status(status: CacheFileStatus) -> ResourceAvailabilityStatus {
    match status {
        CacheFileStatus::Valid => ResourceAvailabilityStatus::Available,
        CacheFileStatus::Missing => ResourceAvailabilityStatus::Missing,
        CacheFileStatus::SizeMismatch => ResourceAvailabilityStatus::SizeMismatch,
        CacheFileStatus::HashMismatch => ResourceAvailabilityStatus::HashMismatch,
    }
}

fn print_join_gate_decision(decision: &JoinGateDecision) {
    println!("Join Gate Mode: {}", format_join_gate_mode(&decision.mode));
    println!(
        "Join Gate Outcome: {}",
        format_join_gate_outcome(&decision.outcome)
    );
    println!("Reason: {}", decision.reason);
    println!(
        "Missing Required: {}",
        summarize_list(&decision.policy_evaluation.missing_required)
    );
    println!(
        "Invalid Required: {}",
        summarize_list(&decision.policy_evaluation.invalid_required)
    );
    println!(
        "Missing Optional: {}",
        summarize_list(&decision.policy_evaluation.missing_optional)
    );
    println!(
        "Invalid Optional: {}",
        summarize_list(&decision.policy_evaluation.invalid_optional)
    );
    println!(
        "Missing Recommended: {}",
        summarize_list(&decision.policy_evaluation.missing_recommended)
    );
    println!(
        "Invalid Recommended: {}",
        summarize_list(&decision.policy_evaluation.invalid_recommended)
    );
    println!("Dry-run only: no disconnects or enforcement were applied.");
}

fn format_join_gate_mode(mode: &JoinGateMode) -> &'static str {
    match mode {
        JoinGateMode::DryRun => "dry_run",
        JoinGateMode::Enforced => "enforced",
    }
}

fn format_join_gate_outcome(outcome: &JoinGateOutcome) -> &'static str {
    match outcome {
        JoinGateOutcome::WouldAllow => "would_allow",
        JoinGateOutcome::WouldWarn => "would_warn",
        JoinGateOutcome::WouldBlock => "would_block",
    }
}

fn summarize_list(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(", ")
    }
}

fn print_signature_report(report: &protocol::SignatureVerificationReport) {
    println!(
        "Announcement Signature Status: {}",
        format_signature_status(&report.status)
    );
    println!("Signature Reason: {}", report.reason);
}

fn format_signature_status(status: &SignatureVerificationStatus) -> &'static str {
    match status {
        SignatureVerificationStatus::NotProvided => "not_provided",
        SignatureVerificationStatus::UnsupportedAlgorithm => "unsupported_algorithm",
        SignatureVerificationStatus::Invalid => "invalid",
        SignatureVerificationStatus::Valid => "valid",
        SignatureVerificationStatus::NotChecked => "not_checked",
    }
}

fn format_compatibility_status(status: &CompatibilityStatus) -> &'static str {
    match status {
        CompatibilityStatus::Compatible => "compatible",
        CompatibilityStatus::Incompatible => "incompatible",
        CompatibilityStatus::Unknown => "unknown",
    }
}

fn parse_game_edition(value: &str) -> Result<GameEdition> {
    match value {
        "legacy" => Ok(GameEdition::Legacy),
        "enhanced" => Ok(GameEdition::Enhanced),
        "unknown" => Ok(GameEdition::Unknown),
        other => anyhow::bail!("invalid --game-edition: {other}"),
    }
}

fn parse_game_platform(value: &str) -> Result<GamePlatform> {
    match value {
        "windows" => Ok(GamePlatform::Windows),
        "linux" => Ok(GamePlatform::Linux),
        "unknown" => Ok(GamePlatform::Unknown),
        other => anyhow::bail!("invalid --game-platform: {other}"),
    }
}

fn format_dependencies(manifest: &ResourceManifest) -> Option<String> {
    if manifest.dependencies.is_empty() {
        None
    } else {
        Some(
            manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
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

fn read_pair_flag(args: &[String], name: &str) -> Option<(String, String)> {
    args.windows(3)
        .find(|window| window[0] == name)
        .map(|window| (window[1].clone(), window[2].clone()))
}
