use pirc_common::types::GroupId;

use crate::app::App;
use crate::config::ClientConfig;
use crate::tui::buffer_manager::BufferId;

fn last_status_line(app: &App) -> String {
    let status_buf = app
        .view
        .buffers()
        .get(&BufferId::Status)
        .unwrap();
    status_buf
        .iter_lines()
        .last()
        .map(|l| l.content.clone())
        .unwrap_or_default()
}

// ── Group message routing ──────────────────────────────────────

#[test]
fn group_buffer_message_routes_through_group_chat_manager() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let group_id = GroupId::new(42);
    app.group_chat.add_group(group_id);

    // Open the group buffer so it exists
    let target = BufferId::Channel(format!("group:{}", group_id.as_u64()));
    app.view.buffers_mut().ensure_open(target.clone());

    // Send a chat message to the group buffer
    rt.block_on(app.handle_chat_message("hello group", &target));

    // The message should have gone through GroupChatManager::send_message,
    // which fails because there are no members with ready encryption sessions.
    // This proves the group path was taken (not the raw PRIVMSG path which
    // would show "Not connected").
    let status = last_status_line(&app);
    assert!(
        status.contains("Group send error"),
        "expected 'Group send error' (proving GroupChatManager path), got: {status}"
    );
}

#[test]
fn non_group_channel_message_sends_privmsg() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Regular channel buffer (not a group)
    let target = BufferId::Channel("#general".into());
    app.view.buffers_mut().ensure_open(target.clone());

    rt.block_on(app.handle_chat_message("hello channel", &target));

    // Should go through the raw PRIVMSG path which fails with "Not connected"
    // since there's no server connection.
    let status = last_status_line(&app);
    assert!(
        status.contains("Not connected"),
        "expected 'Not connected' (raw PRIVMSG path), got: {status}"
    );
}

#[test]
fn group_buffer_message_unregistered_group_shows_error() {
    let config = ClientConfig::default();
    let mut app = App::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Group buffer exists but group is NOT registered in GroupChatManager
    let target = BufferId::Channel("group:999".into());
    app.view.buffers_mut().ensure_open(target.clone());

    rt.block_on(app.handle_chat_message("hello", &target));

    // Should show "group not found" error from GroupChatManager
    let status = last_status_line(&app);
    assert!(
        status.contains("Group send error"),
        "expected 'Group send error', got: {status}"
    );
    assert!(
        status.contains("not found"),
        "expected error to mention 'not found', got: {status}"
    );
}
