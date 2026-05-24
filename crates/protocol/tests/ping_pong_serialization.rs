use protocol::{decode_client_line, decode_server_line, encode_line};

#[test]
fn ping_serde_roundtrip() {
    let ping = protocol::ClientMessage::Ping { sequence: 42 };
    let line = encode_line(&ping).unwrap();
    let parsed = decode_client_line(&line).unwrap();
    match parsed {
        protocol::ClientMessage::Ping { sequence } => assert_eq!(sequence, 42),
        other => panic!("expected Ping, got {:?}", other),
    }
}

#[test]
fn pong_serde_roundtrip() {
    let pong = protocol::ServerMessage::Pong { sequence: 99 };
    let line = encode_line(&pong).unwrap();
    let parsed = decode_server_line(&line).unwrap();
    match parsed {
        protocol::ServerMessage::Pong { sequence } => assert_eq!(sequence, 99),
        other => panic!("expected Pong, got {:?}", other),
    }
}
