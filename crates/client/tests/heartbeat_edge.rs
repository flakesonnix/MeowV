use anyhow::Result;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::Mutex as AsyncMutex, time::sleep};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use protocol::{decode_client_line, encode_line, ClientMessage, ServerMessage};

// Test that heartbeat sequence numbers increment monotonically.
#[tokio::test]
async fn heartbeat_loop_sequence_numbers_increment() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let seen = Arc::new(AsyncMutex::new(Vec::<u64>::new()));
    let seen_srv = seen.clone();

    // spawn test server
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let (r, mut w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();

        // expect Login
        if let Ok(Some(line)) = lines.next_line().await {
            let _ = decode_client_line(&line).expect("decode login");
            // send Welcome and Announcement
            let welcome = encode_line(&ServerMessage::Welcome { client_id: uuid::Uuid::new_v4(), motd: "ok".to_string(), protocol_version: 1 }).unwrap();
            let announcement = encode_line(&ServerMessage::ResourceAnnouncement(protocol::ResourceAnnouncement { resources: vec![], signature: None })).unwrap();
            let _ = w.write_all(welcome.as_bytes()).await;
            let _ = w.write_all(announcement.as_bytes()).await;
        }

        // read pings and echo pongs, record sequences
        while let Ok(Some(line)) = lines.next_line().await {
            match decode_client_line(&line).expect("decode client") {
                ClientMessage::Ping { sequence } => {
                    seen_srv.lock().await.push(sequence);
                    let pong = encode_line(&ServerMessage::Pong { sequence }).unwrap();
                    let _ = w.write_all(pong.as_bytes()).await;
                }
                _ => {}
            }
        }
    });

    // client side
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // send login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "cli".to_string(), protocol_version: 1 })?.as_bytes()).await?;

    // consume server welcome/announcement
    let _ = lines.next_line().await?;
    let _ = lines.next_line().await?;

    let writer = Arc::new(tokio::sync::Mutex::new(writer_half));
    let lines_arc = Arc::new(tokio::sync::Mutex::new(lines));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let hb_writer = writer.clone();
    let hb_lines = lines_arc.clone();
    tokio::spawn(async move {
        client::heartbeat_loop(hb_writer, hb_lines, tokio::time::Duration::from_millis(30), tokio::time::Duration::from_millis(20), stop_rx).await;
    });

    // let a few pings happen
    sleep(tokio::time::Duration::from_millis(220)).await;
    let _ = stop_tx.send(());

    // check seen sequences
    let seqs = seen.lock().await.clone();
    assert!(seqs.len() >= 3, "expected at least 3 pings, got {}", seqs.len());
    // ensure monotonic increment starting at 1
    for (i, s) in seqs.iter().enumerate() {
        assert_eq!(*s, (i as u64) + 1);
    }

    // cleanup
    server.abort();
    Ok(())
}

// Test that when server does not respond with pongs, the heartbeat loop continues sending pings (timeouts are non-fatal)
#[tokio::test]
async fn heartbeat_loop_timeout_continues_without_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let seen = Arc::new(AsyncMutex::new(Vec::<u64>::new()));
    let seen_srv = seen.clone();

    // spawn test server that records pings but does NOT reply
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let (r, _w) = socket.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();

        // expect Login and send welcome
        if let Ok(Some(line)) = lines.next_line().await {
            let _ = decode_client_line(&line).expect("decode login");
            // skip sending welcome/announcement — to be conservative send them so client proceeds
            // send Welcome and Announcement
            // (we will send, otherwise client might not send pings)
        }

        // read pings and record sequences, but don't reply
        while let Ok(Some(line)) = lines.next_line().await {
            match decode_client_line(&line).expect("decode client") {
                ClientMessage::Ping { sequence } => {
                    seen_srv.lock().await.push(sequence);
                    // do not send Pong
                }
                _ => {}
            }
        }
    });

    // client side
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // send login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "cli".to_string(), protocol_version: 1 })?.as_bytes()).await?;

    // don't rely on server welcome; proceed

    let writer = Arc::new(tokio::sync::Mutex::new(writer_half));
    let lines_arc = Arc::new(tokio::sync::Mutex::new(lines));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let hb_writer = writer.clone();
    let hb_lines = lines_arc.clone();
    tokio::spawn(async move {
        client::heartbeat_loop(hb_writer, hb_lines, tokio::time::Duration::from_millis(30), tokio::time::Duration::from_millis(10), stop_rx).await;
    });

    // let a few pings happen
    sleep(tokio::time::Duration::from_millis(220)).await;
    let _ = stop_tx.send(());

    // ensure multiple pings recorded despite no pongs
    let seqs = seen.lock().await.clone();
    assert!(seqs.len() >= 2, "expected at least 2 pings recorded, got {}", seqs.len());

    server.abort();
    Ok(())
}
