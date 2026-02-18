use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use syncvibe_core::protocol::WsMessage;

/// WebSocket client that connects to the SyncVibe relay
#[derive(Clone)]
pub struct WsClient {
    tx: mpsc::Sender<String>,
}

/// Connect to relay, returning (client, message_rx, alive_rx).
/// `alive_rx` emits `false` when the connection drops.
pub async fn connect_ws(
    relay_url: &str,
    room_id: &str,
    room_secret: &str,
    user_id: &str,
    user_name: &str,
    user_color: &str,
    agent_id: Option<String>,
) -> Result<(WsClient, mpsc::Receiver<WsMessage>, watch::Receiver<bool>)> {
    // Enforce TLS to protect room_secret in transit
    if !relay_url.starts_with("wss://") {
        return Err(anyhow::anyhow!(
            "Relay URL must use wss:// (got {})",
            relay_url
        ));
    }
    // Percent-encode room_id to prevent URL injection
    let encoded_room_id: String = room_id
        .bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                vec![b as char]
            } else {
                format!("%{:02X}", b).chars().collect()
            }
        })
        .collect();
    let url = format!("{}/ws/{}", relay_url, encoded_room_id);
    let (ws_stream, _) = time::timeout(Duration::from_secs(5), connect_async(&url))
        .await
        .map_err(|_| anyhow::anyhow!("Connection timed out"))??;
    let (mut write, mut read) = ws_stream.split();

    // Channel for outgoing messages
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    // Channel for incoming parsed messages
    let (in_tx, in_rx) = mpsc::channel::<WsMessage>(64);
    // Watch channel for connection alive status
    let (alive_tx, alive_rx) = watch::channel(true);
    let alive_tx = Arc::new(alive_tx);

    // Send auth message
    let auth = WsMessage::Auth {
        room_id: room_id.to_string(),
        room_secret: room_secret.to_string(),
        user_id: user_id.to_string(),
        user_name: user_name.to_string(),
        user_color: user_color.to_string(),
        agent_id,
    };
    let auth_json = serde_json::to_string(&auth)?;
    write.send(Message::Text(auth_json.into())).await?;

    // Spawn ping task
    let ping_tx = out_tx.clone();
    let ping_handle: JoinHandle<()> = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(25));
        loop {
            interval.tick().await;
            let ping = serde_json::to_string(&WsMessage::Ping).unwrap_or_default();
            if ping_tx.send(ping).await.is_err() {
                break;
            }
        }
    });

    // Spawn writer task — aborts ping task when writer exits
    let writer_alive_tx = alive_tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if write.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
        // Writer done (WS closed or error) — kill the ping task, signal offline
        ping_handle.abort();
        let _ = writer_alive_tx.send(false);
    });

    // Spawn reader task
    tokio::spawn(async move {
        const MAX_MSG_SIZE: usize = 1_048_576; // 1 MB limit per message
        while let Some(Ok(msg)) = read.next().await {
            if let Message::Text(text) = msg {
                if text.len() > MAX_MSG_SIZE {
                    continue; // Drop oversized messages
                }
                if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                    if in_tx.send(ws_msg).await.is_err() {
                        break;
                    }
                }
            }
        }
        // Reader done — signal offline
        let _ = alive_tx.send(false);
    });

    Ok((WsClient { tx: out_tx }, in_rx, alive_rx))
}

impl WsClient {
    /// Send a WebSocket message to the relay
    pub async fn send(&self, msg: WsMessage) -> Result<()> {
        let json = serde_json::to_string(&msg)?;
        self.tx.send(json).await?;
        Ok(())
    }
}
