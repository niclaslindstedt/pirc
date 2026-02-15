use crate::app::*;
use crate::config::ClientConfig;
use pirc_crypto::protocol::{decode_from_wire, encode_for_wire, KeyExchangeMessage};

// ── Encryption tests ─────────────────────────────────────────

#[test]
fn app_new_initializes_encryption_manager() {
    let config = ClientConfig::default();
    let app = App::new(config);
    // EncryptionManager is initialized — verify by checking fingerprint is valid
    let fp = app.encryption.get_identity_fingerprint();
    assert_eq!(fp.len(), 95); // 32 bytes as "XX:XX:...:XX"
}

#[test]
fn upload_pre_key_bundle_constructs_valid_message() {
    // Verify the message construction logic produces a valid bundle
    let config = ClientConfig::default();
    let app = App::new(config);

    let bundle = app.encryption.create_pre_key_bundle();
    let bundle_msg = KeyExchangeMessage::Bundle(Box::new(bundle));
    let encoded = pirc_crypto::protocol::encode_for_wire(&bundle_msg.to_bytes());

    // Verify the encoded data round-trips correctly
    let decoded = decode_from_wire(&encoded).expect("decode should succeed");
    let restored = KeyExchangeMessage::from_bytes(&decoded).expect("parse should succeed");
    assert!(matches!(restored, KeyExchangeMessage::Bundle(_)));

    if let KeyExchangeMessage::Bundle(b) = restored {
        b.validate().expect("bundle should be valid");
    }
}

#[test]
fn upload_pre_key_bundle_no_connection_does_not_panic() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("testuser".to_string());
    let mut app = App::new(config);

    // No connection — upload should silently do nothing
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.upload_pre_key_bundle());
    // Should not panic and encryption manager should still be valid
    assert!(!app.encryption.get_identity_fingerprint().is_empty());
}

#[test]
fn rpl_welcome_triggers_bundle_upload_without_panic() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("testuser".to_string());
    let mut app = App::new(config);

    // Set up registration state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.registration = Some(RegistrationState::new(
        "testuser".into(),
        vec![],
        "testuser".into(),
        "testuser".into(),
    ));
    app.registration_deadline = Some(Instant::now() + REGISTRATION_TIMEOUT);

    let msg = Message::with_prefix(
        pirc_protocol::Prefix::Server("irc.test.net".into()),
        pirc_protocol::Command::Numeric(1),
        vec!["testuser".into(), "Welcome to the test network!".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // Should not panic even without a real connection
    // (upload_pre_key_bundle is a no-op when connection is None)
    rt.block_on(app.handle_server_message(&msg));
    assert!(app.connection_mgr.is_connected());
}

#[test]
fn upload_pre_key_bundle_message_format() {
    // Verify the wire message has the expected structure:
    // PIRC KEYEXCHANGE * <base64-data>
    let config = ClientConfig::default();
    let app = App::new(config);

    let bundle = app.encryption.create_pre_key_bundle();
    let bundle_msg = KeyExchangeMessage::Bundle(Box::new(bundle));
    let encoded = pirc_crypto::protocol::encode_for_wire(&bundle_msg.to_bytes());

    let msg = Message::new(
        Command::Pirc(pirc_protocol::PircSubcommand::KeyExchange),
        vec!["*".to_string(), encoded.clone()],
    );

    // Verify the message serializes correctly
    let wire = msg.to_string();
    assert!(wire.starts_with("PIRC KEYEXCHANGE * "));
    // The encoded data should be present (possibly with : prefix for trailing)
    assert!(wire.contains(&encoded[..20])); // check first 20 chars of base64
}

// ── Key exchange protocol flow tests ─────────────────────────

#[test]
fn handle_pirc_keyexchange_message_routes_correctly() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // Create a PIRC KEYEXCHANGE message with a RequestBundle payload
    let ke_msg = KeyExchangeMessage::RequestBundle;
    let encoded = encode_for_wire(&ke_msg.to_bytes());
    let msg = Message::with_prefix(
        pirc_protocol::Prefix::user("alice", "alice", "host.com"),
        Command::Pirc(pirc_protocol::PircSubcommand::KeyExchange),
        vec!["testuser".into(), encoded],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Should be handled (returns true)
    let handled = rt.block_on(app.handle_pirc_message(&msg));
    assert!(handled);
}

#[test]
fn handle_pirc_keyexchange_complete_promotes_session() {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let config = ClientConfig::default();
            let mut app = App::new(config);

            // Set up a pending exchange manually by initiating key exchange
            // and processing a bundle response
            let bob = crate::encryption::EncryptionManager::new();
            let _request = app.encryption.initiate_key_exchange("bob");
            let bob_bundle = bob.create_pre_key_bundle();
            let (_init_msg, _queued) = app
                .encryption
                .handle_bundle_response("bob", &bob_bundle)
                .expect("bundle response");

            // Now we're in AwaitingComplete state
            assert!(app.encryption.has_pending_exchange("bob"));
            assert!(!app.encryption.has_session("bob"));

            // Handle the KEYEXCHANGE-COMPLETE
            app.handle_key_exchange_complete("bob");

            // Session should now be active
            assert!(app.encryption.has_session("bob"));
            assert!(!app.encryption.has_pending_exchange("bob"));
        })
        .expect("thread spawn failed")
        .join();

    result.expect("handle_pirc_keyexchange_complete panicked");
}

#[test]
fn handle_pirc_message_ignores_non_pirc_commands() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let msg = Message::with_prefix(
        pirc_protocol::Prefix::user("alice", "alice", "host.com"),
        pirc_protocol::Command::Privmsg,
        vec!["#channel".into(), "hello".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let handled = rt.block_on(app.handle_pirc_message(&msg));
    assert!(!handled);
}

#[test]
fn handle_pirc_message_ignores_server_prefix() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let ke_msg = KeyExchangeMessage::RequestBundle;
    let encoded = encode_for_wire(&ke_msg.to_bytes());
    let msg = Message::with_prefix(
        pirc_protocol::Prefix::Server("irc.server.com".into()),
        Command::Pirc(pirc_protocol::PircSubcommand::KeyExchange),
        vec!["testuser".into(), encoded],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Server prefix should not be handled (no user sender)
    let handled = rt.block_on(app.handle_pirc_message(&msg));
    assert!(!handled);
}

#[test]
fn handle_encrypted_message_with_invalid_data_does_not_panic() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let initial_count = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap()
        .len();

    // Try to handle an encrypted message with garbage data
    // (will fail at parse stage — logged via warn!, no status push)
    let fake_data = encode_for_wire(b"not a real encrypted message");
    app.handle_encrypted_message("alice", &fake_data);

    // Should not panic and should not add messages to status
    // (parse failures are logged, not shown to user)
    let new_count = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap()
        .len();
    assert_eq!(new_count, initial_count);
}

#[test]
fn private_msg_initiates_key_exchange_when_no_session() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Send a private message to a peer with no session (no connection either)
    rt.block_on(app.handle_private_msg_command("bob", "hello"));

    // Should show "Not connected" since there's no connection
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("Not connected"),
        "expected 'Not connected', got: {}",
        last_line.content
    );
}

#[test]
fn send_private_message_queues_when_exchange_pending() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // Initiate a key exchange manually
    let _request = app.encryption.initiate_key_exchange("bob");
    assert!(app.encryption.has_pending_exchange("bob"));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Try to send — should queue since exchange is pending
    let handled = rt.block_on(app.send_private_message("bob", "hello"));
    assert!(handled);

    // Status should mention queuing
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("queued"),
        "expected message about queuing, got: {}",
        last_line.content
    );
}

#[test]
fn send_private_message_initiates_exchange_when_no_session() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // No session, no pending exchange
    assert!(!app.encryption.has_session("bob"));
    assert!(!app.encryption.has_pending_exchange("bob"));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Send private message — should initiate key exchange
    let handled = rt.block_on(app.send_private_message("bob", "hello"));
    assert!(handled);

    // Should now have a pending exchange
    assert!(app.encryption.has_pending_exchange("bob"));

    // Status should mention establishing
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("Establishing"),
        "expected status about establishing, got: {}",
        last_line.content
    );
}

#[test]
fn handle_command_msg_to_user_routes_through_encryption() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // /msg bob hello — should go through encryption path
    let cmd = ClientCommand::Msg("bob".into(), "hello".into());
    rt.block_on(app.handle_command(cmd));

    // Should show "Not connected" (no connection)
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("Not connected"),
        "expected 'Not connected', got: {}",
        last_line.content
    );
}

#[test]
fn handle_command_msg_to_channel_bypasses_encryption() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // /msg #channel hello — should NOT go through encryption
    let cmd = ClientCommand::Msg("#channel".into(), "hello".into());
    rt.block_on(app.handle_command(cmd));

    // Should show "Not connected" (no connection), and no encryption initiated
    assert!(!app.encryption.has_pending_exchange("#channel"));
}

#[test]
fn handle_chat_message_query_routes_through_encryption() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Chat message in a query buffer — should route through encryption
    let target = crate::tui::buffer_manager::BufferId::Query("bob".into());
    rt.block_on(app.handle_chat_message("hello", &target));

    // Should show "Not connected" (no connection)
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("Not connected"),
        "expected 'Not connected', got: {}",
        last_line.content
    );
}

#[test]
fn handle_key_exchange_invalid_data_does_not_panic() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Invalid base64 data
    rt.block_on(app.handle_key_exchange_message("alice", "not-valid-base64!!!"));
    // Should not panic

    // Invalid key exchange message (valid base64 but bad crypto data)
    let fake_data = encode_for_wire(&[255, 0, 0, 0]);
    rt.block_on(app.handle_key_exchange_message("alice", &fake_data));
    // Should not panic
}

#[test]
fn full_key_exchange_protocol_flow_via_app() {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let config_a = ClientConfig::default();
            let mut alice_app = App::new(config_a);

            let config_b = ClientConfig::default();
            let mut bob_app = App::new(config_b);

            // Step 1: Alice initiates key exchange with Bob
            let request = alice_app.encryption.initiate_key_exchange("bob");
            assert!(matches!(request, KeyExchangeMessage::RequestBundle));
            assert!(alice_app.encryption.has_pending_exchange("bob"));

            // Also queue a message during the exchange
            alice_app
                .encryption
                .queue_message("bob", b"hello bob!".to_vec());

            // Step 2: Bob receives the request and provides his bundle
            let bob_bundle = bob_app.encryption.create_pre_key_bundle();

            // Step 3: Alice handles Bob's bundle → gets init message + encrypted queued
            let (init_msg, encrypted_queued) = alice_app
                .encryption
                .handle_bundle_response("bob", &bob_bundle)
                .expect("bundle response");

            // One queued message should be encrypted
            assert_eq!(encrypted_queued.len(), 1);

            // Alice is now in AwaitingComplete
            assert!(alice_app.encryption.has_pending_exchange("bob"));
            assert!(!alice_app.encryption.has_session("bob"));

            // Step 4: Bob handles Alice's init message → session on Bob's side
            let complete_msg = bob_app
                .encryption
                .handle_init_message("alice", &init_msg)
                .expect("init message");
            assert!(matches!(complete_msg, KeyExchangeMessage::Complete));
            assert!(bob_app.encryption.has_session("alice"));

            // Bob can decrypt the queued message
            let decrypted = bob_app
                .encryption
                .decrypt("alice", &encrypted_queued[0])
                .expect("decrypt");
            assert_eq!(decrypted, b"hello bob!");

            // Step 5: Alice receives Complete → promote session
            alice_app.handle_key_exchange_complete("bob");
            assert!(alice_app.encryption.has_session("bob"));
            assert!(!alice_app.encryption.has_pending_exchange("bob"));

            // Step 6: Both can now encrypt/decrypt
            let ct = alice_app
                .encryption
                .encrypt("bob", b"secure msg")
                .expect("encrypt");
            let pt = bob_app.encryption.decrypt("alice", &ct).expect("decrypt");
            assert_eq!(pt, b"secure msg");

            let ct2 = bob_app
                .encryption
                .encrypt("alice", b"reply")
                .expect("encrypt");
            let pt2 = alice_app
                .encryption
                .decrypt("bob", &ct2)
                .expect("decrypt");
            assert_eq!(pt2, b"reply");
        })
        .expect("thread spawn failed")
        .join();

    result.expect("full_key_exchange_protocol_flow panicked");
}

// ── Transparent encrypt/decrypt tests ─────────────────────────

#[test]
fn transparent_encrypt_decrypt_full_cycle() {
    // Tests the full path: Alice encrypts via App, Bob decrypts via App.
    // Uses two App instances with established sessions.
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let mut alice_app = App::new(ClientConfig::default());
            let mut bob_app = App::new(ClientConfig::default());

            // Establish session between Alice and Bob
            let _request = alice_app.encryption.initiate_key_exchange("bob");
            let bob_bundle = bob_app.encryption.create_pre_key_bundle();
            let (init_msg, _queued) = alice_app
                .encryption
                .handle_bundle_response("bob", &bob_bundle)
                .expect("bundle response");
            let _complete = bob_app
                .encryption
                .handle_init_message("alice", &init_msg)
                .expect("init message");
            alice_app.handle_key_exchange_complete("bob");

            // Both sides should have active sessions
            assert!(alice_app.encryption.has_session("bob"));
            assert!(bob_app.encryption.has_session("alice"));

            // Alice encrypts a message
            let encrypted = alice_app
                .encryption
                .encrypt("bob", b"hello from alice")
                .expect("encrypt");

            // Encode for wire (as the send_encrypted_message method does)
            let encoded = encode_for_wire(&encrypted.to_bytes());

            // Bob receives and decrypts via handle_encrypted_message
            bob_app.handle_encrypted_message("alice", &encoded);

            // Verify message appears in Bob's query buffer for alice
            let query_buf = bob_app
                .view
                .buffers()
                .get(&crate::tui::buffer_manager::BufferId::Query("alice".into()))
                .expect("query buffer should exist");
            let last_line = query_buf.iter_lines().last().expect("should have a line");
            assert_eq!(last_line.content, "hello from alice");
            assert_eq!(last_line.sender, Some("alice".to_string()));
            assert_eq!(last_line.line_type, LineType::Message);
        })
        .expect("thread spawn failed")
        .join();

    result.expect("transparent_encrypt_decrypt_full_cycle panicked");
}

#[test]
fn decryption_failure_shows_error_in_query_buffer() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // Create a structurally valid but undecryptable EncryptedMessage
    let fake_encrypted = pirc_crypto::message::EncryptedMessage {
        encrypted_header: vec![0xAA; 64],
        header_nonce: [0x11; 12],
        ciphertext: vec![0xBB; 128],
        body_nonce: [0x22; 12],
    };
    let encoded = encode_for_wire(&fake_encrypted.to_bytes());

    app.handle_encrypted_message("alice", &encoded);

    // Error should appear in the query buffer for "alice", not status
    let query_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Query("alice".into()))
        .expect("query buffer for alice should exist");
    let last_line = query_buf.iter_lines().last().expect("should have a line");
    assert!(
        last_line.content.contains("Failed to decrypt"),
        "expected decrypt error, got: {}",
        last_line.content
    );
    assert_eq!(last_line.line_type, LineType::Error);

    // Status buffer should NOT have the error (App::new doesn't push status messages)
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    assert_eq!(status_buf.len(), 0);
}

#[test]
fn outbound_message_requires_connection() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Without a connection, handle_private_msg_command returns early
    rt.block_on(app.handle_private_msg_command("bob", "secret message"));

    // No query buffer created (returned before echo)
    let query_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Query("bob".into()));
    assert!(query_buf.is_none());

    // Status shows "Not connected"
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("Not connected"),
        "expected 'Not connected', got: {}",
        last_line.content
    );
}

#[test]
fn channel_message_not_encrypted() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // /msg #channel hello — should not go through encryption
    let cmd = ClientCommand::Msg("#channel".into(), "hello".into());
    rt.block_on(app.handle_command(cmd));

    // No pending exchange for the channel
    assert!(!app.encryption.has_pending_exchange("#channel"));
    assert!(!app.encryption.has_session("#channel"));
}

#[test]
fn transparent_bidirectional_message_exchange() {
    // Tests messages flowing in both directions after session establishment.
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let mut alice_app = App::new(ClientConfig::default());
            let mut bob_app = App::new(ClientConfig::default());

            // Establish session
            let _request = alice_app.encryption.initiate_key_exchange("bob");
            let bob_bundle = bob_app.encryption.create_pre_key_bundle();
            let (init_msg, _) = alice_app
                .encryption
                .handle_bundle_response("bob", &bob_bundle)
                .expect("bundle response");
            let _complete = bob_app
                .encryption
                .handle_init_message("alice", &init_msg)
                .expect("init message");
            alice_app.handle_key_exchange_complete("bob");

            // Alice sends to Bob
            let ct1 = alice_app
                .encryption
                .encrypt("bob", b"msg 1")
                .expect("encrypt");
            let wire1 = encode_for_wire(&ct1.to_bytes());
            bob_app.handle_encrypted_message("alice", &wire1);

            // Bob sends to Alice
            let ct2 = bob_app
                .encryption
                .encrypt("alice", b"msg 2")
                .expect("encrypt");
            let wire2 = encode_for_wire(&ct2.to_bytes());
            alice_app.handle_encrypted_message("bob", &wire2);

            // Alice sends another message to Bob
            let ct3 = alice_app
                .encryption
                .encrypt("bob", b"msg 3")
                .expect("encrypt");
            let wire3 = encode_for_wire(&ct3.to_bytes());
            bob_app.handle_encrypted_message("alice", &wire3);

            // Verify Bob's query buffer has both messages from Alice
            let bob_query = bob_app
                .view
                .buffers()
                .get(&crate::tui::buffer_manager::BufferId::Query("alice".into()))
                .expect("bob should have alice query buffer");
            let lines: Vec<_> = bob_query.iter_lines().collect();
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0].content, "msg 1");
            assert_eq!(lines[1].content, "msg 3");

            // Verify Alice's query buffer has the message from Bob
            let alice_query = alice_app
                .view
                .buffers()
                .get(&crate::tui::buffer_manager::BufferId::Query("bob".into()))
                .expect("alice should have bob query buffer");
            let lines: Vec<_> = alice_query.iter_lines().collect();
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].content, "msg 2");
        })
        .expect("thread spawn failed")
        .join();

    result.expect("transparent_bidirectional_message_exchange panicked");
}

#[test]
fn handle_encrypted_message_invalid_base64_does_not_panic() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // Not valid base64 — should be silently ignored (logged, no crash)
    app.handle_encrypted_message("alice", "not-valid-base64!!!");

    // No query buffer should be created for alice
    let query_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Query("alice".into()));
    assert!(query_buf.is_none());
}

#[test]
fn query_with_message_routes_through_encryption() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // /query bob hello — should route through encryption
    let cmd = ClientCommand::Query("bob".into(), Some("hello".into()));
    rt.block_on(app.handle_command(cmd));

    // Should show "Not connected" (no connection)
    let status_buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap();
    let last_line = status_buf.iter_lines().last().unwrap();
    assert!(
        last_line.content.contains("Not connected"),
        "expected 'Not connected', got: {}",
        last_line.content
    );
}
