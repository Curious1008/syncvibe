use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomConfig {
    pub room_id: String,
    pub room_secret: String,
    pub relay_url: String,
}

impl RoomConfig {
    pub fn new() -> Self {
        let mut secret_bytes = [0u8; 32];
        getrandom::getrandom(&mut secret_bytes).expect("failed to generate random bytes");
        Self {
            room_id: uuid::Uuid::new_v4().to_string(),
            room_secret: hex_encode(&secret_bytes),
            relay_url: "wss://syncvibe-relay.workers.dev".to_string(),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
