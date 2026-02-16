use std::time::Duration;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
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

impl WsClient {
    /// Connect to the relay and return a client + receiver for incoming messages
    pub async fn connect(
        relay_url: &str,
        room_id: &str,
        room_secret: &str,
        user_id: &str,
        user_name: &str,
        user_color: &str,
    ) -> Result<(Self, mpsc::Receiver<WsMessage>)> {
        let url = format!("{}/ws/{}", relay_url, room_id);
        let (ws_stream, _) = connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Channel for outgoing messages
        let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
        // Channel for incoming parsed messages
        let (in_tx, in_rx) = mpsc::channel::<WsMessage>(64);

        // Send auth message
        let auth = WsMessage::Auth {
            room_id: room_id.to_string(),
            room_secret: room_secret.to_string(),
            user_id: user_id.to_string(),
            user_name: user_name.to_string(),
            user_color: user_color.to_string(),
        };
        let auth_json = serde_json::to_string(&auth)?;
        write.send(Message::Text(auth_json.into())).await?;

        // Spawn ping task
        let ping_tx = out_tx.clone();
        let ping_handle: JoinHandle<()> = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let ping = serde_json::to_string(&WsMessage::Ping).unwrap_or_default();
                if ping_tx.send(ping).await.is_err() {
                    break;
                }
            }
        });

        // Spawn writer task — aborts ping task when writer exits
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if write.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            // Writer done (WS closed or error) — kill the ping task
            ping_handle.abort();
        });

        // Spawn reader task
        tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        if in_tx.send(ws_msg).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok((Self { tx: out_tx }, in_rx))
    }

    /// Send a WebSocket message to the relay
    pub async fn send(&self, msg: WsMessage) -> Result<()> {
        let json = serde_json::to_string(&msg)?;
        self.tx.send(json).await?;
        Ok(())
    }
}
