use super::*;
use crate::config::ClientConfig;

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
    assert!(app.view.buffers().get(&crate::tui::buffer_manager::BufferId::Status).unwrap().len() > 0);
}

#[test]
fn dispatch_quit_returns_true() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(app.dispatch_view_action(ViewAction::Quit));
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
        vec![
            "testuser".into(),
            "Welcome to the test network!".into(),
        ],
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
        vec![
            "servernick".into(),
            "Welcome!".into(),
        ],
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
        "user".into(), vec![], "user".into(), "user".into(),
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
        vec!["*".into(), "mynick".into(), "Nickname is already in use".into()],
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

    let msg = Message::new(
        pirc_protocol::Command::Ping,
        vec!["server".into()],
    );

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

    let msg = Message::new(
        pirc_protocol::Command::Pong,
        vec!["pirc-12345".into()],
    );

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

    let msg = Message::new(
        pirc_protocol::Command::Pong,
        vec!["something".into()],
    );

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
        .get(&crate::tui::buffer_manager::BufferId::Channel("#test".into()))
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
        "user".into(), vec![], "user".into(), "user".into(),
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
