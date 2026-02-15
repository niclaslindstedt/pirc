use super::*;
use crate::config::ClientConfig;
use pirc_crypto::protocol::{decode_from_wire, encode_for_wire, KeyExchangeMessage};

#[test]
fn app_new_creates_with_defaults() {
    let config = ClientConfig::default();
    let app = App::new(config);
    assert!(app.connection.is_none());
    assert!(!app.connection_mgr.is_connected());
}

#[test]
fn app_new_uses_config_nick() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("testuser".to_string());
    let app = App::new(config);
    assert_eq!(app.connection_mgr.nick(), "testuser");
}

#[test]
fn current_timestamp_formats_hm() {
    let ts = current_timestamp("%H:%M");
    assert!(ts.contains(':'), "timestamp should contain a colon: {ts}");
    assert_eq!(ts.len(), 5, "HH:MM should be 5 chars: {ts}");
}

#[test]
fn current_timestamp_fallback() {
    let ts = current_timestamp("custom");
    // Should be epoch seconds since format doesn't match
    assert!(ts.parse::<u64>().is_ok());
}

#[test]
fn handle_disconnect_clears_connection() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    let mut app = App::new(config);

    // Manually force into Connecting then Registering states
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    app.handle_disconnect("test disconnect");
    assert!(app.connection.is_none());
    assert!(!app.connection_mgr.is_connected());
}

#[test]
fn push_status_adds_to_status_buffer() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.push_status("hello world");
    assert!(
        app.view
            .buffers()
            .get(&crate::tui::buffer_manager::BufferId::Status)
            .unwrap()
            .len()
            > 0
    );
}

#[test]
fn dispatch_quit_returns_true() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(app.dispatch_view_action(ViewAction::Quit(None)));
    assert!(result);
}

#[test]
fn dispatch_none_returns_false() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(app.dispatch_view_action(ViewAction::None));
    assert!(!result);
}

#[test]
fn dispatch_redraw_returns_false() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(app.dispatch_view_action(ViewAction::Redraw));
    assert!(!result);
}

#[test]
fn dispatch_command_error_returns_false() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = crate::client_command::CommandError::MissingArgument {
        command: "join".into(),
        argument: "channel".into(),
    };
    let result = rt.block_on(app.dispatch_view_action(ViewAction::CommandError(err)));
    assert!(!result);
}

#[test]
fn handle_server_message_rpl_welcome() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("testuser".to_string());
    let mut app = App::new(config);

    // Must be in Registering state to transition to Connected
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();

    // Set up registration state (as initiate_connection would)
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
    rt.block_on(app.handle_server_message(&msg));
    assert!(app.connection_mgr.is_connected());
    assert_eq!(app.connection_mgr.server_name(), Some("irc.test.net"));
    assert!(app.registration.is_none());
    assert!(app.registration_deadline.is_none());
}

#[test]
fn handle_server_message_rpl_welcome_updates_nick() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("mynick".to_string());
    let mut app = App::new(config);

    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();

    app.registration = Some(RegistrationState::new(
        "mynick".into(),
        vec![],
        "mynick".into(),
        "mynick".into(),
    ));

    let msg = Message::with_prefix(
        pirc_protocol::Prefix::Server("irc.test.net".into()),
        pirc_protocol::Command::Numeric(1),
        vec!["servernick".into(), "Welcome!".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));
    assert_eq!(app.connection_mgr.nick(), "servernick");
}

#[test]
fn handle_server_message_info_numerics() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    let mut app = App::new(config);

    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.registration = Some(RegistrationState::new(
        "user".into(),
        vec![],
        "user".into(),
        "user".into(),
    ));

    let initial_count = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap()
        .len();

    let msg = Message::new(
        pirc_protocol::Command::Numeric(2),
        vec!["user".into(), "Your host is irc.test.net".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    let new_count = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap()
        .len();
    assert_eq!(new_count, initial_count + 1);
}

#[test]
fn handle_server_message_nick_in_use() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("mynick".to_string());
    config.identity.alt_nicks = vec!["alt1".into()];
    let mut app = App::new(config);

    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.registration = Some(RegistrationState::new(
        "mynick".into(),
        vec!["alt1".into()],
        "mynick".into(),
        "mynick".into(),
    ));

    let msg = Message::new(
        pirc_protocol::Command::Numeric(433),
        vec![
            "*".into(),
            "mynick".into(),
            "Nickname is already in use".into(),
        ],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    // Nick should have been updated to alt1
    assert_eq!(app.connection_mgr.nick(), "alt1");
    // Still registering
    assert!(!app.connection_mgr.is_connected());
    assert!(app.registration.is_some());
}

#[test]
fn handle_server_message_ping_no_buffer_output() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let msg = Message::new(pirc_protocol::Command::Ping, vec!["server".into()]);

    let initial_count = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap()
        .len();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    let new_count = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Status)
        .unwrap()
        .len();

    // PING is transport-level and should not produce buffer output
    assert_eq!(new_count, initial_count);
}

#[test]
fn handle_server_message_pong_updates_lag() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // Simulate having sent a PING
    app.ping_sent_at = Some(Instant::now() - Duration::from_millis(42));

    let msg = Message::new(pirc_protocol::Command::Pong, vec!["pirc-12345".into()]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    // Lag should now be set
    assert!(app.lag_ms.is_some());
    // Should have cleared ping_sent_at
    assert!(app.ping_sent_at.is_none());
}

#[test]
fn handle_server_message_pong_without_ping_is_ignored() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    // No PING was sent
    assert!(app.ping_sent_at.is_none());

    let msg = Message::new(pirc_protocol::Command::Pong, vec!["something".into()]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    // Lag should remain None
    assert!(app.lag_ms.is_none());
}

#[test]
fn handle_disconnect_clears_keepalive_state() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    let mut app = App::new(config);

    // Set up some keepalive state
    app.last_message_received = Some(Instant::now());
    app.ping_sent_at = Some(Instant::now());
    app.lag_ms = Some(42);

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    app.handle_disconnect("test disconnect");

    assert!(app.last_message_received.is_none());
    assert!(app.ping_sent_at.is_none());
    assert!(app.lag_ms.is_none());
}

#[test]
fn keepalive_tick_not_connected_does_nothing() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_keepalive_tick());

    // Should not have set ping_sent_at since not connected
    assert!(app.ping_sent_at.is_none());
}

#[test]
fn keepalive_tick_skips_when_recently_active() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    let mut app = App::new(config);

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    // Recently received a message
    app.last_message_received = Some(Instant::now());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_keepalive_tick());

    // Should not have sent a PING since we recently received data
    assert!(app.ping_sent_at.is_none());
}

#[test]
fn handle_server_message_privmsg_to_channel() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let msg = Message::with_prefix(
        pirc_protocol::Prefix::user("alice", "alice", "host.com"),
        pirc_protocol::Command::Privmsg,
        vec!["#test".into(), "hello world".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    // Channel buffer should have been created and contain the message
    let buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Channel(
            "#test".into(),
        ))
        .expect("channel buffer should exist");
    assert_eq!(buf.len(), 1);
}

#[test]
fn handle_server_message_privmsg_query() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("mynick".into());
    let mut app = App::new(config);

    let msg = Message::with_prefix(
        pirc_protocol::Prefix::user("bob", "bob", "host.com"),
        pirc_protocol::Command::Privmsg,
        vec!["mynick".into(), "hey there".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    // Query buffer should have been auto-created
    let buf = app
        .view
        .buffers()
        .get(&crate::tui::buffer_manager::BufferId::Query("bob".into()))
        .expect("query buffer should exist");
    assert_eq!(buf.len(), 1);
}

#[test]
fn handle_server_message_nick_change_updates_state() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("oldnick".into());
    let mut app = App::new(config);

    let msg = Message::with_prefix(
        pirc_protocol::Prefix::user("oldnick", "oldnick", "host.com"),
        pirc_protocol::Command::Nick,
        vec!["newnick".into()],
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_server_message(&msg));

    assert_eq!(app.connection_mgr.nick(), "newnick");
}

#[test]
fn handle_disconnect_clears_registration() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    let mut app = App::new(config);

    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.registration = Some(RegistrationState::new(
        "user".into(),
        vec![],
        "user".into(),
        "user".into(),
    ));
    app.registration_deadline = Some(Instant::now() + REGISTRATION_TIMEOUT);

    app.handle_disconnect("test disconnect");
    assert!(app.registration.is_none());
    assert!(app.registration_deadline.is_none());
    assert!(app.connection.is_none());
}

#[test]
fn render_input_line_works() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("nick".to_string());
    let app = App::new(config);

    let mut buf = Buffer::new(80, 24);
    app.render_input_line(&mut buf);

    // Check that the prompt is rendered
    let cell = buf.get(0, 23);
    assert_eq!(cell.ch, '[');
}

// ── Reconnect tests ──────────────────────────────────────────

#[test]
fn handle_disconnect_schedules_reconnect_when_enabled() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = true;
    config.server.reconnect_delay_secs = 5;
    let mut app = App::new(config);

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    app.handle_disconnect("Connection lost");

    // Should have scheduled a reconnect
    assert!(app.reconnect_at.is_some());
    assert_eq!(app.reconnect_attempt, 1);
    assert_eq!(
        *app.connection_mgr.state(),
        ConnectionState::Reconnecting { attempt: 1 }
    );
}

#[test]
fn handle_disconnect_no_reconnect_when_disabled() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = false;
    let mut app = App::new(config);

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    app.handle_disconnect("Connection lost");

    // Should NOT have scheduled a reconnect
    assert!(app.reconnect_at.is_none());
    assert_eq!(app.reconnect_attempt, 0);
    assert_eq!(*app.connection_mgr.state(), ConnectionState::Disconnected);
}

#[test]
fn schedule_reconnect_exponential_backoff() {
    let mut config = ClientConfig::default();
    config.server.reconnect_delay_secs = 5;
    let mut app = App::new(config);

    // Attempt 1: delay = 5 * 2^0 = 5s
    app.schedule_reconnect(1);
    let at1 = app.reconnect_at.unwrap();
    assert_eq!(app.reconnect_attempt, 1);

    // Attempt 2: delay = 5 * 2^1 = 10s
    // Reset state for next schedule
    let _ = app.connection_mgr.transition(ConnectionState::Disconnected);
    app.schedule_reconnect(2);
    let at2 = app.reconnect_at.unwrap();
    assert_eq!(app.reconnect_attempt, 2);
    // at2 should be further in the future than at1 was
    assert!(at2 > at1);

    // Attempt 5: delay = 5 * 2^4 = 80s → capped at 60s
    let _ = app.connection_mgr.transition(ConnectionState::Disconnected);
    app.schedule_reconnect(5);
    assert_eq!(app.reconnect_attempt, 5);
}

#[test]
fn handle_disconnect_captures_channels_for_rejoin() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = true;
    let mut app = App::new(config);

    // Open some channel buffers
    app.view
        .buffers_mut()
        .ensure_open(crate::tui::buffer_manager::BufferId::Channel(
            "#rust".into(),
        ));
    app.view
        .buffers_mut()
        .ensure_open(crate::tui::buffer_manager::BufferId::Channel(
            "#pirc".into(),
        ));

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    app.handle_disconnect("Connection lost");

    // Should have captured channels
    assert!(app.channels_to_rejoin.contains(&"#rust".to_string()));
    assert!(app.channels_to_rejoin.contains(&"#pirc".to_string()));
}

#[test]
fn disconnect_command_disables_auto_reconnect() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = true;
    let mut app = App::new(config);

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_disconnect_command());

    assert!(!app.connection_mgr.auto_reconnect());
    assert!(app.reconnect_at.is_none());
    assert_eq!(*app.connection_mgr.state(), ConnectionState::Disconnected);
}

#[test]
fn disconnect_command_cancels_pending_reconnect() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = true;
    let mut app = App::new(config);

    // Simulate being in reconnecting state
    app.connection_mgr
        .transition(ConnectionState::Reconnecting { attempt: 3 })
        .unwrap();
    app.reconnect_at = Some(Instant::now() + Duration::from_secs(30));
    app.reconnect_attempt = 3;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_disconnect_command());

    assert!(!app.connection_mgr.auto_reconnect());
    assert!(app.reconnect_at.is_none());
    assert_eq!(app.reconnect_attempt, 0);
    assert_eq!(*app.connection_mgr.state(), ConnectionState::Disconnected);
}

#[test]
fn reconnect_command_when_already_connected() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    let mut app = App::new(config);

    // Force into connected state
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Connected {
            server_name: "test".into(),
        })
        .unwrap();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_reconnect_command());

    // Should still be connected (no-op)
    assert!(app.connection_mgr.is_connected());
}

#[test]
fn reconnect_command_schedules_reconnect() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = false;
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_reconnect_command());

    // Should have re-enabled auto-reconnect and scheduled
    assert!(app.connection_mgr.auto_reconnect());
    assert!(app.reconnect_at.is_some());
    assert_eq!(app.reconnect_attempt, 1);
}

#[test]
fn handle_disconnect_continues_attempt_sequence() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = true;
    let mut app = App::new(config);

    // Simulate being mid-reconnect (attempt 3 failed during registration)
    app.reconnect_attempt = 3;
    app.connection_mgr
        .transition(ConnectionState::Connecting)
        .unwrap();
    app.connection_mgr
        .transition(ConnectionState::Registering)
        .unwrap();

    app.handle_disconnect("Registration timed out");

    // Should continue from attempt 4, not restart at 1
    assert_eq!(app.reconnect_attempt, 4);
}

#[test]
fn app_new_has_no_reconnect_state() {
    let config = ClientConfig::default();
    let app = App::new(config);
    assert!(app.reconnect_at.is_none());
    assert_eq!(app.reconnect_attempt, 0);
    assert!(app.channels_to_rejoin.is_empty());
}

// ── Quit tests ───────────────────────────────────────────────

#[test]
fn dispatch_quit_with_reason_returns_true() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(app.dispatch_view_action(ViewAction::Quit(Some("goodbye".into()))));
    assert!(result);
}

#[test]
fn handle_quit_disables_auto_reconnect() {
    let mut config = ClientConfig::default();
    config.identity.nick = Some("user".to_string());
    config.server.auto_reconnect = true;
    let mut app = App::new(config);

    // Simulate being connected with a pending reconnect
    app.reconnect_at = Some(Instant::now() + Duration::from_secs(30));
    app.reconnect_attempt = 2;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(app.handle_quit(None));

    assert!(!app.connection_mgr.auto_reconnect());
    assert!(app.reconnect_at.is_none());
    assert_eq!(app.reconnect_attempt, 0);
}

#[test]
fn handle_quit_no_connection_does_not_panic() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // Should not panic even without a connection
    rt.block_on(app.handle_quit(Some("leaving".into())));
    assert!(!app.connection_mgr.auto_reconnect());
}

#[test]
fn dispatch_quit_none_returns_true_and_disables_reconnect() {
    let mut config = ClientConfig::default();
    config.server.auto_reconnect = true;
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(app.dispatch_view_action(ViewAction::Quit(None)));
    assert!(result);
    assert!(!app.connection_mgr.auto_reconnect());
}

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
