use crate::client_command::{ClientCommand, CommandError};
use crate::command_parser::{self, ParsedInput};

use super::input::KeyEvent;
use super::input_history::InputHistory;
use super::input_line_state::InputLineState;
use super::tab_completion::TabCompleter;

// ── InputAction ──────────────────────────────────────────────────────

/// An action produced by the input handler in response to a key event.
///
/// The caller (event loop) inspects the returned action and decides what to
/// do: send a protocol message, redraw the input line, scroll the view, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    /// No visible state change — the event was consumed silently.
    None,
    /// The user submitted a valid command. The event loop should convert it
    /// to a protocol message (via `ClientCommand::to_message`) and send it.
    Command(ClientCommand),
    /// The user submitted a plain chat message (no `/` prefix). The event
    /// loop should send it to the current channel/query target.
    ChatMessage(String),
    /// A command was submitted but had invalid arguments.
    CommandError(CommandError),
    /// The input line changed and should be redrawn.
    Redraw,
    /// Scroll the message view up by one page.
    ScrollUp,
    /// Scroll the message view down by one page.
    ScrollDown,
    /// The user requested quit (Ctrl+C or `/quit`), with an optional reason.
    Quit(Option<String>),
    /// The terminal was resized to (cols, rows).
    Resize(u16, u16),
}

// ── InputHandler ─────────────────────────────────────────────────────

/// Orchestrates all input-related components: line editing, history,
/// tab completion, and command parsing.
///
/// `InputHandler` is the single coordination point for keyboard input.
/// It translates raw [`KeyEvent`]s into [`InputAction`]s that the event
/// loop can act on. It does **not** perform rendering — the caller reads
/// the input line state and renders it.
pub struct InputHandler {
    line: InputLineState,
    history: InputHistory,
    completer: TabCompleter,
    /// Nick candidates for tab completion (updated by the event loop).
    nick_candidates: Vec<String>,
    /// Channel candidates for tab completion (updated by the event loop).
    channel_candidates: Vec<String>,
}

impl InputHandler {
    /// Create a new input handler with default settings.
    pub fn new() -> Self {
        Self {
            line: InputLineState::new(),
            history: InputHistory::new(500),
            completer: TabCompleter::new(),
            nick_candidates: Vec::new(),
            channel_candidates: Vec::new(),
        }
    }

    /// Return a reference to the current input line state (for rendering).
    pub fn line(&self) -> &InputLineState {
        &self.line
    }

    /// Return a mutable reference to the current input line state.
    pub fn line_mut(&mut self) -> &mut InputLineState {
        &mut self.line
    }

    /// Update the scroll offset of the input line to keep the cursor visible.
    pub fn update_scroll(&mut self, visible_width: usize) {
        self.line.update_scroll(visible_width);
    }

    /// Update the nick candidates used for tab completion.
    pub fn set_nick_candidates(&mut self, nicks: Vec<String>) {
        self.nick_candidates = nicks;
    }

    /// Update the channel candidates used for tab completion.
    pub fn set_channel_candidates(&mut self, channels: Vec<String>) {
        self.channel_candidates = channels;
    }

    /// Process a key event and return the resulting action.
    ///
    /// This is the main entry point. The event loop calls this for every
    /// key event and then inspects the returned [`InputAction`].
    pub fn handle_key(&mut self, event: KeyEvent) -> InputAction {
        match event {
            // ── Text input ───────────────────────────────────────
            KeyEvent::Char(ch) => {
                self.completer.reset();
                self.line.insert_char(ch);
                InputAction::Redraw
            }

            // ── Submission ───────────────────────────────────────
            KeyEvent::Enter => {
                self.completer.reset();
                self.submit()
            }

            // ── Deletion ─────────────────────────────────────────
            KeyEvent::Backspace => {
                self.completer.reset();
                if self.line.backspace() {
                    InputAction::Redraw
                } else {
                    InputAction::None
                }
            }
            KeyEvent::Delete => {
                self.completer.reset();
                if self.line.delete() {
                    InputAction::Redraw
                } else {
                    InputAction::None
                }
            }

            // ── Cursor movement ──────────────────────────────────
            KeyEvent::Left => {
                self.completer.reset();
                self.line.move_left();
                InputAction::Redraw
            }
            KeyEvent::Right => {
                self.completer.reset();
                self.line.move_right();
                InputAction::Redraw
            }
            KeyEvent::Home => {
                self.completer.reset();
                self.line.move_home();
                InputAction::Redraw
            }
            KeyEvent::End => {
                self.completer.reset();
                self.line.move_end();
                InputAction::Redraw
            }

            // ── History navigation ───────────────────────────────
            KeyEvent::Up => {
                self.completer.reset();
                self.navigate_history_up()
            }
            KeyEvent::Down => {
                self.completer.reset();
                self.navigate_history_down()
            }

            // ── Tab completion ───────────────────────────────────
            KeyEvent::Tab => self.tab_complete(),
            KeyEvent::BackTab => self.tab_complete_backward(),

            // ── Ctrl shortcuts ───────────────────────────────────
            KeyEvent::Ctrl('a') => {
                self.completer.reset();
                self.line.move_home();
                InputAction::Redraw
            }
            KeyEvent::Ctrl('e') => {
                self.completer.reset();
                self.line.move_end();
                InputAction::Redraw
            }
            KeyEvent::Ctrl('u') => {
                self.completer.reset();
                self.line.kill_line();
                InputAction::Redraw
            }
            KeyEvent::Ctrl('k') => {
                self.completer.reset();
                self.line.kill_to_end();
                InputAction::Redraw
            }
            KeyEvent::Ctrl('w') => {
                self.completer.reset();
                self.line.delete_word_backward();
                InputAction::Redraw
            }
            KeyEvent::Ctrl('c') => {
                self.completer.reset();
                if self.line.is_empty() {
                    InputAction::Quit(None)
                } else {
                    self.line.clear();
                    self.history.reset_navigation();
                    InputAction::Redraw
                }
            }

            // ── Passthrough events ───────────────────────────────
            KeyEvent::PageUp => InputAction::ScrollUp,
            KeyEvent::PageDown => InputAction::ScrollDown,
            KeyEvent::Resize(cols, rows) => InputAction::Resize(cols, rows),

            // ── Ignored ──────────────────────────────────────────
            KeyEvent::Escape | KeyEvent::Unknown(_) | KeyEvent::Ctrl(_) => {
                self.completer.reset();
                InputAction::None
            }
        }
    }

    // ── Private helpers ──────────────────────────────────────────────

    /// Handle Enter: parse the input, add to history, clear the line,
    /// and return the appropriate action.
    fn submit(&mut self) -> InputAction {
        let text = self.line.content().to_owned();

        // Empty input — nothing to submit.
        if text.trim().is_empty() {
            return InputAction::None;
        }

        // Add to history and clear the line.
        self.history.push(&text);
        self.line.clear();
        self.history.reset_navigation();

        // Parse and produce the action.
        match command_parser::parse(&text) {
            ParsedInput::ChatMessage(msg) => InputAction::ChatMessage(msg),
            ParsedInput::Command { name, args } => match ClientCommand::from_parsed(&name, &args) {
                Ok(cmd) => {
                    if let ClientCommand::Quit(reason) = cmd {
                        InputAction::Quit(reason)
                    } else {
                        InputAction::Command(cmd)
                    }
                }
                Err(e) => InputAction::CommandError(e),
            },
        }
    }

    /// Navigate history upward and replace the input line content.
    fn navigate_history_up(&mut self) -> InputAction {
        let current = self.line.content().to_owned();
        if let Some(entry) = self.history.navigate_up(&current) {
            let entry = entry.to_owned();
            self.line.set_content(&entry);
            InputAction::Redraw
        } else {
            InputAction::None
        }
    }

    /// Navigate history downward and replace the input line content.
    fn navigate_history_down(&mut self) -> InputAction {
        if let Some(entry) = self.history.navigate_down() {
            let entry = entry.to_owned();
            self.line.set_content(&entry);
            InputAction::Redraw
        } else {
            InputAction::None
        }
    }

    /// Perform forward tab completion.
    fn tab_complete(&mut self) -> InputAction {
        let line = self.line.content().to_owned();
        let cursor = self.line.cursor_position();

        let nicks: Vec<&str> = self.nick_candidates.iter().map(|s| s.as_str()).collect();
        let channels: Vec<&str> = self.channel_candidates.iter().map(|s| s.as_str()).collect();

        if let Some((new_line, new_cursor)) =
            self.completer.complete(&line, cursor, &nicks, &channels)
        {
            self.line.set_content(&new_line);
            // Position cursor at the completion point.
            self.line.move_home();
            for _ in 0..new_cursor {
                self.line.move_right();
            }
            InputAction::Redraw
        } else {
            InputAction::None
        }
    }

    /// Perform backward tab completion (Shift+Tab).
    fn tab_complete_backward(&mut self) -> InputAction {
        if let Some((new_line, new_cursor)) = self.completer.complete_backward() {
            self.line.set_content(&new_line);
            self.line.move_home();
            for _ in 0..new_cursor {
                self.line.move_right();
            }
            InputAction::Redraw
        } else {
            InputAction::None
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> InputHandler {
        InputHandler::new()
    }

    // ── Character input ──────────────────────────────────────────

    #[test]
    fn char_inserts_and_redraws() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Char('a')), InputAction::Redraw);
        assert_eq!(h.line().content(), "a");
    }

    #[test]
    fn multiple_chars_build_string() {
        let mut h = handler();
        h.handle_key(KeyEvent::Char('h'));
        h.handle_key(KeyEvent::Char('i'));
        assert_eq!(h.line().content(), "hi");
    }

    #[test]
    fn unicode_char_input() {
        let mut h = handler();
        h.handle_key(KeyEvent::Char('ä'));
        h.handle_key(KeyEvent::Char('ö'));
        assert_eq!(h.line().content(), "äö");
    }

    // ── Enter / submission ───────────────────────────────────────

    #[test]
    fn enter_on_empty_line_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Enter), InputAction::None);
    }

    #[test]
    fn enter_on_whitespace_only_returns_none() {
        let mut h = handler();
        h.handle_key(KeyEvent::Char(' '));
        h.handle_key(KeyEvent::Char(' '));
        assert_eq!(h.handle_key(KeyEvent::Enter), InputAction::None);
    }

    #[test]
    fn enter_submits_chat_message() {
        let mut h = handler();
        for ch in "hello world".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(action, InputAction::ChatMessage("hello world".into()));
        assert!(h.line().is_empty());
    }

    #[test]
    fn enter_submits_command() {
        let mut h = handler();
        for ch in "/join #test".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(
            action,
            InputAction::Command(ClientCommand::Join("#test".into()))
        );
        assert!(h.line().is_empty());
    }

    #[test]
    fn enter_with_invalid_command_returns_error() {
        let mut h = handler();
        // /join without a channel argument
        for ch in "/join".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert!(matches!(action, InputAction::CommandError(_)));
    }

    #[test]
    fn quit_command_returns_quit_action() {
        let mut h = handler();
        for ch in "/quit".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(h.handle_key(KeyEvent::Enter), InputAction::Quit(None));
    }

    #[test]
    fn quit_command_with_reason_returns_quit() {
        let mut h = handler();
        for ch in "/quit goodbye".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(
            h.handle_key(KeyEvent::Enter),
            InputAction::Quit(Some("goodbye".to_string()))
        );
    }

    #[test]
    fn double_slash_escape_is_chat_message() {
        let mut h = handler();
        for ch in "//not a command".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(action, InputAction::ChatMessage("/not a command".into()));
    }

    #[test]
    fn enter_adds_to_history() {
        let mut h = handler();
        for ch in "first".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);

        for ch in "second".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);

        // Navigate up should show "second"
        let action = h.handle_key(KeyEvent::Up);
        assert_eq!(action, InputAction::Redraw);
        assert_eq!(h.line().content(), "second");
    }

    #[test]
    fn enter_clears_line_after_submit() {
        let mut h = handler();
        for ch in "test".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);
        assert!(h.line().is_empty());
        assert_eq!(h.line().cursor_position(), 0);
    }

    // ── Backspace and Delete ─────────────────────────────────────

    #[test]
    fn backspace_on_empty_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Backspace), InputAction::None);
    }

    #[test]
    fn backspace_deletes_char() {
        let mut h = handler();
        h.handle_key(KeyEvent::Char('a'));
        h.handle_key(KeyEvent::Char('b'));
        assert_eq!(h.handle_key(KeyEvent::Backspace), InputAction::Redraw);
        assert_eq!(h.line().content(), "a");
    }

    #[test]
    fn delete_on_empty_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Delete), InputAction::None);
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut h = handler();
        h.handle_key(KeyEvent::Char('a'));
        h.handle_key(KeyEvent::Char('b'));
        h.handle_key(KeyEvent::Left); // cursor before 'b'
        assert_eq!(h.handle_key(KeyEvent::Delete), InputAction::Redraw);
        assert_eq!(h.line().content(), "a");
    }

    // ── Cursor movement ──────────────────────────────────────────

    #[test]
    fn left_right_movement() {
        let mut h = handler();
        h.handle_key(KeyEvent::Char('a'));
        h.handle_key(KeyEvent::Char('b'));
        h.handle_key(KeyEvent::Char('c'));
        assert_eq!(h.line().cursor_position(), 3);

        h.handle_key(KeyEvent::Left);
        assert_eq!(h.line().cursor_position(), 2);

        h.handle_key(KeyEvent::Right);
        assert_eq!(h.line().cursor_position(), 3);
    }

    #[test]
    fn home_end_movement() {
        let mut h = handler();
        for ch in "hello".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Home);
        assert_eq!(h.line().cursor_position(), 0);

        h.handle_key(KeyEvent::End);
        assert_eq!(h.line().cursor_position(), 5);
    }

    // ── History navigation ───────────────────────────────────────

    #[test]
    fn up_on_empty_history_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Up), InputAction::None);
    }

    #[test]
    fn down_without_browsing_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Down), InputAction::None);
    }

    #[test]
    fn history_up_down_cycle() {
        let mut h = handler();

        // Add two entries.
        for ch in "alpha".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);
        for ch in "beta".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);

        // Up → "beta"
        h.handle_key(KeyEvent::Up);
        assert_eq!(h.line().content(), "beta");

        // Up → "alpha"
        h.handle_key(KeyEvent::Up);
        assert_eq!(h.line().content(), "alpha");

        // Up at oldest → still "alpha"
        h.handle_key(KeyEvent::Up);
        assert_eq!(h.line().content(), "alpha");

        // Down → "beta"
        h.handle_key(KeyEvent::Down);
        assert_eq!(h.line().content(), "beta");

        // Down → back to draft (empty)
        h.handle_key(KeyEvent::Down);
        assert_eq!(h.line().content(), "");
    }

    #[test]
    fn history_preserves_draft() {
        let mut h = handler();

        // Add a history entry.
        for ch in "old".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);

        // Type some draft text.
        for ch in "draft".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }

        // Navigate up — should see "old", draft saved.
        h.handle_key(KeyEvent::Up);
        assert_eq!(h.line().content(), "old");

        // Navigate down — draft restored.
        h.handle_key(KeyEvent::Down);
        assert_eq!(h.line().content(), "draft");
    }

    // ── Ctrl shortcuts ───────────────────────────────────────────

    #[test]
    fn ctrl_a_moves_home() {
        let mut h = handler();
        for ch in "hello".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Ctrl('a'));
        assert_eq!(h.line().cursor_position(), 0);
    }

    #[test]
    fn ctrl_e_moves_end() {
        let mut h = handler();
        for ch in "hello".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Home);
        h.handle_key(KeyEvent::Ctrl('e'));
        assert_eq!(h.line().cursor_position(), 5);
    }

    #[test]
    fn ctrl_u_clears_line() {
        let mut h = handler();
        for ch in "hello".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(h.handle_key(KeyEvent::Ctrl('u')), InputAction::Redraw);
        assert!(h.line().is_empty());
    }

    #[test]
    fn ctrl_k_kills_to_end() {
        let mut h = handler();
        for ch in "hello".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        // Move cursor to position 2
        h.handle_key(KeyEvent::Home);
        h.handle_key(KeyEvent::Right);
        h.handle_key(KeyEvent::Right);
        assert_eq!(h.handle_key(KeyEvent::Ctrl('k')), InputAction::Redraw);
        assert_eq!(h.line().content(), "he");
    }

    #[test]
    fn ctrl_w_deletes_word_backward() {
        let mut h = handler();
        for ch in "hello world".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(h.handle_key(KeyEvent::Ctrl('w')), InputAction::Redraw);
        assert_eq!(h.line().content(), "hello ");
    }

    #[test]
    fn ctrl_c_on_empty_line_quits() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Ctrl('c')), InputAction::Quit(None));
    }

    #[test]
    fn ctrl_c_with_text_clears_line() {
        let mut h = handler();
        for ch in "some text".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(h.handle_key(KeyEvent::Ctrl('c')), InputAction::Redraw);
        assert!(h.line().is_empty());
    }

    // ── Tab completion ───────────────────────────────────────────

    #[test]
    fn tab_with_no_candidates_returns_none() {
        let mut h = handler();
        for ch in "he".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(h.handle_key(KeyEvent::Tab), InputAction::None);
    }

    #[test]
    fn tab_completes_nick() {
        let mut h = handler();
        h.set_nick_candidates(vec!["alice".into(), "bob".into()]);

        for ch in "al".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Tab);
        assert_eq!(action, InputAction::Redraw);
        // At start of line, nick gets ": " suffix.
        assert_eq!(h.line().content(), "alice: ");
    }

    #[test]
    fn tab_completes_command() {
        let mut h = handler();
        for ch in "/jo".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Tab);
        assert_eq!(action, InputAction::Redraw);
        assert_eq!(h.line().content(), "/join ");
    }

    #[test]
    fn tab_completes_channel() {
        let mut h = handler();
        h.set_channel_candidates(vec!["#general".into(), "#random".into()]);

        for ch in "/join #gen".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Tab);
        assert_eq!(action, InputAction::Redraw);
        assert_eq!(h.line().content(), "/join #general ");
    }

    #[test]
    fn tab_cycling() {
        let mut h = handler();
        h.set_nick_candidates(vec!["alice".into(), "alex".into()]);

        for ch in "al".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Tab);
        let first = h.line().content().to_owned();

        h.handle_key(KeyEvent::Tab);
        let second = h.line().content().to_owned();

        assert_ne!(first, second);
    }

    #[test]
    fn backtab_cycles_backward() {
        let mut h = handler();
        h.set_nick_candidates(vec!["alice".into(), "alex".into()]);

        for ch in "al".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        // Start completion session.
        h.handle_key(KeyEvent::Tab);
        let first = h.line().content().to_owned();

        // Cycle forward.
        h.handle_key(KeyEvent::Tab);
        let second = h.line().content().to_owned();

        // Cycle backward — should be back to first.
        h.handle_key(KeyEvent::BackTab);
        assert_eq!(h.line().content(), first);
        assert_ne!(first, second);
    }

    #[test]
    fn non_tab_key_resets_completion() {
        let mut h = handler();
        h.set_nick_candidates(vec!["alice".into(), "alex".into()]);

        for ch in "al".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Tab);
        assert!(h.completer.is_active());

        // Any non-Tab key resets completion.
        h.handle_key(KeyEvent::Char('x'));
        assert!(!h.completer.is_active());
    }

    // ── Passthrough events ───────────────────────────────────────

    #[test]
    fn page_up_returns_scroll_up() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::PageUp), InputAction::ScrollUp);
    }

    #[test]
    fn page_down_returns_scroll_down() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::PageDown), InputAction::ScrollDown);
    }

    #[test]
    fn resize_returns_resize_action() {
        let mut h = handler();
        assert_eq!(
            h.handle_key(KeyEvent::Resize(120, 40)),
            InputAction::Resize(120, 40)
        );
    }

    // ── Ignored events ───────────────────────────────────────────

    #[test]
    fn escape_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Escape), InputAction::None);
    }

    #[test]
    fn unknown_sequence_returns_none() {
        let mut h = handler();
        assert_eq!(
            h.handle_key(KeyEvent::Unknown(vec![0x1b, 0x5b, 0x99])),
            InputAction::None
        );
    }

    #[test]
    fn unhandled_ctrl_returns_none() {
        let mut h = handler();
        assert_eq!(h.handle_key(KeyEvent::Ctrl('z')), InputAction::None);
    }

    // ── Integration: full pipeline ───────────────────────────────

    #[test]
    fn full_pipeline_type_and_submit_command() {
        let mut h = handler();
        // Type "/nick bob", press Enter.
        for ch in "/nick bob".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(
            action,
            InputAction::Command(ClientCommand::Nick("bob".into()))
        );
        // Line should be clear, history should have the entry.
        assert!(h.line().is_empty());
    }

    #[test]
    fn full_pipeline_history_then_submit() {
        let mut h = handler();

        // Submit first message.
        for ch in "/join #rust".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Enter);

        // Navigate up to recall it.
        h.handle_key(KeyEvent::Up);
        assert_eq!(h.line().content(), "/join #rust");

        // Submit the recalled command.
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(
            action,
            InputAction::Command(ClientCommand::Join("#rust".into()))
        );
    }

    #[test]
    fn full_pipeline_tab_complete_then_submit() {
        let mut h = handler();
        h.set_nick_candidates(vec!["charlie".into()]);

        // Type "ch", tab complete to "charlie: ", then add text and submit.
        for ch in "ch".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        h.handle_key(KeyEvent::Tab);
        assert_eq!(h.line().content(), "charlie: ");

        for ch in "hey!".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(action, InputAction::ChatMessage("charlie: hey!".into()));
    }

    #[test]
    fn full_pipeline_edit_then_submit() {
        let mut h = handler();

        // Type "helo", fix typo, submit.
        for ch in "helo".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        // Move left twice (cursor before 'l'), insert 'l'.
        h.handle_key(KeyEvent::Left);
        h.handle_key(KeyEvent::Left);
        h.handle_key(KeyEvent::Char('l'));
        assert_eq!(h.line().content(), "hello");

        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(action, InputAction::ChatMessage("hello".into()));
    }

    #[test]
    fn full_pipeline_ctrl_c_clears_then_quit() {
        let mut h = handler();

        // Type some text, Ctrl+C clears it.
        for ch in "text".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        assert_eq!(h.handle_key(KeyEvent::Ctrl('c')), InputAction::Redraw);
        assert!(h.line().is_empty());

        // Now Ctrl+C on empty line quits.
        assert_eq!(h.handle_key(KeyEvent::Ctrl('c')), InputAction::Quit(None));
    }

    #[test]
    fn msg_command_pipeline() {
        let mut h = handler();
        for ch in "/msg alice hello there".chars() {
            h.handle_key(KeyEvent::Char(ch));
        }
        let action = h.handle_key(KeyEvent::Enter);
        assert_eq!(
            action,
            InputAction::Command(ClientCommand::Msg("alice".into(), "hello there".into()))
        );
    }
}
