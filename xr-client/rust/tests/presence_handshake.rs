//! Integration tests for the `/ws/presence` server-initiated handshake through
//! the public `PresenceClient` API and the fake transport/signer ports.
//!
//! Flow (authoritative `src/handlers/presence_handler.rs`):
//!   server -> challenge -> client -> auth -> server -> joined.

use std::sync::Arc;

use visionclaw_xr_gdext::ports::fakes::{FakeSigner, FakeWsTransport};
use visionclaw_xr_gdext::ports::{Signer, WsMessage};
use visionclaw_xr_gdext::presence::{PresenceClient, PresenceError};
use visionclaw_xr_presence::wire::{decode, OPCODE_AVATAR_POSE};
use visionclaw_xr_presence::{PoseFrame, RoomId, Transform};

fn room() -> RoomId {
    RoomId::parse("urn:visionclaw:room:sha256-12-aaaaaaaaaaaa").unwrap()
}

fn challenge_json() -> String {
    serde_json::json!({
        "type": "challenge",
        "nonce": "00".repeat(32),
        "ts": 1_700_000_000_000_000u64
    })
    .to_string()
}

fn joined_json(avatar_hex: &str, members: serde_json::Value) -> String {
    serde_json::json!({
        "type": "joined",
        "room_id": room().as_str(),
        "avatar_id": format!("urn:visionclaw:avatar:{avatar_hex}"),
        "members": members
    })
    .to_string()
}

#[tokio::test]
async fn handshake_and_pose_round_trip_through_fake_transport() {
    let (transport, inbox) = FakeWsTransport::new();
    let transport = Arc::new(transport);
    let signer = Arc::new(FakeSigner::new());
    let avatar_hex = signer.pubkey_hex();
    let mut client = PresenceClient::new(transport.clone(), signer, room());

    let members = serde_json::json!([{
        "did": format!("did:nostr:{}", "c".repeat(64)),
        "display_name": "carol",
        "model_uri": null
    }]);
    inbox.send(WsMessage::Text(challenge_json())).await.unwrap();
    inbox
        .send(WsMessage::Text(joined_json(&avatar_hex, members)))
        .await
        .unwrap();

    let state = client.handshake("alice".into(), None).await.unwrap();
    assert_eq!(state.members.len(), 1);
    assert_eq!(state.members[0].display_name, "carol");

    // The auth reply carried the room and display name. Scope the guard so it
    // is released before the next await (no lock held across `.await`).
    {
        let sent_text = transport.sent_text.lock().unwrap();
        assert_eq!(sent_text.len(), 1);
        let auth: serde_json::Value = serde_json::from_str(&sent_text[0]).unwrap();
        assert_eq!(auth["type"], "auth");
        assert_eq!(auth["room_id"], room().as_str());
        assert_eq!(auth["metadata"]["display_name"], "alice");
    }

    let frame = PoseFrame {
        timestamp_us: 99_000,
        head: Transform {
            position: [0.5, 1.7, -0.2],
            rotation: [0.0, 0.0, 0.0, 1.0],
        },
        left_hand: Some(Transform::identity()),
        right_hand: Some(Transform::identity()),
    };
    client.send_pose(&frame).await.unwrap();

    let sent = transport.sent_binary.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0][0], OPCODE_AVATAR_POSE);

    let decoded = decode(&sent[0]).unwrap();
    assert_eq!(decoded.frame, frame);
}

#[tokio::test]
async fn server_close_during_init_yields_rejected() {
    let (transport, inbox) = FakeWsTransport::new();
    let transport = Arc::new(transport);
    let signer = Arc::new(FakeSigner::new());
    let mut client = PresenceClient::new(transport, signer, room());

    inbox.send(WsMessage::Close).await.unwrap();

    let err = client.handshake("alice".into(), None).await.unwrap_err();
    assert!(matches!(err, PresenceError::Rejected(_)));
}

#[tokio::test]
async fn send_pose_before_handshake_is_rejected() {
    let (transport, _inbox) = FakeWsTransport::new();
    let client = PresenceClient::new(Arc::new(transport), Arc::new(FakeSigner::new()), room());
    let frame = PoseFrame {
        timestamp_us: 1,
        head: Transform::identity(),
        left_hand: None,
        right_hand: None,
    };
    let err = client.send_pose(&frame).await.unwrap_err();
    assert!(matches!(err, PresenceError::Protocol(_)));
}

#[tokio::test]
async fn handshake_with_empty_members_succeeds() {
    let (transport, inbox) = FakeWsTransport::new();
    let transport = Arc::new(transport);
    let signer = Arc::new(FakeSigner::new());
    let avatar_hex = signer.pubkey_hex();
    let mut client = PresenceClient::new(transport.clone(), signer, room());

    inbox.send(WsMessage::Text(challenge_json())).await.unwrap();
    inbox
        .send(WsMessage::Text(joined_json(
            &avatar_hex,
            serde_json::json!([]),
        )))
        .await
        .unwrap();

    let state = client.handshake("alice".into(), None).await.unwrap();
    assert!(state.members.is_empty());
    assert!(client.avatar_id().is_some());
}
