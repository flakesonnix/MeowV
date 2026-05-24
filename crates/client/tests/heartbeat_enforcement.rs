use anyhow::Result;
use protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_client_line, encode_line};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};

/// Start a TCP server that handles the login handshake but never responds to Pings.
/// Returns the bound address. The server task runs until the connection closes.
async fn spawn_no_pong_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, mut w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();

        // Handle Login → send Welcome + ResourceAnnouncement
        if let Ok(Some(line)) = lines.next_line().await {
            let _ = decode_client_line(&line);
            let welcome = encode_line(&ServerMessage::Welcome {
                client_id: uuid::Uuid::new_v4(),
                motd: "enforcement test".to_string(),
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();
            let announcement = encode_line(&ServerMessage::ResourceAnnouncement(
                protocol::ResourceAnnouncement {
                    resources: vec![],
                    signature: None,
                },
            ))
            .unwrap();
            let _ = w.write_all(welcome.as_bytes()).await;
            let _ = w.write_all(announcement.as_bytes()).await;
        }

        // Read and ignore all subsequent messages (no Pong responses)
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    addr
}

/// Start a TCP server that echoes Pong for every Ping.
async fn spawn_pong_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, mut w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();

        // Handle Login → send Welcome + Announcement
        if let Ok(Some(line)) = lines.next_line().await {
            let _ = decode_client_line(&line);
            let welcome = encode_line(&ServerMessage::Welcome {
                client_id: uuid::Uuid::new_v4(),
                motd: "pong server".to_string(),
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();
            let announcement = encode_line(&ServerMessage::ResourceAnnouncement(
                protocol::ResourceAnnouncement {
                    resources: vec![],
                    signature: None,
                },
            ))
            .unwrap();
            let _ = w.write_all(welcome.as_bytes()).await;
            let _ = w.write_all(announcement.as_bytes()).await;
        }

        // Echo Pong for every Ping
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(ClientMessage::Ping { sequence }) = decode_client_line(&line) {
                let pong = encode_line(&ServerMessage::Pong { sequence }).unwrap();
                if w.write_all(pong.as_bytes()).await.is_err() {
                    break;
                }
            }
        }
    });

    addr
}

async fn login_and_consume_handshake(
    addr: std::net::SocketAddr,
) -> Result<(
    Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    Arc<tokio::sync::Mutex<tokio::io::Lines<tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>>>>,
)> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "enforcer".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    // Consume Welcome and ResourceAnnouncement
    let _ = lines.next_line().await?;
    let _ = lines.next_line().await?;

    let writer = Arc::new(tokio::sync::Mutex::new(writer_half));
    let lines_arc = Arc::new(tokio::sync::Mutex::new(lines));
    Ok((writer, lines_arc))
}

#[tokio::test]
async fn report_only_does_not_disconnect_on_missed_heartbeats() -> Result<()> {
    let addr = spawn_no_pong_server().await;
    let (writer, lines_arc) = login_and_consume_handshake(addr).await?;

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(client::heartbeat_loop(
        writer,
        lines_arc,
        Duration::from_millis(20),
        Duration::from_millis(5),
        stop_rx,
        client::ClientHeartbeatPolicy::ReportOnly,
    ));

    // Let enough intervals pass for 4+ timeouts to accumulate
    sleep(Duration::from_millis(200)).await;
    let _ = stop_tx.send(());

    let metrics = tokio::time::timeout(Duration::from_millis(300), handle).await??;

    // ReportOnly: loop ran until stop signal, never enforcement-disconnected
    assert!(!metrics.enforcement_disconnect);
    assert!(metrics.timeout_or_error_count >= client::CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD);
    assert_eq!(metrics.pong_count, 0);
    Ok(())
}

#[tokio::test]
async fn strict_disconnects_after_threshold_timeouts() -> Result<()> {
    let addr = spawn_no_pong_server().await;
    let (writer, lines_arc) = login_and_consume_handshake(addr).await?;

    let (_stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    // Note: _stop_tx is never sent — enforcement must break the loop

    let handle = tokio::spawn(client::heartbeat_loop(
        writer,
        lines_arc,
        Duration::from_millis(20),
        Duration::from_millis(5),
        stop_rx,
        client::ClientHeartbeatPolicy::Strict,
    ));

    let metrics = tokio::time::timeout(Duration::from_secs(5), handle).await??;

    // Strict: loop stopped itself at the threshold
    assert!(metrics.enforcement_disconnect);
    assert_eq!(
        metrics.timeout_or_error_count,
        client::CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD
    );
    assert_eq!(metrics.pong_count, 0);
    Ok(())
}

#[tokio::test]
async fn strict_stays_connected_when_heartbeats_are_healthy() -> Result<()> {
    let addr = spawn_pong_server().await;
    let (writer, lines_arc) = login_and_consume_handshake(addr).await?;

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(client::heartbeat_loop(
        writer,
        lines_arc,
        Duration::from_millis(20),
        Duration::from_millis(100),
        stop_rx,
        client::ClientHeartbeatPolicy::Strict,
    ));

    // Let several successful heartbeats complete
    sleep(Duration::from_millis(150)).await;
    let _ = stop_tx.send(());

    let metrics = tokio::time::timeout(Duration::from_millis(500), handle).await??;

    // Strict but healthy: no enforcement disconnect
    assert!(!metrics.enforcement_disconnect);
    assert!(metrics.pong_count >= 1);
    assert_eq!(metrics.timeout_or_error_count, 0);
    Ok(())
}

#[tokio::test]
async fn strict_enforcement_disconnect_sets_metrics_correctly() -> Result<()> {
    let addr = spawn_no_pong_server().await;
    let (writer, lines_arc) = login_and_consume_handshake(addr).await?;

    let (_stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let metrics = tokio::time::timeout(
        Duration::from_secs(5),
        client::heartbeat_loop(
            writer,
            lines_arc,
            Duration::from_millis(20),
            Duration::from_millis(5),
            stop_rx,
            client::ClientHeartbeatPolicy::Strict,
        ),
    )
    .await?;

    assert!(metrics.enforcement_disconnect);
    // sent_count >= timeout count (every sent ping timed out)
    assert!(metrics.sent_count >= metrics.timeout_or_error_count);
    // to_text() includes the enforcement note
    assert!(metrics.to_text().contains("enforcement_disconnect: true"));
    Ok(())
}
