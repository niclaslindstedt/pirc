//! TURN (RFC 5766) relay client for NAT traversal fallback.
//!
//! Implements the TURN protocol on top of the existing STUN message format:
//! - Allocate request/response for creating relay allocations
//! - `CreatePermission` for allowing peer traffic through relay
//! - `ChannelBind` for efficient data relay via channel numbers
//! - Send/Data indications for relaying application data
//! - Long-term credential authentication (`MESSAGE-INTEGRITY`)

mod client;
mod codec;
mod types;

pub use client::{
    allocate, channel_bind, create_permission, decode_channel_data, encode_channel_data,
    parse_data_indication, send_to_peer,
};
pub use codec::compute_long_term_key;
pub use types::{Allocation, TurnAttribute, TurnCredentials, TurnMessage};

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::net::UdpSocket;

    use crate::stun::TransactionId;

    use super::client::{
        allocate, channel_bind, decode_channel_data, encode_channel_data, parse_data_indication,
        send_to_peer,
    };
    use super::codec::{compute_hmac_sha1, compute_long_term_key};
    use super::types::{
        Allocation, TurnAttribute, TurnCredentials, TurnMessage,
        ALLOCATE_ERROR_RESPONSE, ALLOCATE_REQUEST, ALLOCATE_RESPONSE,
        ATTR_LIFETIME, ATTR_MESSAGE_INTEGRITY, CHANNEL_BIND_ERROR_RESPONSE,
        CHANNEL_BIND_REQUEST, CHANNEL_BIND_RESPONSE, CREATE_PERMISSION_ERROR_RESPONSE,
        CREATE_PERMISSION_REQUEST, CREATE_PERMISSION_RESPONSE, DATA_INDICATION,
        HEADER_SIZE, HMAC_SHA1_LEN, MAGIC_COOKIE, REFRESH_REQUEST, REFRESH_RESPONSE,
        SEND_INDICATION,
    };

    fn fixed_tid() -> TransactionId {
        TransactionId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
    }

    fn test_creds() -> TurnCredentials {
        TurnCredentials {
            username: "user".into(),
            password: "pass".into(),
            realm: "example.com".into(),
            nonce: "abc123".into(),
        }
    }

    // --- Allocate request/response tests ---

    #[test]
    fn allocate_request_serialization() {
        let msg = TurnMessage::allocate_request();
        let bytes = msg.to_bytes(None);

        // Should have header + REQUESTED-TRANSPORT attribute
        assert!(bytes.len() > HEADER_SIZE);
        // Message type: Allocate Request (0x0003)
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 0x03);
        // Magic cookie
        assert_eq!(
            &bytes[4..8],
            &MAGIC_COOKIE.to_be_bytes()
        );
    }

    #[test]
    fn allocate_request_with_credentials_roundtrip() {
        let creds = test_creds();
        let msg = TurnMessage::allocate_request_with_credentials(&creds);
        let key = compute_long_term_key(&creds.username, &creds.realm, &creds.password);
        let bytes = msg.to_bytes(Some(&key));
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.msg_type, ALLOCATE_REQUEST);
        // Should have RequestedTransport, Username, Realm, Nonce, and MessageIntegrity
        let has_username = parsed.attributes.iter().any(|a| {
            matches!(a, TurnAttribute::Username(u) if u == "user")
        });
        let has_realm = parsed.attributes.iter().any(|a| {
            matches!(a, TurnAttribute::Realm(r) if r == "example.com")
        });
        let has_nonce = parsed.attributes.iter().any(|a| {
            matches!(a, TurnAttribute::Nonce(n) if n == "abc123")
        });
        let has_transport = parsed.attributes.iter().any(|a| {
            matches!(a, TurnAttribute::RequestedTransport(17))
        });
        let has_integrity = parsed.attributes.iter().any(|a| {
            matches!(a, TurnAttribute::MessageIntegrity(_))
        });

        assert!(has_username, "missing USERNAME");
        assert!(has_realm, "missing REALM");
        assert!(has_nonce, "missing NONCE");
        assert!(has_transport, "missing REQUESTED-TRANSPORT");
        assert!(has_integrity, "missing MESSAGE-INTEGRITY");
    }

    #[test]
    fn allocate_response_parsing() {
        let relay_addr: SocketAddr = "198.51.100.1:49152".parse().unwrap();
        let mapped_addr: SocketAddr = "203.0.113.42:5060".parse().unwrap();

        let response = TurnMessage {
            msg_type: ALLOCATE_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![
                TurnAttribute::XorRelayedAddress(relay_addr),
                TurnAttribute::XorMappedAddress(mapped_addr),
                TurnAttribute::Lifetime(600),
            ],
        };

        let bytes = response.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert!(parsed.is_allocate_response());
        assert_eq!(parsed.relayed_address(), Some(relay_addr));
        assert_eq!(parsed.mapped_address(), Some(mapped_addr));
        assert_eq!(parsed.lifetime(), Some(600));
    }

    #[test]
    fn allocate_error_response_401() {
        let response = TurnMessage {
            msg_type: ALLOCATE_ERROR_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![
                TurnAttribute::ErrorCode(401, "Unauthorized".into()),
                TurnAttribute::Realm("example.com".into()),
                TurnAttribute::Nonce("nonce123".into()),
            ],
        };

        let bytes = response.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert!(parsed.is_allocate_error());
        let (code, reason) = parsed.error_code().unwrap();
        assert_eq!(code, 401);
        assert_eq!(reason, "Unauthorized");
        assert_eq!(parsed.realm(), Some("example.com"));
        assert_eq!(parsed.nonce(), Some("nonce123"));
    }

    // --- CreatePermission tests ---

    #[test]
    fn create_permission_request_roundtrip() {
        let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let creds = test_creds();
        let msg = TurnMessage::create_permission_request(peer, &creds);
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.msg_type, CREATE_PERMISSION_REQUEST);
        assert_eq!(parsed.peer_address(), Some(peer));
    }

    #[test]
    fn create_permission_response_detected() {
        let msg = TurnMessage {
            msg_type: CREATE_PERMISSION_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: Vec::new(),
        };
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert!(parsed.is_create_permission_response());
    }

    #[test]
    fn create_permission_error_detected() {
        let msg = TurnMessage {
            msg_type: CREATE_PERMISSION_ERROR_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![TurnAttribute::ErrorCode(403, "Forbidden".into())],
        };
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert!(parsed.is_create_permission_error());
        let (code, reason) = parsed.error_code().unwrap();
        assert_eq!(code, 403);
        assert_eq!(reason, "Forbidden");
    }

    // --- ChannelBind tests ---

    #[test]
    fn channel_bind_request_roundtrip() {
        let peer: SocketAddr = "192.168.1.100:8080".parse().unwrap();
        let creds = test_creds();
        let msg = TurnMessage::channel_bind_request(0x4000, peer, &creds);
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.msg_type, CHANNEL_BIND_REQUEST);
        assert_eq!(parsed.channel_number(), Some(0x4000));
        assert_eq!(parsed.peer_address(), Some(peer));
    }

    #[test]
    fn channel_bind_response_detected() {
        let msg = TurnMessage {
            msg_type: CHANNEL_BIND_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: Vec::new(),
        };
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert!(parsed.is_channel_bind_response());
    }

    #[test]
    fn channel_bind_error_detected() {
        let msg = TurnMessage {
            msg_type: CHANNEL_BIND_ERROR_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![TurnAttribute::ErrorCode(400, "Bad Request".into())],
        };
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert!(parsed.is_channel_bind_error());
    }

    // --- Send/Data indication tests ---

    #[test]
    fn send_indication_roundtrip() {
        let peer: SocketAddr = "10.0.0.5:12345".parse().unwrap();
        let payload = b"Hello, peer!".to_vec();
        let msg = TurnMessage::send_indication(peer, payload.clone());
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert!(parsed.is_send_indication());
        assert_eq!(parsed.peer_address(), Some(peer));
        assert_eq!(parsed.data(), Some(payload.as_slice()));
    }

    #[test]
    fn data_indication_parsing() {
        let peer: SocketAddr = "172.16.0.1:5000".parse().unwrap();
        let payload = b"Relayed data from peer".to_vec();

        let msg = TurnMessage {
            msg_type: DATA_INDICATION,
            transaction_id: fixed_tid(),
            attributes: vec![
                TurnAttribute::XorPeerAddress(peer),
                TurnAttribute::Data(payload.clone()),
            ],
        };

        let bytes = msg.to_bytes(None);
        let (parsed_peer, parsed_data) = parse_data_indication(&bytes).unwrap();

        assert_eq!(parsed_peer, peer);
        assert_eq!(parsed_data, payload);
    }

    #[test]
    fn data_indication_rejects_non_indication() {
        let msg = TurnMessage {
            msg_type: ALLOCATE_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: Vec::new(),
        };
        let bytes = msg.to_bytes(None);
        let result = parse_data_indication(&bytes);
        assert!(result.is_err());
    }

    // --- ChannelData tests ---

    #[test]
    fn channel_data_encode_decode_roundtrip() {
        let channel = 0x4001u16;
        let data = b"channel data payload";
        let encoded = encode_channel_data(channel, data);
        let (parsed_ch, parsed_data) = decode_channel_data(&encoded).unwrap();

        assert_eq!(parsed_ch, channel);
        assert_eq!(parsed_data, data);
    }

    #[test]
    fn channel_data_with_padding() {
        // 5-byte payload should be padded to 8 (next 4-byte boundary)
        let data = b"hello";
        let encoded = encode_channel_data(0x4000, data);
        // 2 (channel) + 2 (length) + 5 (data) + 3 (padding) = 12
        assert_eq!(encoded.len(), 12);
        let (_, parsed) = decode_channel_data(&encoded).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn channel_data_rejects_truncated() {
        let result = decode_channel_data(&[0x40, 0x00]);
        assert!(result.is_err());
    }

    #[test]
    fn channel_data_rejects_short_payload() {
        // Header claims 100 bytes but only 4 available after header
        let mut data = vec![0x40, 0x00, 0x00, 0x64]; // channel 0x4000, length 100
        data.extend_from_slice(&[0u8; 4]);
        let result = decode_channel_data(&data);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("truncated"), "error: {err}");
    }

    // --- Credential / HMAC tests ---

    #[test]
    fn long_term_key_computation() {
        // RFC 5389 example: key = MD5("user:realm:pass")
        let key = compute_long_term_key("user", "example.com", "pass");
        assert_eq!(key.len(), 16); // MD5 produces 16 bytes

        // Verify deterministic
        let key2 = compute_long_term_key("user", "example.com", "pass");
        assert_eq!(key, key2);

        // Different inputs produce different keys
        let key3 = compute_long_term_key("user2", "example.com", "pass");
        assert_ne!(key, key3);
    }

    #[test]
    fn message_integrity_is_20_bytes() {
        let creds = test_creds();
        let key = compute_long_term_key(&creds.username, &creds.realm, &creds.password);
        let msg = TurnMessage::allocate_request_with_credentials(&creds);
        let bytes = msg.to_bytes(Some(&key));
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        for attr in &parsed.attributes {
            if let TurnAttribute::MessageIntegrity(hmac_val) = attr {
                assert_eq!(hmac_val.len(), 20);
                return;
            }
        }
        panic!("no MESSAGE-INTEGRITY found");
    }

    #[test]
    fn message_integrity_verification() {
        let creds = test_creds();
        let key = compute_long_term_key(&creds.username, &creds.realm, &creds.password);
        let msg = TurnMessage::allocate_request_with_credentials(&creds);
        let bytes = msg.to_bytes(Some(&key));

        // The MESSAGE-INTEGRITY covers everything up to (but not including) itself,
        // with the message length adjusted to include the MI TLV.
        // Verify that the HMAC-SHA1 in the message is correct.
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        let mi_value = parsed.attributes.iter().find_map(|a| {
            if let TurnAttribute::MessageIntegrity(v) = a {
                Some(*v)
            } else {
                None
            }
        }).expect("MESSAGE-INTEGRITY not found");

        // Find where MESSAGE-INTEGRITY starts in the raw bytes
        // It's the last attribute: 4 byte header + 20 byte value = 24 bytes from end
        let mi_offset = bytes.len() - 24;

        // Recompute: take everything before MI, but adjust the message length field
        let mut hmac_input = bytes[..mi_offset].to_vec();
        // The message length in the header should include the MI TLV
        #[allow(clippy::cast_possible_truncation)]
        let adjusted_len = (mi_offset - HEADER_SIZE + 24) as u16;
        hmac_input[2..4].copy_from_slice(&adjusted_len.to_be_bytes());

        let expected = compute_hmac_sha1(&key, &hmac_input);
        assert_eq!(mi_value, expected);
    }

    // --- XOR address encoding tests ---

    #[test]
    fn xor_relayed_address_ipv4_roundtrip() {
        let addr: SocketAddr = "198.51.100.1:49152".parse().unwrap();
        let tid = fixed_tid();
        let response = TurnMessage {
            msg_type: ALLOCATE_RESPONSE,
            transaction_id: tid,
            attributes: vec![TurnAttribute::XorRelayedAddress(addr)],
        };

        let bytes = response.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.relayed_address(), Some(addr));
    }

    #[test]
    fn xor_relayed_address_ipv6_roundtrip() {
        let addr: SocketAddr = "[2001:db8::1]:8080".parse().unwrap();
        let tid = fixed_tid();
        let response = TurnMessage {
            msg_type: ALLOCATE_RESPONSE,
            transaction_id: tid,
            attributes: vec![TurnAttribute::XorRelayedAddress(addr)],
        };

        let bytes = response.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.relayed_address(), Some(addr));
    }

    #[test]
    fn xor_peer_address_roundtrip() {
        let addr: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let tid = fixed_tid();
        let msg = TurnMessage {
            msg_type: SEND_INDICATION,
            transaction_id: tid,
            attributes: vec![TurnAttribute::XorPeerAddress(addr)],
        };

        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.peer_address(), Some(addr));
    }

    // --- Error handling tests ---

    #[test]
    fn rejects_truncated_turn_attribute() {
        let tid = fixed_tid();
        let mut bytes = vec![];
        // STUN header
        bytes.extend_from_slice(&ALLOCATE_RESPONSE.to_be_bytes());
        bytes.extend_from_slice(&8u16.to_be_bytes()); // 8 bytes of attrs
        bytes.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        bytes.extend_from_slice(tid.as_bytes());
        // Attribute claiming 100 bytes but only 4 available
        bytes.extend_from_slice(&ATTR_LIFETIME.to_be_bytes());
        bytes.extend_from_slice(&100u16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 4]);

        let result = TurnMessage::from_bytes(&bytes);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("truncated"), "error: {err}");
    }

    #[test]
    fn rejects_wrong_mi_size() {
        let tid = fixed_tid();
        let mut bytes = vec![];
        // STUN header
        bytes.extend_from_slice(&ALLOCATE_RESPONSE.to_be_bytes());
        let attr_data_len = 4 + 10; // MI header + 10 byte wrong-size value
        #[allow(clippy::cast_possible_truncation)]
        bytes.extend_from_slice(&(attr_data_len as u16).to_be_bytes());
        bytes.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        bytes.extend_from_slice(tid.as_bytes());
        // MESSAGE-INTEGRITY with wrong size (10 instead of 20)
        bytes.extend_from_slice(&ATTR_MESSAGE_INTEGRITY.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 10]);
        // Pad to 4-byte boundary
        bytes.extend_from_slice(&[0u8; 2]);

        let result = TurnMessage::from_bytes(&bytes);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MESSAGE-INTEGRITY"), "error: {err}");
    }

    // --- Refresh tests ---

    #[test]
    fn refresh_request_roundtrip() {
        let creds = test_creds();
        let msg = TurnMessage::refresh_request(300, &creds);
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.msg_type, REFRESH_REQUEST);
        assert_eq!(parsed.lifetime(), Some(300));
    }

    #[test]
    fn refresh_response_detected() {
        let msg = TurnMessage {
            msg_type: REFRESH_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![TurnAttribute::Lifetime(600)],
        };
        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();
        assert!(parsed.is_refresh_response());
        assert_eq!(parsed.lifetime(), Some(600));
    }

    // --- Loopback integration test ---

    #[tokio::test]
    async fn turn_allocate_loopback() {
        // Simulate a TURN server that does the 401 challenge then allocates
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();
        let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let relay_addr: SocketAddr = "198.51.100.1:49152".parse().unwrap();

        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 4096];

            // First request: respond with 401
            let (len, src) = server_sock.recv_from(&mut buf).await.unwrap();
            let request = TurnMessage::from_bytes(&buf[..len]).unwrap();
            assert_eq!(request.msg_type, ALLOCATE_REQUEST);

            let challenge = TurnMessage {
                msg_type: ALLOCATE_ERROR_RESPONSE,
                transaction_id: request.transaction_id,
                attributes: vec![
                    TurnAttribute::ErrorCode(401, "Unauthorized".into()),
                    TurnAttribute::Realm("test.example.com".into()),
                    TurnAttribute::Nonce("servernonce".into()),
                ],
            };
            server_sock
                .send_to(&challenge.to_bytes(None), src)
                .await
                .unwrap();

            // Second request: authenticated, respond with success
            let (len, src) = server_sock.recv_from(&mut buf).await.unwrap();
            let auth_request = TurnMessage::from_bytes(&buf[..len]).unwrap();
            assert_eq!(auth_request.msg_type, ALLOCATE_REQUEST);

            // Verify auth attributes are present
            let has_username = auth_request
                .attributes
                .iter()
                .any(|a| matches!(a, TurnAttribute::Username(_)));
            assert!(has_username);

            let response = TurnMessage {
                msg_type: ALLOCATE_RESPONSE,
                transaction_id: auth_request.transaction_id,
                attributes: vec![
                    TurnAttribute::XorRelayedAddress(relay_addr),
                    TurnAttribute::XorMappedAddress(src),
                    TurnAttribute::Lifetime(600),
                ],
            };
            server_sock
                .send_to(&response.to_bytes(None), src)
                .await
                .unwrap();
        });

        let allocation = allocate(&client_sock, server_addr, "testuser", "testpass")
            .await
            .unwrap();

        assert_eq!(allocation.relay_addr, relay_addr);
        assert!(allocation.mapped_addr.is_some());
        assert_eq!(allocation.lifetime, 600);

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn turn_send_indication_loopback() {
        let server_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server_sock.local_addr().unwrap();
        let client_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let payload = b"hello from client".to_vec();

        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let (len, _src) = server_sock.recv_from(&mut buf).await.unwrap();
            let msg = TurnMessage::from_bytes(&buf[..len]).unwrap();
            assert!(msg.is_send_indication());
            assert_eq!(msg.peer_address(), Some(peer));
            assert_eq!(msg.data(), Some(b"hello from client".as_slice()));
        });

        send_to_peer(&client_sock, server_addr, peer, payload)
            .await
            .unwrap();

        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn channel_bind_validates_range() {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server: SocketAddr = "127.0.0.1:3478".parse().unwrap();
        let peer: SocketAddr = "10.0.0.1:9000".parse().unwrap();
        let creds = test_creds();

        // Channel number below valid range
        let result = channel_bind(&sock, server, 0x3FFF, peer, &creds).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid channel number"), "error: {err}");

        // Channel number above valid range
        let result = channel_bind(&sock, server, 0x8000, peer, &creds).await;
        assert!(result.is_err());
    }

    // --- Unknown attribute preservation ---

    #[test]
    fn unknown_turn_attributes_preserved() {
        let msg = TurnMessage {
            msg_type: ALLOCATE_RESPONSE,
            transaction_id: fixed_tid(),
            attributes: vec![
                TurnAttribute::Unknown(0x8028, vec![0xAB, 0xCD, 0xEF, 0x01]),
                TurnAttribute::Lifetime(300),
            ],
        };

        let bytes = msg.to_bytes(None);
        let parsed = TurnMessage::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.attributes.len(), 2);
        match &parsed.attributes[0] {
            TurnAttribute::Unknown(t, v) => {
                assert_eq!(*t, 0x8028);
                assert_eq!(v, &[0xAB, 0xCD, 0xEF, 0x01]);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
        assert_eq!(parsed.lifetime(), Some(300));
    }
}
