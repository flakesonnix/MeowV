use anyhow::Result;
use protocol::{
    decode_server_line, encode_line, ClientMessage, DisconnectReason, ServerMessage,
    PROTOCOL_VERSION,
};
use server::{run_with_listener, ServerConfig};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{timeout, Duration},
};

#[tokio::test]
async fn login_chat_and_snapshot_flow() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server_task = tokio::spawn(run_with_listener(
        listener,
        ServerConfig {
            bind: addr.to_string(),
            tick_rate: 20,
            motd: "test motd".to_string(),
        },
    ));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION,
            })?
            .as_bytes(),
        )
        .await?;
    writer_half
        .write_all(
            encode_line(&ClientMessage::Chat {
                message: "hello".to_string(),
            })?
            .as_bytes(),
        )
        .await?;

    let welcome = read_packet(&mut lines).await?;
    match welcome {
        ServerMessage::Welcome {
            motd,
            protocol_version,
            ..
        } => {
            assert_eq!(motd, "test motd");
            assert_eq!(protocol_version, PROTOCOL_VERSION);
        }
        other => panic!("expected welcome, got {other:?}"),
    }

    let joined = read_packet(&mut lines).await?;
    match joined {
        ServerMessage::ChatBroadcast { from, message } => {
            assert_eq!(from, "server");
            assert_eq!(message, "alice joined");
        }
        other => panic!("expected join broadcast, got {other:?}"),
    }

    let chat = read_packet(&mut lines).await?;
    match chat {
        ServerMessage::ChatBroadcast { from, message } => {
            assert_eq!(from, "alice");
            assert_eq!(message, "hello");
        }
        other => panic!("expected chat broadcast, got {other:?}"),
    }

    let snapshot = read_packet(&mut lines).await?;
    match snapshot {
        ServerMessage::EntitySnapshot { entities } => {
            assert!(!entities.is_empty());
            assert_eq!(entities[0].tick, 1);
        }
        other => panic!("expected entity snapshot, got {other:?}"),
    }

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn rejects_protocol_mismatch() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server_task = tokio::spawn(run_with_listener(
        listener,
        ServerConfig {
            bind: addr.to_string(),
            tick_rate: 20,
            motd: "test motd".to_string(),
        },
    ));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION + 1,
            })?
            .as_bytes(),
        )
        .await?;

    let disconnect = read_packet(&mut lines).await?;
    match disconnect {
        ServerMessage::Disconnect { reason, message } => {
            assert_eq!(reason, DisconnectReason::ProtocolMismatch);
            assert!(message.contains("protocol mismatch"));
        }
        other => panic!("expected disconnect, got {other:?}"),
    }

    server_task.abort();
    Ok(())
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
