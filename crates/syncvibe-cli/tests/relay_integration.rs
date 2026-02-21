//! Integration tests for the WebSocket relay.
//! These tests connect to the real deployed relay at syncvibe-relay.business-a9e.workers.dev.
//! Run with: cargo test --test relay_integration -- --nocapture

use std::time::Duration;
use tokio::time;

// We test using raw tokio-tungstenite since WsClient is in the binary crate.
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const RELAY_URL: &str = "wss://relay.syncvibe.online";

fn new_room_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn new_secret() -> String {
    use rand::Rng;
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Helper: connect and authenticate a client, returning (write, read) split streams
async fn connect_and_auth(
    room_id: &str,
    room_secret: &str,
    user_id: &str,
    user_name: &str,
    user_color: &str,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let url = format!("{}/ws/{}", RELAY_URL, room_id);
    let (ws, _) = connect_async(&url)
        .await
        .expect("Failed to connect to relay");
    let (mut write, read) = ws.split();

    let auth = serde_json::json!({
        "type": "auth",
        "data": {
            "room_id": room_id,
            "room_secret": room_secret,
            "user_id": user_id,
            "user_name": user_name,
            "user_color": user_color,
        }
    });
    write
        .send(Message::Text(auth.to_string()))
        .await
        .expect("Failed to send auth");

    (write, read)
}

/// Read next text message with timeout
async fn recv_text(
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    timeout_secs: u64,
) -> Option<serde_json::Value> {
    let result = time::timeout(Duration::from_secs(timeout_secs), async {
        while let Some(Ok(msg)) = read.next().await {
            if let Message::Text(text) = msg {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    return Some(val);
                }
            }
        }
        None
    })
    .await;
    result.unwrap_or(None)
}

#[tokio::test]
async fn test_auth_and_presence() {
    let room_id = new_room_id();
    let secret = new_secret();

    // Client A connects
    let (mut _write_a, mut read_a) =
        connect_and_auth(&room_id, &secret, "user-a", "Alice", "#ff0000").await;

    // Should receive auth_ok
    let msg = recv_text(&mut read_a, 5)
        .await
        .expect("No auth_ok for Alice");
    assert_eq!(msg["type"], "auth_ok");

    // Client B connects
    let (_write_b, mut read_b) =
        connect_and_auth(&room_id, &secret, "user-b", "Bob", "#00ff00").await;

    // B should receive auth_ok with Alice in presence
    let msg = recv_text(&mut read_b, 5).await.expect("No auth_ok for Bob");
    assert_eq!(msg["type"], "auth_ok");
    let users = msg["data"]["users"].as_array().unwrap();
    assert!(
        users.iter().any(|u| u["user_id"] == "user-a"),
        "Alice should be in presence list"
    );

    // A should receive user_joined for Bob
    let msg = recv_text(&mut read_a, 5)
        .await
        .expect("Alice didn't receive user_joined");
    assert_eq!(msg["type"], "user_joined");
    assert_eq!(msg["data"]["user_id"], "user-b");
    assert_eq!(msg["data"]["user_name"], "Bob");
}

#[tokio::test]
async fn test_chat_message_broadcast() {
    let room_id = new_room_id();
    let secret = new_secret();

    // Two clients
    let (mut write_a, mut read_a) =
        connect_and_auth(&room_id, &secret, "user-a", "Alice", "#ff0000").await;
    let _ = recv_text(&mut read_a, 5).await; // auth_ok

    let (_write_b, mut read_b) =
        connect_and_auth(&room_id, &secret, "user-b", "Bob", "#00ff00").await;
    let _ = recv_text(&mut read_b, 5).await; // auth_ok
    let _ = recv_text(&mut read_a, 5).await; // user_joined

    // A sends a chat message
    let chat_msg = serde_json::json!({
        "type": "chat_message",
        "data": {
            "id": uuid::Uuid::new_v4().to_string(),
            "user_id": "user-a",
            "user_name": "Alice",
            "user_color": "#ff0000",
            "content": "Hello from Alice!",
            "message_type": "user",
            "thread_id": null,
            "session_id": "test-session",
            "timestamp": "2026-02-16T00:00:00Z"
        }
    });
    write_a
        .send(Message::Text(chat_msg.to_string()))
        .await
        .unwrap();

    // B should receive the chat message
    let msg = recv_text(&mut read_b, 5)
        .await
        .expect("Bob didn't receive chat message");
    assert_eq!(msg["type"], "chat_message");
    assert_eq!(msg["data"]["content"], "Hello from Alice!");
    assert_eq!(msg["data"]["user_name"], "Alice");
}

#[tokio::test]
async fn test_wrong_secret_rejected() {
    let room_id = new_room_id();
    let secret = new_secret();

    // First client sets the secret
    let (_write_a, mut read_a) =
        connect_and_auth(&room_id, &secret, "user-a", "Alice", "#ff0000").await;
    let msg = recv_text(&mut read_a, 5).await.expect("No auth_ok");
    assert_eq!(msg["type"], "auth_ok");

    // Second client tries with wrong secret
    let (_write_bad, mut read_bad) =
        connect_and_auth(&room_id, "wrong-secret", "user-bad", "Eve", "#000000").await;
    let msg = recv_text(&mut read_bad, 5).await.expect("No auth_fail");
    assert_eq!(msg["type"], "auth_fail");
    assert_eq!(msg["data"]["reason"], "Invalid room secret");
}

#[tokio::test]
async fn test_user_left_notification() {
    let room_id = new_room_id();
    let secret = new_secret();

    // Two clients
    let (_write_a, mut read_a) =
        connect_and_auth(&room_id, &secret, "user-a", "Alice", "#ff0000").await;
    let _ = recv_text(&mut read_a, 5).await; // auth_ok

    let (write_b, mut read_b) =
        connect_and_auth(&room_id, &secret, "user-b", "Bob", "#00ff00").await;
    let _ = recv_text(&mut read_b, 5).await; // auth_ok
    let _ = recv_text(&mut read_a, 5).await; // user_joined

    // B disconnects
    drop(write_b);
    drop(read_b);

    // A should get user_left
    let msg = recv_text(&mut read_a, 5)
        .await
        .expect("Alice didn't receive user_left");
    assert_eq!(msg["type"], "user_left");
    assert_eq!(msg["data"]["user_id"], "user-b");
}

#[tokio::test]
async fn test_ping_pong() {
    let room_id = new_room_id();
    let secret = new_secret();

    let (mut write_a, mut read_a) =
        connect_and_auth(&room_id, &secret, "user-a", "Alice", "#ff0000").await;
    let _ = recv_text(&mut read_a, 5).await; // auth_ok

    // Send ping
    let ping = serde_json::json!({"type": "ping"});
    write_a.send(Message::Text(ping.to_string())).await.unwrap();

    // Should receive pong
    let msg = recv_text(&mut read_a, 5).await.expect("No pong received");
    assert_eq!(msg["type"], "pong");
}

#[tokio::test]
async fn test_three_users_broadcast() {
    let room_id = new_room_id();
    let secret = new_secret();

    // Three clients connect
    let (mut write_a, mut read_a) =
        connect_and_auth(&room_id, &secret, "user-a", "Alice", "#ff0000").await;
    let _ = recv_text(&mut read_a, 5).await; // auth_ok

    let (_write_b, mut read_b) =
        connect_and_auth(&room_id, &secret, "user-b", "Bob", "#00ff00").await;
    let _ = recv_text(&mut read_b, 5).await; // auth_ok
    let _ = recv_text(&mut read_a, 5).await; // user_joined(B)

    let (_write_c, mut read_c) =
        connect_and_auth(&room_id, &secret, "user-c", "Charlie", "#0000ff").await;
    let _ = recv_text(&mut read_c, 5).await; // auth_ok
    let _ = recv_text(&mut read_a, 5).await; // user_joined(C)
    let _ = recv_text(&mut read_b, 5).await; // user_joined(C)

    // A sends a message
    let chat_msg = serde_json::json!({
        "type": "chat_message",
        "data": {
            "id": uuid::Uuid::new_v4().to_string(),
            "user_id": "user-a",
            "user_name": "Alice",
            "user_color": "#ff0000",
            "content": "Hello everyone!",
            "message_type": "user",
            "thread_id": null,
            "session_id": "test-session",
            "timestamp": "2026-02-16T00:00:00Z"
        }
    });
    write_a
        .send(Message::Text(chat_msg.to_string()))
        .await
        .unwrap();

    // Both B and C should receive it
    let msg_b = recv_text(&mut read_b, 5)
        .await
        .expect("Bob didn't receive message");
    assert_eq!(msg_b["data"]["content"], "Hello everyone!");

    let msg_c = recv_text(&mut read_c, 5)
        .await
        .expect("Charlie didn't receive message");
    assert_eq!(msg_c["data"]["content"], "Hello everyone!");
}
