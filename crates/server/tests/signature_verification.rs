use std::sync::Arc;

use anyhow::Result;
use protocol::signature_engine::{
    SignaturePolicy, evaluate_signature_policy, execute_verification_plan,
};
use protocol::{
    ClientMessage, PROTOCOL_VERSION, ResourceAvailabilityEntry, ResourceAvailabilityReport,
    ResourceAvailabilityStatus, ServerMessage, TrustedKey, build_signature_verification_plan,
    decode_server_line, encode_line,
};
use server::{ServerConfig, ServerSection, SharedState, run_with_listener_and_state};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

fn test_config(addr: &str) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: addr.to_string(),
            tick_rate: 20,
            motd: "signature test".to_string(),
            ..ServerSection::default()
        },
        ..ServerConfig::default()
    }
}

async fn read_packet<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<ServerMessage>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = timeout(Duration::from_secs(2), lines.next_line())
        .await??
        .expect("stream closed before packet arrived");
    Ok(decode_server_line(&line)?)
}

#[tokio::test]
async fn live_unsigned_announcement_report_only_accepts() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = test_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state.clone()));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _welcome = read_packet(&mut lines).await?;
    assert!(matches!(_welcome, ServerMessage::Welcome { .. }));

    let announcement = match read_packet(&mut lines).await? {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };
    assert_eq!(announcement.resources.len(), 1);
    assert!(
        announcement.signature.is_none(),
        "server produces unsigned announcements"
    );

    let trusted_keys: Vec<protocol::signature_engine::TrustedPublicKey> = vec![];
    let key_identities: Vec<TrustedKey> = vec![];

    let plan = build_signature_verification_plan(&announcement, &key_identities, false);
    assert!(!plan.entries.is_empty());
    assert_eq!(
        plan.entries[0].action,
        protocol::SignatureVerificationAction::MissingSignature
    );

    let report = execute_verification_plan(&announcement, &plan, &trusted_keys);
    assert!(
        !report.all_valid(),
        "unsigned announcement should not be valid"
    );

    assert!(
        evaluate_signature_policy(&report, &SignaturePolicy::ReportOnly).is_ok(),
        "ReportOnly should always accept"
    );

    let report_text = report.to_text();
    assert!(
        report_text.contains("skipped"),
        "should report skipped: {report_text}"
    );

    let resources = announcement
        .resources
        .iter()
        .flat_map(|r| {
            r.files.iter().map(|f| ResourceAvailabilityEntry {
                resource_name: r.name.clone(),
                file_path: f.relative_path.clone(),
                status: ResourceAvailabilityStatus::Available,
            })
        })
        .collect();
    writer_half
        .write_all(
            encode_line(&ClientMessage::ResourceAvailabilityReport(
                ResourceAvailabilityReport {
                    resources,
                    is_fully_available: true,
                },
            ))?
            .as_bytes(),
        )
        .await?;

    loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(_) => {
                break;
            }
            ServerMessage::ChatBroadcast { .. } | ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    drop(lines);
    drop(writer_half);
    tokio::time::sleep(Duration::from_millis(100)).await;
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn live_unsigned_announcement_strict_would_reject() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = test_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state.clone()));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _welcome = read_packet(&mut lines).await?;

    let announcement = match read_packet(&mut lines).await? {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    let trusted_keys: Vec<protocol::signature_engine::TrustedPublicKey> = vec![];
    let key_identities: Vec<TrustedKey> = vec![];

    let plan = build_signature_verification_plan(&announcement, &key_identities, false);
    let report = execute_verification_plan(&announcement, &plan, &trusted_keys);

    let violation = evaluate_signature_policy(&report, &SignaturePolicy::Strict);
    assert!(
        violation.is_err(),
        "Strict should reject unsigned announcement"
    );
    let err_msg = violation.unwrap_err().message;
    assert!(
        err_msg.contains("signature policy violation"),
        "violation message: {err_msg}"
    );

    drop(lines);
    drop(writer_half);
    tokio::time::sleep(Duration::from_millis(100)).await;
    server_task.abort();
    Ok(())
}
