use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: u32,
    pub owner_id: Uuid,
    pub position: Position,
    pub tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAnnouncement {
    pub resources: Vec<AnnouncedResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnouncedResource {
    pub name: String,
    pub version: String,
    pub files: Vec<AnnouncedResourceFile>,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnouncedResourceFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAvailabilityStatus {
    Available,
    Missing,
    SizeMismatch,
    HashMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAvailabilityReport {
    pub resources: Vec<ResourceAvailabilityEntry>,
    pub is_fully_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAvailabilityEntry {
    pub resource_name: String,
    pub file_path: String,
    pub status: ResourceAvailabilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Login { name: String, protocol_version: u32 },
    Chat { message: String },
    ResourceAvailabilityReport(ResourceAvailabilityReport),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectReason {
    ProtocolMismatch,
    InvalidHandshake,
    ClientClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        client_id: Uuid,
        motd: String,
        protocol_version: u32,
    },
    ChatBroadcast {
        from: String,
        message: String,
    },
    EntitySnapshot {
        entities: Vec<EntityState>,
    },
    ResourceAnnouncement(ResourceAnnouncement),
    Disconnect {
        reason: DisconnectReason,
        message: String,
    },
    Error {
        message: String,
    },
}

pub fn encode_line<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

pub fn decode_client_line(line: &str) -> Result<ClientMessage, serde_json::Error> {
    serde_json::from_str(line)
}

pub fn decode_server_line(line: &str) -> Result<ServerMessage, serde_json::Error> {
    serde_json::from_str(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize_resource_announcement() {
        let announcement = ResourceAnnouncement {
            resources: vec![AnnouncedResource {
                name: "chat".to_string(),
                version: "0.1.0".to_string(),
                files: vec![AnnouncedResourceFile {
                    relative_path: "resource.toml".to_string(),
                    size_bytes: 123,
                    sha256: "abc".to_string(),
                }],
                protocol_version: PROTOCOL_VERSION,
            }],
        };

        let line = encode_line(&ServerMessage::ResourceAnnouncement(announcement.clone())).unwrap();
        let decoded = decode_server_line(line.trim()).unwrap();
        match decoded {
            ServerMessage::ResourceAnnouncement(actual) => assert_eq!(actual, announcement),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn serialize_deserialize_resource_availability_report() {
        let report = ResourceAvailabilityReport {
            resources: vec![ResourceAvailabilityEntry {
                resource_name: "chat".to_string(),
                file_path: "resource.toml".to_string(),
                status: ResourceAvailabilityStatus::Available,
            }],
            is_fully_available: true,
        };

        let line = encode_line(&ClientMessage::ResourceAvailabilityReport(report.clone())).unwrap();
        let decoded = decode_client_line(line.trim()).unwrap();
        match decoded {
            ClientMessage::ResourceAvailabilityReport(actual) => assert_eq!(actual, report),
            other => panic!("unexpected packet: {other:?}"),
        }
    }

    #[test]
    fn protocol_messages_include_resource_types() {
        let server =
            ServerMessage::ResourceAnnouncement(ResourceAnnouncement { resources: vec![] });
        let client = ClientMessage::ResourceAvailabilityReport(ResourceAvailabilityReport {
            resources: vec![],
            is_fully_available: true,
        });

        assert!(matches!(server, ServerMessage::ResourceAnnouncement(_)));
        assert!(matches!(
            client,
            ClientMessage::ResourceAvailabilityReport(_)
        ));
    }
}
