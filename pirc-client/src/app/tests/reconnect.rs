use crate::app::*;
use crate::config::ClientConfig;

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
