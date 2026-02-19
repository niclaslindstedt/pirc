//! STUN protocol integration tests.
//!
//! Exercises STUN message encoding/decoding round-trips, binding
//! request/response flows, error handling, and the reflexive address
//! discovery function against a mock STUN server.

use std::net::SocketAddr;

use pirc_p2p::stun::{discover_reflexive_address, StunAttribute, StunMessage, TransactionId};
use tokio::net::UdpSocket;

use super::spawn_mock_stun_server;

// --- Encoding / decoding round-trips ---

#[tokio::test]
async fn stun_binding_request_roundtrip() {
    let msg = StunMessage::binding_request();
    let bytes = msg.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.msg_type, msg.msg_type);
    assert_eq!(parsed.transaction_id, msg.transaction_id);
    assert!(parsed.attributes.is_empty());
}

#[tokio::test]
async fn stun_binding_response_with_xor_mapped_address_ipv4() {
    let tid = TransactionId::random();
    let addr: SocketAddr = "203.0.113.42:5060".parse().unwrap();

    let response = StunMessage {
        msg_type: 0x0101,
        transaction_id: tid,
        attributes: vec![StunAttribute::XorMappedAddress(addr)],
    };

    let bytes = response.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_binding_response());
    assert!(!parsed.is_binding_error());
    assert_eq!(parsed.mapped_address(), Some(addr));
}

#[tokio::test]
async fn stun_binding_response_with_xor_mapped_address_ipv6() {
    let tid = TransactionId::random();
    let addr: SocketAddr = "[2001:db8::1]:8080".parse().unwrap();

    let response = StunMessage {
        msg_type: 0x0101,
        transaction_id: tid,
        attributes: vec![StunAttribute::XorMappedAddress(addr)],
    };

    let bytes = response.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_binding_response());
    assert_eq!(parsed.mapped_address(), Some(addr));
}

#[tokio::test]
async fn stun_mapped_address_fallback() {
    let tid = TransactionId::random();
    let addr: SocketAddr = "192.168.1.100:1234".parse().unwrap();

    let response = StunMessage {
        msg_type: 0x0101,
        transaction_id: tid,
        attributes: vec![StunAttribute::MappedAddress(addr)],
    };

    let bytes = response.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.mapped_address(), Some(addr));
}

#[tokio::test]
async fn stun_xor_mapped_preferred_over_mapped() {
    let tid = TransactionId::random();
    let xor_addr: SocketAddr = "1.2.3.4:5678".parse().unwrap();
    let mapped_addr: SocketAddr = "5.6.7.8:9012".parse().unwrap();

    let response = StunMessage {
        msg_type: 0x0101,
        transaction_id: tid,
        attributes: vec![
            StunAttribute::MappedAddress(mapped_addr),
            StunAttribute::XorMappedAddress(xor_addr),
        ],
    };

    let bytes = response.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.mapped_address(), Some(xor_addr));
}

// --- Error handling ---

#[tokio::test]
async fn stun_rejects_short_message() {
    let result = StunMessage::from_bytes(&[0; 10]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too short"));
}

#[tokio::test]
async fn stun_rejects_bad_magic_cookie() {
    let mut bytes = vec![0u8; 20];
    bytes[0] = 0x01;
    bytes[1] = 0x01;
    bytes[4] = 0xFF;
    bytes[5] = 0xFF;
    bytes[6] = 0xFF;
    bytes[7] = 0xFF;

    let result = StunMessage::from_bytes(&bytes);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("magic cookie"));
}

#[tokio::test]
async fn stun_binding_error_response_detected() {
    let tid = TransactionId::random();
    let msg = StunMessage {
        msg_type: 0x0111, // Binding Error Response
        transaction_id: tid,
        attributes: Vec::new(),
    };
    let bytes = msg.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert!(parsed.is_binding_error());
    assert!(!parsed.is_binding_response());
}

#[tokio::test]
async fn stun_unknown_attributes_preserved() {
    let tid = TransactionId::random();
    let addr: SocketAddr = "10.0.0.1:3478".parse().unwrap();

    let response = StunMessage {
        msg_type: 0x0101,
        transaction_id: tid,
        attributes: vec![
            StunAttribute::Unknown(0x8028, vec![0xAB, 0xCD, 0xEF, 0x01]),
            StunAttribute::XorMappedAddress(addr),
        ],
    };

    let bytes = response.to_bytes();
    let parsed = StunMessage::from_bytes(&bytes).unwrap();

    assert_eq!(parsed.attributes.len(), 2);
    assert_eq!(parsed.mapped_address(), Some(addr));

    match &parsed.attributes[0] {
        StunAttribute::Unknown(t, v) => {
            assert_eq!(*t, 0x8028);
            assert_eq!(v, &[0xAB, 0xCD, 0xEF, 0x01]);
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}

// --- Loopback STUN server tests ---

#[tokio::test]
async fn discover_reflexive_address_via_mock_stun() {
    let (server_addr, _handle) = spawn_mock_stun_server().await;

    let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = client_sock.local_addr().unwrap();

    let reflexive = discover_reflexive_address(&client_sock, server_addr)
        .await
        .unwrap();

    // On loopback the reflexive address equals the local address
    assert_eq!(reflexive, client_addr);
}

#[tokio::test]
async fn stun_multiple_clients_sequential() {
    let (server_addr, _handle) = spawn_mock_stun_server().await;

    for _ in 0..3 {
        let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_addr = client_sock.local_addr().unwrap();

        let reflexive = discover_reflexive_address(&client_sock, server_addr)
            .await
            .unwrap();

        assert_eq!(reflexive, client_addr);
    }
}

#[tokio::test]
async fn stun_error_response_from_server() {
    // Spawn a mock server that always returns Binding Error Response
    let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server_sock.local_addr().unwrap();

    let _handle = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        let (len, src) = server_sock.recv_from(&mut buf).await.unwrap();
        let request = StunMessage::from_bytes(&buf[..len]).unwrap();

        let error_response = StunMessage {
            msg_type: 0x0111, // Binding Error Response
            transaction_id: request.transaction_id,
            attributes: Vec::new(),
        };
        let _ = server_sock
            .send_to(&error_response.to_bytes(), src)
            .await;
    });

    let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let result = discover_reflexive_address(&client_sock, server_addr).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Binding Error Response"));
}

#[tokio::test]
async fn stun_transaction_id_uniqueness() {
    let a = TransactionId::random();
    let b = TransactionId::random();
    assert_ne!(a, b);
}
