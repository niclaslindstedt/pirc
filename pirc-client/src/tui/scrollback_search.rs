use crate::tui::input_line_state::InputLineState;
use crate::tui::message_buffer::MessageBuffer;

/// Direction of search navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

/// Pure state for scrollback search mode.
///
/// Manages the search query (via `InputLineState`), match indices into the
/// `MessageBuffer`, and navigation between matches. No I/O is performed.
#[derive(Debug, Clone)]
pub struct SearchState {
    active: bool,
    query: InputLineState,
    matches: Vec<usize>,
    current_match: Option<usize>,
    direction: SearchDirection,
}

impl SearchState {
    /// Create a new inactive search state.
    pub fn new() -> Self {
        Self {
            active: false,
            query: InputLineState::new(),
            matches: Vec::new(),
            current_match: None,
            direction: SearchDirection::Forward,
        }
    }

    /// Enter search mode, clearing any previous query and results.
    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.current_match = None;
        self.direction = SearchDirection::Forward;
    }

    /// Exit search mode and clear all state.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current_match = None;
    }

    /// Returns `true` if search mode is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Re-run search against the buffer using the current query.
    ///
    /// Performs case-insensitive substring search across both the sender
    /// and content fields of each `BufferLine`. Results are ordered by
    /// buffer index (oldest to newest). If the previous current match is
    /// still valid, it is preserved; otherwise the first match is selected.
    pub fn update_query(&mut self, buffer: &MessageBuffer) {
        let query_text = self.query.content().to_lowercase();
        self.matches.clear();
        self.current_match = None;

        if query_text.is_empty() {
            return;
        }

        // Search both sender and content (case-insensitive).
        for (i, line) in buffer.iter_lines().enumerate() {
            let content_match = line.content.to_lowercase().contains(&query_text);
            let sender_match = line
                .sender
                .as_ref()
                .map(|s| s.to_lowercase().contains(&query_text))
                .unwrap_or(false);
            if content_match || sender_match {
                self.matches.push(i);
            }
        }

        if !self.matches.is_empty() {
            self.current_match = Some(0);
        }
    }

    /// Move to the next match, wrapping around to the first.
    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.current_match = Some(match self.current_match {
            Some(idx) => (idx + 1) % self.matches.len(),
            None => 0,
        });
        self.direction = SearchDirection::Forward;
    }

    /// Move to the previous match, wrapping around to the last.
    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.current_match = Some(match self.current_match {
            Some(0) => self.matches.len() - 1,
            Some(idx) => idx - 1,
            None => self.matches.len() - 1,
        });
        self.direction = SearchDirection::Backward;
    }

    /// Return the buffer line index of the current match, if any.
    pub fn current_match_line(&self) -> Option<usize> {
        self.current_match.map(|idx| self.matches[idx])
    }

    /// Return the total number of matches.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Return the current search query text.
    pub fn query_str(&self) -> &str {
        self.query.content()
    }

    /// Return a mutable reference to the query input state for editing.
    pub fn query_mut(&mut self) -> &mut InputLineState {
        &mut self.query
    }

    /// Return the current match index (1-based) for display, if any.
    pub fn current_match_index(&self) -> Option<usize> {
        self.current_match.map(|idx| idx + 1)
    }

    /// Return the search direction.
    pub fn direction(&self) -> SearchDirection {
        self.direction
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a search bar overlay into the given buffer region.
///
/// When search is active, draws:
///   `[Search: query_text] (N/M matches)` or `[Search: query_text] (no matches)`
///
/// Uses char-based operations for all string rendering to prevent UTF-8 panics.
pub fn render_search_bar(
    buf: &mut crate::tui::buffer::Buffer,
    region: &crate::tui::layout::Rect,
    state: &SearchState,
) {
    use crate::tui::style::{Color, Style};

    if !state.is_active() || region.width == 0 || region.height == 0 {
        return;
    }

    let bar_style = Style::new().fg(Color::White).bg(Color::Blue).bold(true);
    let row = region.y + region.height - 1;
    let width = region.width as usize;

    // Clear the row with the bar style.
    buf.clear_region(region.x, row, region.width, 1, bar_style);

    // Build the search bar text.
    let query_text = state.query_str();
    let match_info = if state.matches.is_empty() {
        if query_text.is_empty() {
            String::new()
        } else {
            " (no matches)".to_string()
        }
    } else {
        let current = state.current_match_index().unwrap_or(0);
        format!(" ({}/{})", current, state.match_count())
    };

    let prefix = "[Search: ";
    let suffix = "]";

    // Calculate how much space is available for the query text.
    // Use chars().count() for UTF-8 safety.
    let fixed_chars = prefix.chars().count() + suffix.chars().count() + match_info.chars().count();

    let available = width.saturating_sub(fixed_chars);

    // Truncate query if needed, using char-based operations.
    let query_char_count = query_text.chars().count();
    let displayed_query: String = if query_char_count > available {
        // Show the tail of the query so the cursor stays visible.
        let skip = query_char_count - available;
        query_text.chars().skip(skip).collect()
    } else {
        query_text.to_string()
    };

    let full_text = format!("{}{}{}{}", prefix, displayed_query, suffix, match_info);

    // Write char-by-char to the buffer, respecting region width.
    let mut col = region.x;
    for ch in full_text.chars() {
        if col >= region.x + region.width {
            break;
        }
        buf.set(col, row, ch, bar_style);
        col += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::message_buffer::{BufferLine, LineType, MessageBuffer};

    fn make_line(content: &str) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: Some("nick".to_string()),
            content: content.to_string(),
            line_type: LineType::Message,
        }
    }

    fn make_line_with_sender(sender: &str, content: &str) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: Some(sender.to_string()),
            content: content.to_string(),
            line_type: LineType::Message,
        }
    }

    fn make_system_line(content: &str) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: None,
            content: content.to_string(),
            line_type: LineType::System,
        }
    }

    // ── Construction and defaults ────────────────────────────────

    #[test]
    fn new_is_inactive() {
        let state = SearchState::new();
        assert!(!state.is_active());
        assert_eq!(state.query_str(), "");
        assert_eq!(state.match_count(), 0);
        assert_eq!(state.current_match_line(), None);
    }

    #[test]
    fn default_is_same_as_new() {
        let state = SearchState::default();
        assert!(!state.is_active());
        assert_eq!(state.match_count(), 0);
    }

    // ── Activation / deactivation ────────────────────────────────

    #[test]
    fn activate_enables_search() {
        let mut state = SearchState::new();
        state.activate();
        assert!(state.is_active());
        assert_eq!(state.query_str(), "");
        assert_eq!(state.match_count(), 0);
    }

    #[test]
    fn deactivate_clears_state() {
        let mut state = SearchState::new();
        state.activate();
        state.query_mut().insert_char('a');
        state.deactivate();
        assert!(!state.is_active());
        assert_eq!(state.query_str(), "");
        assert_eq!(state.match_count(), 0);
        assert_eq!(state.current_match_line(), None);
    }

    #[test]
    fn activate_clears_previous_query() {
        let mut state = SearchState::new();
        state.activate();
        state.query_mut().insert_char('x');

        // Re-activate should clear.
        state.activate();
        assert_eq!(state.query_str(), "");
        assert_eq!(state.match_count(), 0);
    }

    // ── Query update and search ──────────────────────────────────

    #[test]
    fn update_query_finds_matches() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("Hello World"));
        buf.push_message(make_line("foo bar"));
        buf.push_message(make_line("hello again"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 2);
        assert_eq!(state.current_match_line(), Some(0));
    }

    #[test]
    fn update_query_case_insensitive() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("HELLO"));
        buf.push_message(make_line("Hello"));
        buf.push_message(make_line("hello"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hElLo".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 3);
    }

    #[test]
    fn update_query_empty_returns_no_matches() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello"));

        let mut state = SearchState::new();
        state.activate();
        state.update_query(&buf);

        assert_eq!(state.match_count(), 0);
        assert_eq!(state.current_match_line(), None);
    }

    #[test]
    fn update_query_no_matches() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("foo"));
        buf.push_message(make_line("bar"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "baz".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 0);
        assert_eq!(state.current_match_line(), None);
    }

    #[test]
    fn update_query_searches_sender() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line_with_sender("Alice", "hello"));
        buf.push_message(make_line_with_sender("Bob", "world"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "alice".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 1);
        assert_eq!(state.current_match_line(), Some(0));
    }

    #[test]
    fn update_query_searches_sender_and_content() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line_with_sender("alice", "hello"));
        buf.push_message(make_line_with_sender("bob", "alice said hi"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "alice".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        // Matches both: line 0 by sender, line 1 by content.
        assert_eq!(state.match_count(), 2);
    }

    #[test]
    fn update_query_handles_no_sender() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_system_line("System message"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "system".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 1);
        assert_eq!(state.current_match_line(), Some(0));
    }

    #[test]
    fn update_query_empty_buffer() {
        let buf = MessageBuffer::new(100);

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 0);
    }

    // ── Match navigation ─────────────────────────────────────────

    #[test]
    fn next_match_advances() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello a"));
        buf.push_message(make_line("world"));
        buf.push_message(make_line("hello b"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.current_match_line(), Some(0));
        state.next_match();
        assert_eq!(state.current_match_line(), Some(2));
    }

    #[test]
    fn next_match_wraps() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello a"));
        buf.push_message(make_line("hello b"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.current_match_line(), Some(0));
        state.next_match();
        assert_eq!(state.current_match_line(), Some(1));
        state.next_match();
        // Wraps to first.
        assert_eq!(state.current_match_line(), Some(0));
    }

    #[test]
    fn prev_match_goes_backward() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello a"));
        buf.push_message(make_line("world"));
        buf.push_message(make_line("hello b"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        // Start at first match.
        assert_eq!(state.current_match_line(), Some(0));
        state.prev_match();
        // Wraps to last.
        assert_eq!(state.current_match_line(), Some(2));
    }

    #[test]
    fn prev_match_wraps_from_last() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello a"));
        buf.push_message(make_line("hello b"));
        buf.push_message(make_line("hello c"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        // Go to last.
        state.prev_match();
        assert_eq!(state.current_match_line(), Some(2));
        state.prev_match();
        assert_eq!(state.current_match_line(), Some(1));
        state.prev_match();
        assert_eq!(state.current_match_line(), Some(0));
        // Wrap again.
        state.prev_match();
        assert_eq!(state.current_match_line(), Some(2));
    }

    #[test]
    fn next_match_on_empty_is_noop() {
        let mut state = SearchState::new();
        state.activate();
        state.next_match();
        assert_eq!(state.current_match_line(), None);
    }

    #[test]
    fn prev_match_on_empty_is_noop() {
        let mut state = SearchState::new();
        state.activate();
        state.prev_match();
        assert_eq!(state.current_match_line(), None);
    }

    #[test]
    fn direction_updates_on_navigation() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello a"));
        buf.push_message(make_line("hello b"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.direction(), SearchDirection::Forward);
        state.prev_match();
        assert_eq!(state.direction(), SearchDirection::Backward);
        state.next_match();
        assert_eq!(state.direction(), SearchDirection::Forward);
    }

    // ── Match index display ──────────────────────────────────────

    #[test]
    fn current_match_index_is_one_based() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello a"));
        buf.push_message(make_line("hello b"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.current_match_index(), Some(1));
        state.next_match();
        assert_eq!(state.current_match_index(), Some(2));
    }

    #[test]
    fn current_match_index_none_when_no_matches() {
        let state = SearchState::new();
        assert_eq!(state.current_match_index(), None);
    }

    // ── Query editing ────────────────────────────────────────────

    #[test]
    fn query_mut_allows_editing() {
        let mut state = SearchState::new();
        state.activate();
        state.query_mut().insert_char('h');
        state.query_mut().insert_char('i');
        assert_eq!(state.query_str(), "hi");
    }

    #[test]
    fn query_editing_with_backspace() {
        let mut state = SearchState::new();
        state.activate();
        state.query_mut().insert_char('a');
        state.query_mut().insert_char('b');
        state.query_mut().backspace();
        assert_eq!(state.query_str(), "a");
    }

    // ── Rendering ────────────────────────────────────────────────

    #[test]
    fn render_search_bar_inactive_is_noop() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(40, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };
        let state = SearchState::new();
        render_search_bar(&mut buf, &region, &state);

        // Buffer should be unchanged (all spaces).
        assert_eq!(buf.get(0, 0).ch, ' ');
    }

    #[test]
    fn render_search_bar_shows_query() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(40, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };
        let mut state = SearchState::new();
        state.activate();
        for ch in "test".chars() {
            state.query_mut().insert_char(ch);
        }
        render_search_bar(&mut buf, &region, &state);

        // Collect rendered text.
        let rendered: String = (0..40).map(|c| buf.get(c, 0).ch).collect();
        assert!(rendered.contains("[Search: test]"));
        assert!(rendered.contains("(no matches)"));
    }

    #[test]
    fn render_search_bar_shows_match_count() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut screen = Buffer::new(40, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };

        let mut mbuf = MessageBuffer::new(100);
        mbuf.push_message(make_line("hello a"));
        mbuf.push_message(make_line("world"));
        mbuf.push_message(make_line("hello b"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&mbuf);

        render_search_bar(&mut screen, &region, &state);

        let rendered: String = (0..40).map(|c| screen.get(c, 0).ch).collect();
        assert!(rendered.contains("[Search: hello]"));
        assert!(rendered.contains("(1/2)"));
    }

    #[test]
    fn render_search_bar_empty_query() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(40, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };
        let mut state = SearchState::new();
        state.activate();
        render_search_bar(&mut buf, &region, &state);

        let rendered: String = (0..40).map(|c| buf.get(c, 0).ch).collect();
        assert!(rendered.contains("[Search: ]"));
    }

    #[test]
    fn render_search_bar_truncates_long_query() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(30, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 1,
        };
        let mut state = SearchState::new();
        state.activate();
        for ch in "this is a very long search query that exceeds".chars() {
            state.query_mut().insert_char(ch);
        }
        render_search_bar(&mut buf, &region, &state);

        // Should not panic and should fill the available width.
        let rendered: String = (0..30).map(|c| buf.get(c, 0).ch).collect();
        assert!(rendered.contains("[Search: "));
    }

    #[test]
    fn render_search_bar_with_unicode_query() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(40, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };
        let mut state = SearchState::new();
        state.activate();
        // Insert multi-byte chars.
        for ch in "日本語".chars() {
            state.query_mut().insert_char(ch);
        }
        render_search_bar(&mut buf, &region, &state);

        let rendered: String = (0..40).map(|c| buf.get(c, 0).ch).collect();
        assert!(rendered.contains("日本語"));
    }

    #[test]
    fn render_search_bar_zero_width_is_noop() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(1, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 1,
        };
        let mut state = SearchState::new();
        state.activate();
        render_search_bar(&mut buf, &region, &state);
        // Should not panic.
    }

    #[test]
    fn render_search_bar_zero_height_is_noop() {
        use crate::tui::buffer::Buffer;
        use crate::tui::layout::Rect;

        let mut buf = Buffer::new(40, 1);
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 0,
        };
        let mut state = SearchState::new();
        state.activate();
        render_search_bar(&mut buf, &region, &state);
        // Should not panic.
    }

    // ── Single match navigation ──────────────────────────────────

    #[test]
    fn single_match_next_wraps_to_self() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        assert_eq!(state.match_count(), 1);
        assert_eq!(state.current_match_line(), Some(0));
        state.next_match();
        assert_eq!(state.current_match_line(), Some(0));
    }

    #[test]
    fn single_match_prev_wraps_to_self() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("hello"));

        let mut state = SearchState::new();
        state.activate();
        for ch in "hello".chars() {
            state.query_mut().insert_char(ch);
        }
        state.update_query(&buf);

        state.prev_match();
        assert_eq!(state.current_match_line(), Some(0));
    }
}
