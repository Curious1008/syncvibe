use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

pub const DEFAULT_RELAY_URL: &str = "wss://relay.syncvibe.online";
const INVITE_PREFIX: &str = "syncvibe://";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomConfig {
    pub room_id: String,
    pub room_secret: String,
    pub relay_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

impl Default for RoomConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl RoomConfig {
    pub fn new() -> Self {
        let mut secret_bytes = [0u8; 32];
        // getrandom only fails in very unusual environments (no OS entropy source)
        if getrandom::getrandom(&mut secret_bytes).is_err() {
            // Fallback: use uuid bytes repeated (still random via uuid v4)
            let uuid_bytes = *uuid::Uuid::new_v4().as_bytes();
            secret_bytes[..16].copy_from_slice(&uuid_bytes);
            secret_bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        }
        Self {
            room_id: uuid::Uuid::new_v4().to_string(),
            room_secret: hex_encode(&secret_bytes),
            relay_url: DEFAULT_RELAY_URL.to_string(),
            room_name: None,
            agent: None,
        }
    }

    /// Encode room config as a shareable invite code: `syncvibe://BASE64`
    /// Format: base64(uuid_raw_16_bytes + secret_raw_32_bytes)
    pub fn to_invite_code(&self) -> Result<String, String> {
        let uuid = uuid::Uuid::parse_str(&self.room_id)
            .map_err(|e| format!("Invalid room_id UUID: {}", e))?;
        let uuid_bytes = uuid.as_bytes(); // 16 bytes
        let secret_bytes =
            hex_decode(&self.room_secret).map_err(|e| format!("Invalid room_secret: {}", e))?;

        let mut payload = Vec::with_capacity(48 + 64);
        payload.extend_from_slice(uuid_bytes);
        payload.extend_from_slice(&secret_bytes);

        // Append room name (truncated to 64 bytes) after the 48-byte header
        // Truncate on a valid UTF-8 boundary to avoid splitting multi-byte chars
        if let Some(ref name) = self.room_name {
            let mut len = name.len().min(64);
            while len > 0 && !name.is_char_boundary(len) {
                len -= 1;
            }
            payload.extend_from_slice(&name.as_bytes()[..len]);
        }

        Ok(format!(
            "{}{}",
            INVITE_PREFIX,
            URL_SAFE_NO_PAD.encode(&payload)
        ))
    }

    /// Decode an invite code back into a RoomConfig (uses default relay URL)
    pub fn from_invite_code(code: &str) -> Result<Self, String> {
        let b64 = code
            .strip_prefix(INVITE_PREFIX)
            .ok_or("Invalid invite code: must start with syncvibe://")?;

        let payload = URL_SAFE_NO_PAD
            .decode(b64.trim())
            .map_err(|e| format!("Invalid invite code: {}", e))?;

        if payload.len() < 48 {
            return Err(format!(
                "Invalid invite code: expected >= 48 bytes, got {}",
                payload.len()
            ));
        }

        let uuid = uuid::Uuid::from_slice(&payload[..16])
            .map_err(|e| format!("Invalid UUID in invite code: {}", e))?;
        let secret = hex_encode(&payload[16..48]);

        // Extract room name from bytes after the 48-byte header.
        // Sanitize to prevent path traversal: strip null bytes, path separators,
        // and ".." components since room_name is used to create directories.
        let room_name = if payload.len() > 48 {
            String::from_utf8(payload[48..].to_vec())
                .ok()
                .map(|s| sanitize_room_name(&s))
                .filter(|s| !s.is_empty())
        } else {
            None
        };

        Ok(Self {
            room_id: uuid.to_string(),
            room_secret: secret,
            relay_url: DEFAULT_RELAY_URL.to_string(),
            room_name,
            agent: None,
        })
    }
}

/// Sanitize a room name decoded from an invite code to prevent path traversal.
/// Removes null bytes, path separators (`/`, `\`), and rejects names that are
/// or contain `..` components. Returns an empty string if the name is unsafe.
fn sanitize_room_name(name: &str) -> String {
    // Remove null bytes and path separators
    let cleaned: String = name
        .chars()
        .filter(|c| *c != '\0' && *c != '/' && *c != '\\')
        .collect();
    // Reject if the cleaned name is "." or ".." or contains ".." as a component
    let trimmed = cleaned.trim();
    if trimmed == "." || trimmed == ".." || trimmed.contains("..") {
        return String::new();
    }
    trimmed.to_string()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.is_ascii() {
        return Err("hex string contains non-ASCII characters".to_string());
    }
    if !hex.len().is_multiple_of(2) {
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
        let short = format!("syncvibe://{}", URL_SAFE_NO_PAD.encode([0u8; 10]));
        assert!(RoomConfig::from_invite_code(&short).is_err());
    }

    #[test]
    fn invite_code_roundtrip_with_name() {
        let mut room = RoomConfig::new();
        room.room_name = Some("my-project".to_string());
        let code = room.to_invite_code().unwrap();

        let decoded = RoomConfig::from_invite_code(&code).unwrap();
        assert_eq!(decoded.room_id, room.room_id);
        assert_eq!(decoded.room_secret, room.room_secret);
        assert_eq!(decoded.room_name, Some("my-project".to_string()));
    }

    #[test]
    fn invite_code_without_name_compat() {
        // Old codes without name should still work
        let room = RoomConfig::new();
        // Manually create a 48-byte-only code (no name)
        let uuid = uuid::Uuid::parse_str(&room.room_id).unwrap();
        let secret_bytes = hex_decode(&room.room_secret).unwrap();
        let mut payload = Vec::with_capacity(48);
        payload.extend_from_slice(uuid.as_bytes());
        payload.extend_from_slice(&secret_bytes);
        let code = format!("syncvibe://{}", URL_SAFE_NO_PAD.encode(&payload));

        let decoded = RoomConfig::from_invite_code(&code).unwrap();
        assert_eq!(decoded.room_id, room.room_id);
        assert_eq!(decoded.room_secret, room.room_secret);
        assert_eq!(decoded.room_name, None);
    }
}
