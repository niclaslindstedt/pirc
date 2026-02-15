use std::collections::HashMap;

use crate::client_command::{ClientCommand, CommandError};
use crate::encryption::EncryptionStatus;
use crate::tui::buffer::Buffer;
use crate::tui::buffer_manager::{BufferId, BufferManager};
use crate::tui::chat_renderer::render_chat_area;
use crate::tui::input::KeyEvent;
use crate::tui::input_handler::{InputAction, InputHandler};
use crate::tui::layout::Layout;
use crate::tui::message_buffer::BufferLine;
use crate::tui::scrollback_search::{render_search_bar, SearchState};
use crate::tui::status_bar::{render_status_bar, StatusBarInfo};
use crate::tui::tab_bar::{render_tab_bar, TabInfo};

/// An action produced by the view coordinator for the event loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewAction {
    /// No action needed.
    None,
    /// The view changed and needs a redraw.
    Redraw,
    /// A command to send to the server.
    Command(ClientCommand),
    /// A chat message to send to the given buffer's target.
    ChatMessage(String, BufferId),
    /// A command parse error to display.
    CommandError(CommandError),
    /// The user requested quit, with an optional reason for the QUIT message.
    Quit(Option<String>),
}

/// Integrates all TUI subcomponents into a unified view layer.
///
/// The event loop delegates input handling and rendering to ViewCoordinator,
/// which routes events to the appropriate subcomponent (input handler, search,
/// buffer manager) and orchestrates rendering of all UI regions.
pub struct ViewCoordinator {
    buffers: BufferManager,
    input: InputHandler,
    search: SearchState,
    layout: Option<Layout>,
    width: u16,
    height: u16,
    nick: String,
    lag: Option<u32>,
    encryption_states: HashMap<String, EncryptionStatus>,
}

impl ViewCoordinator {
    /// Create a new view coordinator with the given terminal dimensions.
    pub fn new(width: u16, height: u16, scrollback_capacity: usize) -> Self {
        let layout = Layout::compute(width, height, 0);
        Self {
            buffers: BufferManager::new(scrollback_capacity),
            input: InputHandler::new(),
            search: SearchState::new(),
            layout,
            width,
            height,
            nick: "pirc_user".to_string(),
            lag: None,
            encryption_states: HashMap::new(),
        }
    }

    /// Handle a terminal resize.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.layout = Layout::compute(width, height, 0);
    }

    /// Set the current nick for status bar display.
    pub fn set_nick(&mut self, nick: String) {
        self.nick = nick;
    }

    /// Set the current server lag for status bar display.
    pub fn set_lag(&mut self, lag: Option<u32>) {
        self.lag = lag;
    }

    /// Update the encryption status for a peer (query buffer nick).
    pub fn set_encryption_status(&mut self, peer: String, status: EncryptionStatus) {
        if status == EncryptionStatus::None {
            self.encryption_states.remove(&peer);
        } else {
            self.encryption_states.insert(peer, status);
        }
    }

    /// Return a reference to the buffer manager.
    pub fn buffers(&self) -> &BufferManager {
        &self.buffers
    }

    /// Return a mutable reference to the buffer manager.
    pub fn buffers_mut(&mut self) -> &mut BufferManager {
        &mut self.buffers
    }

    /// Return a reference to the input handler.
    pub fn input(&self) -> &InputHandler {
        &self.input
    }

    /// Return a mutable reference to the input handler.
    pub fn input_mut(&mut self) -> &mut InputHandler {
        &mut self.input
    }

    /// Return true if search mode is active.
    pub fn is_search_active(&self) -> bool {
        self.search.is_active()
    }

    /// Process an input action from the input handler and produce a view action.
    pub fn handle_input_action(&mut self, action: InputAction) -> ViewAction {
        match action {
            InputAction::None => ViewAction::None,
            InputAction::Redraw => ViewAction::Redraw,
            InputAction::Quit(reason) => ViewAction::Quit(reason),
            InputAction::ScrollUp => {
                self.buffers.active_mut().scroll_up(10);
                ViewAction::Redraw
            }
            InputAction::ScrollDown => {
                self.buffers.active_mut().scroll_down(10);
                ViewAction::Redraw
            }
            InputAction::Resize(w, h) => {
                self.resize(w, h);
                ViewAction::Redraw
            }
            InputAction::Command(cmd) => ViewAction::Command(cmd),
            InputAction::ChatMessage(msg) => {
                let id = self.buffers.active_id().clone();
                ViewAction::ChatMessage(msg, id)
            }
            InputAction::CommandError(err) => ViewAction::CommandError(err),
        }
    }

    /// Handle a raw key event, routing through input handler or search.
    pub fn handle_key(&mut self, event: KeyEvent) -> ViewAction {
        if self.search.is_active() {
            return self.handle_search_key(event);
        }
        // Ctrl+F enters search mode.
        if event == KeyEvent::Ctrl('f') {
            self.start_search();
            return ViewAction::Redraw;
        }
        let action = self.input.handle_key(event);
        self.handle_input_action(action)
    }

    /// Switch to the next buffer.
    pub fn switch_next_buffer(&mut self) {
        self.buffers.switch_next();
        self.search.deactivate();
    }

    /// Switch to the previous buffer.
    pub fn switch_prev_buffer(&mut self) {
        self.buffers.switch_prev();
        self.search.deactivate();
    }

    /// Switch to a buffer by its position number (0-indexed).
    pub fn switch_buffer_by_number(&mut self, n: usize) -> bool {
        let result = self.buffers.switch_by_index(n);
        if result {
            self.search.deactivate();
        }
        result
    }

    /// Enter search mode for the active buffer.
    pub fn start_search(&mut self) {
        self.search.activate();
    }

    /// Handle a key event during search mode.
    pub fn handle_search_key(&mut self, event: KeyEvent) -> ViewAction {
        match event {
            KeyEvent::Escape => {
                self.search.deactivate();
                ViewAction::Redraw
            }
            KeyEvent::Enter => {
                self.search.next_match();
                self.search.deactivate();
                ViewAction::Redraw
            }
            KeyEvent::Ctrl('n') => {
                self.search.next_match();
                ViewAction::Redraw
            }
            KeyEvent::Ctrl('p') => {
                self.search.prev_match();
                ViewAction::Redraw
            }
            KeyEvent::Backspace => {
                self.search.query_mut().backspace();
                self.search.update_query(self.buffers.active());
                ViewAction::Redraw
            }
            KeyEvent::Char(ch) => {
                self.search.query_mut().insert_char(ch);
                self.search.update_query(self.buffers.active());
                ViewAction::Redraw
            }
            _ => ViewAction::None,
        }
    }

    /// Render all UI components into the given screen buffer.
    pub fn render(&mut self, buf: &mut Buffer) {
        let layout = match self.layout {
            Some(l) => l,
            None => return,
        };

        // Tab bar
        let active_id = self.buffers.active_id().clone();
        let tabs: Vec<TabInfo> = self
            .buffers
            .buffer_list()
            .into_iter()
            .map(|(id, label, unread, activity)| {
                let encryption_status = match &id {
                    BufferId::Query(nick) => self
                        .encryption_states
                        .get(nick)
                        .copied()
                        .unwrap_or(EncryptionStatus::None),
                    _ => EncryptionStatus::None,
                };
                TabInfo {
                    label: label.to_string(),
                    is_active: id == active_id,
                    unread_count: unread,
                    has_activity: activity,
                    encryption_status,
                }
            })
            .collect();
        render_tab_bar(buf, &layout.channel_tabs, &tabs);

        // Chat area
        let nick_width = 12;
        render_chat_area(
            buf,
            &layout.chat_area,
            self.buffers.active_mut(),
            nick_width,
        );

        // Status bar
        let encryption_status = match &active_id {
            BufferId::Query(nick) => self
                .encryption_states
                .get(nick)
                .copied()
                .unwrap_or(EncryptionStatus::None),
            _ => EncryptionStatus::None,
        };
        let info = StatusBarInfo {
            nick: self.nick.clone(),
            buffer_label: active_id.to_string(),
            buffer_id: active_id,
            topic: None,
            user_count: None,
            lag: self.lag,
            away: false,
            scroll_info: None,
            encryption_status,
        };
        render_status_bar(buf, &layout.status_bar, &info);

        // Search bar overlay (renders on top of status bar row if active)
        if self.search.is_active() {
            render_search_bar(buf, &layout.status_bar, &self.search);
        }
    }

    /// Push a message to a specific buffer by target name.
    pub fn push_message(&mut self, target: &BufferId, line: BufferLine) {
        self.buffers.push_to(target, line);
    }

    /// Push a status message to the Status buffer.
    pub fn push_status_message(&mut self, line: BufferLine) {
        self.buffers.push_to(&BufferId::Status, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::message_buffer::LineType;

    fn make_line(content: &str) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: Some("nick".to_string()),
            content: content.to_string(),
            line_type: LineType::Message,
        }
    }

    // ── Construction ─────────────────────────────────────────────

    #[test]
    fn new_creates_with_status_buffer() {
        let vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(*vc.buffers().active_id(), BufferId::Status);
        assert_eq!(vc.buffers().buffer_count(), 1);
    }

    #[test]
    fn new_computes_layout() {
        let vc = ViewCoordinator::new(80, 24, 100);
        assert!(vc.layout.is_some());
    }

    #[test]
    fn new_too_small_no_layout() {
        let vc = ViewCoordinator::new(10, 5, 100);
        assert!(vc.layout.is_none());
    }

    // ── Resize ───────────────────────────────────────────────────

    #[test]
    fn resize_updates_dimensions() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.resize(120, 40);
        assert_eq!(vc.width, 120);
        assert_eq!(vc.height, 40);
        assert!(vc.layout.is_some());
    }

    #[test]
    fn resize_to_too_small_clears_layout() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.resize(10, 5);
        assert!(vc.layout.is_none());
    }

    // ── Input action routing ─────────────────────────────────────

    #[test]
    fn handle_none_action() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(vc.handle_input_action(InputAction::None), ViewAction::None);
    }

    #[test]
    fn handle_redraw_action() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(
            vc.handle_input_action(InputAction::Redraw),
            ViewAction::Redraw
        );
    }

    #[test]
    fn handle_quit_action() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(
            vc.handle_input_action(InputAction::Quit(None)),
            ViewAction::Quit(None)
        );
    }

    #[test]
    fn handle_quit_action_with_reason() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(
            vc.handle_input_action(InputAction::Quit(Some("bye".into()))),
            ViewAction::Quit(Some("bye".into()))
        );
    }

    #[test]
    fn handle_command_action() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        let cmd = ClientCommand::Join("#test".into());
        assert_eq!(
            vc.handle_input_action(InputAction::Command(cmd.clone())),
            ViewAction::Command(cmd)
        );
    }

    #[test]
    fn handle_chat_message_uses_active_buffer_id() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#test".into()));
        let action = vc.handle_input_action(InputAction::ChatMessage("hello".into()));
        assert_eq!(
            action,
            ViewAction::ChatMessage("hello".into(), BufferId::Channel("#test".into()))
        );
    }

    #[test]
    fn handle_chat_message_on_status_buffer() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        let action = vc.handle_input_action(InputAction::ChatMessage("hello".into()));
        assert_eq!(
            action,
            ViewAction::ChatMessage("hello".into(), BufferId::Status)
        );
    }

    #[test]
    fn handle_command_error_action() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        let err = CommandError::MissingArgument {
            command: "join".into(),
            argument: "channel".into(),
        };
        assert_eq!(
            vc.handle_input_action(InputAction::CommandError(err.clone())),
            ViewAction::CommandError(err)
        );
    }

    #[test]
    fn handle_scroll_up_redraws() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(
            vc.handle_input_action(InputAction::ScrollUp),
            ViewAction::Redraw
        );
    }

    #[test]
    fn handle_scroll_down_redraws() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(
            vc.handle_input_action(InputAction::ScrollDown),
            ViewAction::Redraw
        );
    }

    #[test]
    fn handle_resize_action() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        let action = vc.handle_input_action(InputAction::Resize(120, 40));
        assert_eq!(action, ViewAction::Redraw);
        assert_eq!(vc.width, 120);
        assert_eq!(vc.height, 40);
    }

    // ── Buffer switching ─────────────────────────────────────────

    #[test]
    fn switch_next_buffer_cycles() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#a".into()));
        vc.buffers_mut().open(BufferId::Channel("#b".into()));
        vc.buffers_mut().switch_to(&BufferId::Status);

        vc.switch_next_buffer();
        assert_eq!(*vc.buffers().active_id(), BufferId::Channel("#a".into()));
        vc.switch_next_buffer();
        assert_eq!(*vc.buffers().active_id(), BufferId::Channel("#b".into()));
    }

    #[test]
    fn switch_prev_buffer_cycles() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#a".into()));
        vc.buffers_mut().switch_to(&BufferId::Status);

        vc.switch_prev_buffer();
        assert_eq!(*vc.buffers().active_id(), BufferId::Channel("#a".into()));
    }

    #[test]
    fn switch_buffer_by_number() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#a".into()));
        vc.buffers_mut().open(BufferId::Channel("#b".into()));

        assert!(vc.switch_buffer_by_number(0));
        assert_eq!(*vc.buffers().active_id(), BufferId::Status);

        assert!(vc.switch_buffer_by_number(1));
        assert_eq!(*vc.buffers().active_id(), BufferId::Channel("#a".into()));

        assert!(!vc.switch_buffer_by_number(10));
    }

    #[test]
    fn switch_buffer_deactivates_search() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#a".into()));
        vc.start_search();
        assert!(vc.is_search_active());

        vc.switch_next_buffer();
        assert!(!vc.is_search_active());
    }

    // ── Search mode ──────────────────────────────────────────────

    #[test]
    fn start_search_activates() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert!(!vc.is_search_active());
        vc.start_search();
        assert!(vc.is_search_active());
    }

    #[test]
    fn search_escape_deactivates() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.start_search();
        let action = vc.handle_search_key(KeyEvent::Escape);
        assert_eq!(action, ViewAction::Redraw);
        assert!(!vc.is_search_active());
    }

    #[test]
    fn search_char_input() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.start_search();
        let action = vc.handle_search_key(KeyEvent::Char('h'));
        assert_eq!(action, ViewAction::Redraw);
        assert_eq!(vc.search.query_str(), "h");
    }

    #[test]
    fn search_backspace() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.start_search();
        vc.handle_search_key(KeyEvent::Char('a'));
        vc.handle_search_key(KeyEvent::Char('b'));
        vc.handle_search_key(KeyEvent::Backspace);
        assert_eq!(vc.search.query_str(), "a");
    }

    #[test]
    fn search_enter_deactivates() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.start_search();
        let action = vc.handle_search_key(KeyEvent::Enter);
        assert_eq!(action, ViewAction::Redraw);
        assert!(!vc.is_search_active());
    }

    // ── Key handling with search routing ─────────────────────────

    #[test]
    fn ctrl_f_enters_search() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        let action = vc.handle_key(KeyEvent::Ctrl('f'));
        assert_eq!(action, ViewAction::Redraw);
        assert!(vc.is_search_active());
    }

    #[test]
    fn key_routes_to_search_when_active() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.start_search();
        let action = vc.handle_key(KeyEvent::Char('x'));
        assert_eq!(action, ViewAction::Redraw);
        assert_eq!(vc.search.query_str(), "x");
    }

    #[test]
    fn key_routes_to_input_when_search_inactive() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        let action = vc.handle_key(KeyEvent::Char('h'));
        assert_eq!(action, ViewAction::Redraw);
        assert_eq!(vc.input().line().content(), "h");
    }

    // ── Message routing ──────────────────────────────────────────

    #[test]
    fn push_message_to_buffer() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#test".into()));
        vc.push_message(&BufferId::Channel("#test".into()), make_line("hello"));
        assert_eq!(
            vc.buffers()
                .get(&BufferId::Channel("#test".into()))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn push_status_message() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.push_status_message(make_line("server info"));
        assert_eq!(vc.buffers().get(&BufferId::Status).unwrap().len(), 1);
    }

    // ── Nick management ──────────────────────────────────────────

    #[test]
    fn set_nick_updates() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.set_nick("testuser".into());
        assert_eq!(vc.nick, "testuser");
    }

    #[test]
    fn set_lag_updates() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        assert_eq!(vc.lag, None);
        vc.set_lag(Some(42));
        assert_eq!(vc.lag, Some(42));
        vc.set_lag(None);
        assert_eq!(vc.lag, None);
    }

    #[test]
    fn render_with_lag_shows_in_status_bar() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.set_lag(Some(42));
        let mut buf = Buffer::new(80, 24);
        vc.render(&mut buf);
        // Verify the lag indicator appears in the rendered output.
        // The status bar is at a specific row — extract that row's text.
        let layout = vc.layout.unwrap();
        let row = layout.status_bar.y;
        let text: String = (0..80).map(|col| buf.get(col, row).ch).collect();
        assert!(
            text.contains("lag: 42ms"),
            "status bar should contain lag indicator: {text}"
        );
    }

    // ── Rendering ────────────────────────────────────────────────

    #[test]
    fn render_does_not_panic() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.push_status_message(make_line("welcome"));
        let mut buf = Buffer::new(80, 24);
        vc.render(&mut buf);
    }

    #[test]
    fn render_with_no_layout_is_noop() {
        let mut vc = ViewCoordinator::new(10, 5, 100);
        assert!(vc.layout.is_none());
        let mut buf = Buffer::new(10, 5);
        vc.render(&mut buf);
    }

    #[test]
    fn render_with_search_active() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.start_search();
        let mut buf = Buffer::new(80, 24);
        vc.render(&mut buf);
    }

    #[test]
    fn render_with_multiple_buffers() {
        let mut vc = ViewCoordinator::new(80, 24, 100);
        vc.buffers_mut().open(BufferId::Channel("#a".into()));
        vc.buffers_mut().open(BufferId::Channel("#b".into()));
        vc.push_message(&BufferId::Channel("#a".into()), make_line("hello"));
        let mut buf = Buffer::new(80, 24);
        vc.render(&mut buf);
    }
}
