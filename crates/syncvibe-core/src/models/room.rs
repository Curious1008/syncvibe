use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

pub const DEFAULT_RELAY_URL: &str = "wss://syncvibe-relay.business-a9e.workers.dev";
const INVITE_PREFIX: &str = "syncvibe://";

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
            relay_url: DEFAULT_RELAY_URL.to_string(),
        }
    }

    /// Encode room config as a shareable invite code: `syncvibe://BASE64`
    /// Format: base64(uuid_raw_16_bytes + secret_raw_32_bytes)
    pub fn to_invite_code(&self) -> Result<String, String> {
        let uuid = uuid::Uuid::parse_str(&self.room_id)
            .map_err(|e| format!("Invalid room_id UUID: {}", e))?;
        let uuid_bytes = uuid.as_bytes(); // 16 bytes
        let secret_bytes = hex_decode(&self.room_secret)
            .map_err(|e| format!("Invalid room_secret: {}", e))?;

        let mut payload = Vec::with_capacity(48);
        payload.extend_from_slice(uuid_bytes);
        payload.extend_from_slice(&secret_bytes);

        Ok(format!("{}{}", INVITE_PREFIX, URL_SAFE_NO_PAD.encode(&payload)))
    }

    /// Decode an invite code back into a RoomConfig (uses default relay URL)
    pub fn from_invite_code(code: &str) -> Result<Self, String> {
        let b64 = code
            .strip_prefix(INVITE_PREFIX)
            .ok_or("Invalid invite code: must start with syncvibe://")?;

        let payload = URL_SAFE_NO_PAD
            .decode(b64.trim())
            .map_err(|e| format!("Invalid invite code: {}", e))?;

        if payload.len() != 48 {
            return Err(format!(
                "Invalid invite code: expected 48 bytes, got {}",
                payload.len()
            ));
        }

        let uuid = uuid::Uuid::from_slice(&payload[..16])
            .map_err(|e| format!("Invalid UUID in invite code: {}", e))?;
        let secret = hex_encode(&payload[16..]);

        Ok(Self {
            room_id: uuid.to_string(),
            room_secret: secret,
            relay_url: DEFAULT_RELAY_URL.to_string(),
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("odd-length hex string".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| format!("invalid hex at position {}: {}", i, e))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_code_roundtrip() {
        let room = RoomConfig::new();
        let code = room.to_invite_code().unwrap();
        assert!(code.starts_with("syncvibe://"));

        let decoded = RoomConfig::from_invite_code(&code).unwrap();
        assert_eq!(decoded.room_id, room.room_id);
        assert_eq!(decoded.room_secret, room.room_secret);
        assert_eq!(decoded.relay_url, DEFAULT_RELAY_URL);
    }

    #[test]
    fn invite_code_rejects_bad_prefix() {
        assert!(RoomConfig::from_invite_code("bad://abc").is_err());
    }

    #[test]
    fn invite_code_rejects_bad_length() {
        let short = format!("syncvibe://{}", URL_SAFE_NO_PAD.encode(&[0u8; 10]));
        assert!(RoomConfig::from_invite_code(&short).is_err());
    }
}
