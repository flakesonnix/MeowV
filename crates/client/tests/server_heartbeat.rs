use anyhow::Result;
use protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_client_line, encode_line};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::Duration;

/// Connect to addr, send Login, discard Welcome + Announcement.
async fn connect_and_handshake(
    addr: std::net::SocketAddr,
) -> Result<(
    tokio::net::tcp::OwnedWriteHalf,
    tokio::io::Lines<tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>>,
)> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (r, mut w) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(r).lines();

    w.write_all(
        encode_line(&ClientMessage::Login {
            name: "test".to_string(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: protocol::current_login_capabilities(),
        })?
        .as_bytes(),
    )
    .await?;

    // Consume Welcome and Announcement
    let _ = lines.next_line().await?;
    let _ = lines.next_line().await?;

    Ok((w, lines))
}

// ─── handle_server_ping unit tests ────────────────────────────────────────────

#[tokio::test]
async fn server_ping_elicits_server_pong() -> Result<()> {
    // Standalone: create a loopback pair, call handle_server_ping, read back.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, _w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        let raw = lines.next_line().await.unwrap().unwrap();
        decode_client_line(&raw).unwrap()
    });

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (_, mut writer) = stream.into_split();

    client::handle_server_ping(&mut writer, 1).await?;

    let received = tokio::time::timeout(Duration::from_secs(2), server_task).await??;
    match received {
        ClientMessage::ServerPong { sequence } => assert_eq!(sequence, 1),
        other => panic!("expected ServerPong, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn multiple_server_pings_get_matching_pongs() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, _w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        let mut received = Vec::new();
        for _ in 0..3u32 {
            let raw = lines.next_line().await.unwrap().unwrap();
            received.push(decode_client_line(&raw).unwrap());
        }
        received
    });

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (_, mut writer) = stream.into_split();

    for seq in [1u64, 2, 3] {
        client::handle_server_ping(&mut writer, seq).await?;
    }

    let received = tokio::time::timeout(Duration::from_secs(2), server_task).await??;
    for (i, msg) in received.into_iter().enumerate() {
        let expected_seq = (i as u64) + 1;
        match msg {
            ClientMessage::ServerPong { sequence } => assert_eq!(sequence, expected_seq),
            other => panic!("expected ServerPong({expected_seq}), got {other:?}"),
        }
    }
    Ok(())
}

#[tokio::test]
async fn handle_server_ping_sequence_zero() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, _w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        let raw = lines.next_line().await.unwrap().unwrap();
        decode_client_line(&raw).unwrap()
    });

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (_, mut writer) = stream.into_split();
    client::handle_server_ping(&mut writer, 0).await?;

    let msg = tokio::time::timeout(Duration::from_secs(2), server_task).await??;
    assert!(matches!(msg, ClientMessage::ServerPong { sequence: 0 }));
    Ok(())
}

// ─── heartbeat context: ServerPing interleaved while waiting for Pong ─────────

/// Server that, upon receiving client Ping(1), sends ServerPing(99) before
/// the Pong(1). Asserts the client replies ServerPong(99) then gets Pong(1).
#[tokio::test]
async fn server_ping_handled_while_waiting_for_heartbeat_pong() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, mut w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();

        // Login
        let _ = lines.next_line().await;
        let welcome = encode_line(&ServerMessage::Welcome {
            client_id: uuid::Uuid::new_v4(),
            motd: "interleave_test".to_string(),
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

        // Wait for client Ping(1)
        let ping_line = lines.next_line().await.unwrap().unwrap();
        let client_msg = decode_client_line(&ping_line).unwrap();
        assert!(
            matches!(client_msg, ClientMessage::Ping { sequence: 1 }),
            "expected Ping(1), got {client_msg:?}"
        );

        // Send ServerPing(99) before responding with Pong(1)
        let server_ping = encode_line(&ServerMessage::ServerPing { sequence: 99 }).unwrap();
        w.write_all(server_ping.as_bytes()).await.unwrap();

        // Expect client to reply with ServerPong(99)
        let reply_line = lines.next_line().await.unwrap().unwrap();
        let reply = decode_client_line(&reply_line).unwrap();
        assert!(
            matches!(reply, ClientMessage::ServerPong { sequence: 99 }),
            "expected ServerPong(99), got {reply:?}"
        );

        // Now send the actual Pong(1)
        let pong = encode_line(&ServerMessage::Pong { sequence: 1 }).unwrap();
        w.write_all(pong.as_bytes()).await.unwrap();
    });

    let (mut writer, mut lines) = connect_and_handshake(addr).await?;

    // Send Ping(1) and wait — this goes through send_ping_and_wait_with_timeout
    // which will handle the interleaved ServerPing(99) transparently
    client::heartbeat::send_ping_and_wait_with_timeout(
        &mut writer,
        &mut lines,
        1,
        Duration::from_secs(2),
    )
    .await?;

    tokio::time::timeout(Duration::from_secs(2), server_task).await??;
    Ok(())
}

/// Receiving a ServerPing does not disrupt unrelated server messages.
#[tokio::test]
async fn unrelated_server_messages_not_disrupted_by_server_ping() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (r, mut w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();

        // Login
        let _ = lines.next_line().await;
        let welcome = encode_line(&ServerMessage::Welcome {
            client_id: uuid::Uuid::new_v4(),
            motd: "unrelated_test".to_string(),
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
        w.write_all(welcome.as_bytes()).await.unwrap();
        w.write_all(announcement.as_bytes()).await.unwrap();

        // Wait for Ping(1)
        let _ = lines.next_line().await.unwrap().unwrap();

        // Send ServerPing(7) before Pong
        let sp = encode_line(&ServerMessage::ServerPing { sequence: 7 }).unwrap();
        w.write_all(sp.as_bytes()).await.unwrap();

        // Client replies ServerPong(7)
        let _ = lines.next_line().await.unwrap().unwrap();

        // Now send Pong(1)
        let pong = encode_line(&ServerMessage::Pong { sequence: 1 }).unwrap();
        w.write_all(pong.as_bytes()).await.unwrap();
    });

    let (mut writer, mut lines) = connect_and_handshake(addr).await?;

    // This must complete successfully — ServerPing didn't break the wait
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client::heartbeat::send_ping_and_wait_with_timeout(
            &mut writer,
            &mut lines,
            1,
            Duration::from_secs(2),
        ),
    )
    .await?;
    assert!(result.is_ok());
    Ok(())
}
