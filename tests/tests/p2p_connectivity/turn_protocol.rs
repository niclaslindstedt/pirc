//! TURN protocol integration tests.
//!
//! Exercises TURN message encoding/decoding, Send/Data Indication flows,
//! channel data framing, credential handling, and message integrity
//! computation.

use std::net::SocketAddr;

use pirc_p2p::stun::TransactionId;
use pirc_p2p::turn::{
    compute_long_term_key, decode_channel_data, encode_channel_data, parse_data_indication,
    send_to_peer, TurnAttribute, TurnCredentials, TurnMessage,
};
use tokio::net::UdpSocket;

// --- Message encoding / decoding ---

#[tokio::test]
async fn turn_allocate_request_roundtrip() {
    let msg = TurnMessage::allocate_request();
    let bytes = msg.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.msg_type, msg.msg_type);
    assert_eq!(parsed.transaction_id, msg.transaction_id);
}

#[tokio::test]
async fn turn_allocate_request_with_credentials_roundtrip() {
    let creds = TurnCredentials {
        username: "user".into(),
        password: "pass".into(),
        realm: "realm".into(),
        nonce: "nonce123".into(),
    };
    let msg = TurnMessage::allocate_request_with_credentials(&creds);
    let bytes = msg.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.msg_type, msg.msg_type);

    // Check username attribute exists
    let has_username = parsed
        .attributes
        .iter()
        .any(|a| matches!(a, TurnAttribute::Username(u) if u == "user"));
    assert!(has_username);
}

#[tokio::test]
async fn turn_send_indication_roundtrip() {
    let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
    let data = b"hello relay";
    let msg = TurnMessage::send_indication(peer, data.to_vec());
    let bytes = msg.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_send_indication());
    assert_eq!(parsed.data().unwrap(), data);
    assert_eq!(parsed.peer_address().unwrap(), peer);
}

#[tokio::test]
async fn turn_data_indication_roundtrip() {
    let peer: SocketAddr = "10.0.0.2:8000".parse().unwrap();
    let payload = b"data from peer";

    let indication = TurnMessage {
        msg_type: 0x0017, // DATA_INDICATION
        transaction_id: TransactionId::random(),
        attributes: vec![
            TurnAttribute::XorPeerAddress(peer),
            TurnAttribute::Data(payload.to_vec()),
        ],
    };

    let bytes = indication.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_data_indication());
    assert_eq!(parsed.data().unwrap(), payload);
    assert_eq!(parsed.peer_address().unwrap(), peer);
}

#[tokio::test]
async fn turn_create_permission_request_roundtrip() {
    let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
    let creds = TurnCredentials {
        username: "alice".into(),
        password: "secret".into(),
        realm: "example.com".into(),
        nonce: "abc123".into(),
    };

    let msg = TurnMessage::create_permission_request(peer, &creds);
    let bytes = msg.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.msg_type, msg.msg_type);
    assert_eq!(parsed.peer_address().unwrap(), peer);
}

#[tokio::test]
async fn turn_refresh_request_roundtrip() {
    let creds = TurnCredentials {
        username: "alice".into(),
        password: "secret".into(),
        realm: "example.com".into(),
        nonce: "abc123".into(),
    };

    let msg = TurnMessage::refresh_request(600, &creds);
    let bytes = msg.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.lifetime().unwrap(), 600);
}

// --- Channel data framing ---

#[tokio::test]
async fn channel_data_encode_decode_roundtrip() {
    let channel: u16 = 0x4000;
    let data = b"channel payload";
    let encoded = encode_channel_data(channel, data);
    let (decoded_channel, decoded_data) = decode_channel_data(&encoded).unwrap();

    assert_eq!(decoded_channel, channel);
    assert_eq!(decoded_data, data);
}

#[tokio::test]
async fn channel_data_various_channels() {
    for channel in [0x4000u16, 0x4001, 0x5000, 0x7FFF] {
        let data = format!("channel {channel}");
        let encoded = encode_channel_data(channel, data.as_bytes());
        let (decoded_ch, decoded_data) = decode_channel_data(&encoded).unwrap();
        assert_eq!(decoded_ch, channel);
        assert_eq!(decoded_data, data.as_bytes());
    }
}

#[tokio::test]
async fn channel_data_preserves_binary() {
    let channel: u16 = 0x4000;
    let data: Vec<u8> = (0..=255).collect();
    let encoded = encode_channel_data(channel, &data);
    let (_, decoded) = decode_channel_data(&encoded).unwrap();
    assert_eq!(decoded, data);
}

// --- Data indication parsing ---

#[tokio::test]
async fn parse_data_indication_extracts_peer_and_payload() {
    let peer: SocketAddr = "10.0.0.3:7777".parse().unwrap();
    let payload = b"relayed data";

    let indication = TurnMessage {
        msg_type: 0x0017,
        transaction_id: TransactionId::random(),
        attributes: vec![
            TurnAttribute::XorPeerAddress(peer),
            TurnAttribute::Data(payload.to_vec()),
        ],
    };

    let bytes = indication.to_bytes(None);
    let (parsed_peer, parsed_data) = parse_data_indication(&bytes).unwrap();

    assert_eq!(parsed_peer, peer);
    assert_eq!(parsed_data, payload);
}

// --- Send indication over loopback ---

#[tokio::test]
async fn send_to_peer_creates_send_indication() {
    let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server_sock.local_addr().unwrap();
    let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
    let data = b"relay this";

    send_to_peer(&client_sock, server_addr, peer, data.to_vec())
        .await
        .unwrap();

    let mut buf = [0u8; 4096];
    let (n, _src) = server_sock.recv_from(&mut buf).await.unwrap();

    let msg = TurnMessage::from_bytes(&buf[..n]).unwrap();
    assert!(msg.is_send_indication());
    assert_eq!(msg.peer_address().unwrap(), peer);
    assert_eq!(msg.data().unwrap(), data);
}

// --- Credential / long-term key computation ---

#[tokio::test]
async fn long_term_key_deterministic() {
    let key1 = compute_long_term_key("alice", "example.com", "password");
    let key2 = compute_long_term_key("alice", "example.com", "password");
    assert_eq!(key1, key2);
}

#[tokio::test]
async fn long_term_key_differs_by_username() {
    let key1 = compute_long_term_key("alice", "example.com", "password");
    let key2 = compute_long_term_key("bob", "example.com", "password");
    assert_ne!(key1, key2);
}

#[tokio::test]
async fn long_term_key_differs_by_realm() {
    let key1 = compute_long_term_key("alice", "example.com", "password");
    let key2 = compute_long_term_key("alice", "other.com", "password");
    assert_ne!(key1, key2);
}

#[tokio::test]
async fn long_term_key_differs_by_password() {
    let key1 = compute_long_term_key("alice", "example.com", "password1");
    let key2 = compute_long_term_key("alice", "example.com", "password2");
    assert_ne!(key1, key2);
}

// --- Message integrity ---

#[tokio::test]
async fn message_with_integrity_roundtrip() {
    let creds = TurnCredentials {
        username: "alice".into(),
        password: "secret".into(),
        realm: "example.com".into(),
        nonce: "nonce456".into(),
    };
    let msg = TurnMessage::allocate_request_with_credentials(&creds);

    let integrity_key = compute_long_term_key("alice", "example.com", "secret");
    let bytes = msg.to_bytes(Some(&integrity_key));
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    // The MESSAGE-INTEGRITY attribute should be present
    let has_integrity = parsed
        .attributes
        .iter()
        .any(|a| matches!(a, TurnAttribute::MessageIntegrity(_)));
    assert!(has_integrity);
}

// --- Channel bind request ---

#[tokio::test]
async fn channel_bind_request_roundtrip() {
    let peer: SocketAddr = "10.0.0.5:5555".parse().unwrap();
    let creds = TurnCredentials {
        username: "alice".into(),
        password: "secret".into(),
        realm: "example.com".into(),
        nonce: "nonce789".into(),
    };

    let msg = TurnMessage::channel_bind_request(0x4000, peer, &creds);
    let bytes = msg.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.channel_number().unwrap(), 0x4000);
    assert_eq!(parsed.peer_address().unwrap(), peer);
}

// --- Allocate response with relay address ---

#[tokio::test]
async fn allocate_response_with_relay_address() {
    let relay: SocketAddr = "198.51.100.1:49152".parse().unwrap();
    let mapped: SocketAddr = "203.0.113.1:54321".parse().unwrap();

    let response = TurnMessage {
        msg_type: 0x0103, // Allocate Response
        transaction_id: TransactionId::random(),
        attributes: vec![
            TurnAttribute::XorRelayedAddress(relay),
            TurnAttribute::XorMappedAddress(mapped),
            TurnAttribute::Lifetime(600),
        ],
    };

    let bytes = response.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_allocate_response());
    assert_eq!(parsed.relayed_address().unwrap(), relay);
    assert_eq!(parsed.mapped_address().unwrap(), mapped);
    assert_eq!(parsed.lifetime().unwrap(), 600);
}

// --- Error responses ---

#[tokio::test]
async fn allocate_error_response_with_realm_nonce() {
    let response = TurnMessage {
        msg_type: 0x0113, // Allocate Error Response
        transaction_id: TransactionId::random(),
        attributes: vec![
            TurnAttribute::ErrorCode(401, "Unauthorized".into()),
            TurnAttribute::Realm("example.com".into()),
            TurnAttribute::Nonce("challenge123".into()),
        ],
    };

    let bytes = response.to_bytes(None);
    let parsed = TurnMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_allocate_error());
    assert_eq!(parsed.error_code().unwrap(), (401, "Unauthorized".into()));
    assert_eq!(parsed.realm().unwrap(), "example.com");
    assert_eq!(parsed.nonce().unwrap(), "challenge123");
}
