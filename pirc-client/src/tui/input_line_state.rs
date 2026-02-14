/// Pure-state input line buffer with cursor movement and text editing.
///
/// Tracks the text content, cursor position (both char index and byte offset),
/// and a horizontal scroll offset for rendering long lines. All operations
/// are UTF-8 aware and respect character boundaries.
#[derive(Debug, Clone)]
pub struct InputLineState {
    /// The text content of the input line.
    buf: String,
    /// Cursor position as a character index (0 = before first char).
    cursor_char: usize,
    /// Cursor position as a byte offset into `buf`.
    cursor_byte: usize,
    /// Horizontal scroll offset (in characters) for rendering long lines.
    scroll_offset: usize,
}

impl InputLineState {
    /// Create a new empty input line.
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            cursor_char: 0,
            cursor_byte: 0,
            scroll_offset: 0,
        }
    }

    /// Return the current text content.
    pub fn content(&self) -> &str {
        &self.buf
    }

    /// Return the cursor position as a character index.
    pub fn cursor_position(&self) -> usize {
        self.cursor_char
    }

    /// Return the cursor position as a byte offset.
    pub fn cursor_byte_offset(&self) -> usize {
        self.cursor_byte
    }

    /// Return the current horizontal scroll offset (in characters).
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Return the total number of characters in the buffer.
    pub fn char_count(&self) -> usize {
        self.buf.chars().count()
    }

    /// Clear the buffer and reset cursor to the beginning.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor_char = 0;
        self.cursor_byte = 0;
        self.scroll_offset = 0;
    }

    /// Insert a character at the current cursor position.
    pub fn insert_char(&mut self, ch: char) {
        self.buf.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
        self.cursor_char += 1;
    }

    /// Delete the character before the cursor (backspace).
    /// Returns `true` if a character was deleted.
    pub fn backspace(&mut self) -> bool {
        if self.cursor_char == 0 {
            return false;
        }
        // Find the previous character boundary.
        let prev = prev_char_boundary(&self.buf, self.cursor_byte);
        self.buf.drain(prev..self.cursor_byte);
        self.cursor_byte = prev;
        self.cursor_char -= 1;
        true
    }

    /// Delete the character at the cursor (delete key).
    /// Returns `true` if a character was deleted.
    pub fn delete(&mut self) -> bool {
        if self.cursor_byte >= self.buf.len() {
            return false;
        }
        let next = next_char_boundary(&self.buf, self.cursor_byte);
        self.buf.drain(self.cursor_byte..next);
        // cursor_char and cursor_byte remain unchanged.
        true
    }

    /// Move the cursor one character to the left.
    pub fn move_left(&mut self) {
        if self.cursor_char == 0 {
            return;
        }
        let prev = prev_char_boundary(&self.buf, self.cursor_byte);
        self.cursor_byte = prev;
        self.cursor_char -= 1;
    }

    /// Move the cursor one character to the right.
    pub fn move_right(&mut self) {
        if self.cursor_byte >= self.buf.len() {
            return;
        }
        let next = next_char_boundary(&self.buf, self.cursor_byte);
        self.cursor_byte = next;
        self.cursor_char += 1;
    }

    /// Move the cursor to the beginning of the line (Home / Ctrl+A).
    pub fn move_home(&mut self) {
        self.cursor_char = 0;
        self.cursor_byte = 0;
    }

    /// Move the cursor to the end of the line (End / Ctrl+E).
    pub fn move_end(&mut self) {
        self.cursor_char = self.buf.chars().count();
        self.cursor_byte = self.buf.len();
    }

    /// Clear the entire line (Ctrl+U).
    pub fn kill_line(&mut self) {
        self.clear();
    }

    /// Kill from cursor to end of line (Ctrl+K).
    pub fn kill_to_end(&mut self) {
        self.buf.truncate(self.cursor_byte);
        // cursor position stays the same since text after cursor was removed.
    }

    /// Delete one word backward (Ctrl+W).
    ///
    /// Deletes whitespace immediately before the cursor, then deletes
    /// non-whitespace characters until a whitespace boundary or the
    /// start of the line.
    pub fn delete_word_backward(&mut self) {
        if self.cursor_char == 0 {
            return;
        }

        let start_byte = self.cursor_byte;
        let start_char = self.cursor_char;

        // Skip trailing whitespace.
        while self.cursor_char > 0 {
            let prev = prev_char_boundary(&self.buf, self.cursor_byte);
            let ch = self.buf[prev..self.cursor_byte]
                .chars()
                .next()
                .unwrap();
            if !ch.is_whitespace() {
                break;
            }
            self.cursor_byte = prev;
            self.cursor_char -= 1;
        }

        // Delete non-whitespace characters (the word).
        while self.cursor_char > 0 {
            let prev = prev_char_boundary(&self.buf, self.cursor_byte);
            let ch = self.buf[prev..self.cursor_byte]
                .chars()
                .next()
                .unwrap();
            if ch.is_whitespace() {
                break;
            }
            self.cursor_byte = prev;
            self.cursor_char -= 1;
        }

        // Remove the range.
        let chars_deleted = start_char - self.cursor_char;
        if chars_deleted > 0 {
            self.buf.drain(self.cursor_byte..start_byte);
        }
    }

    /// Update the scroll offset so the cursor is visible within `visible_width` columns.
    ///
    /// Returns the adjusted scroll offset.
    pub fn update_scroll(&mut self, visible_width: usize) -> usize {
        if visible_width == 0 {
            self.scroll_offset = 0;
            return 0;
        }
        if self.cursor_char < self.scroll_offset {
            self.scroll_offset = self.cursor_char;
        } else if self.cursor_char >= self.scroll_offset + visible_width {
            self.scroll_offset = self.cursor_char - visible_width + 1;
        }
        self.scroll_offset
    }

    /// Return `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Default for InputLineState {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the byte offset of the previous character boundary before `pos`.
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    let bytes = s.as_bytes();
    let mut i = pos;
    loop {
        if i == 0 {
            return 0;
        }
        i -= 1;
        // UTF-8 continuation bytes start with 0b10xxxxxx.
        if bytes[i] & 0xC0 != 0x80 {
            return i;
        }
    }
}

/// Find the byte offset of the next character boundary after `pos`.
fn next_char_boundary(s: &str, pos: usize) -> usize {
    let bytes = s.as_bytes();
    let mut i = pos + 1;
    while i < bytes.len() && bytes[i] & 0xC0 == 0x80 {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic operations ──────────────────────────────────────────

    #[test]
    fn new_is_empty() {
        let state = InputLineState::new();
        assert!(state.is_empty());
        assert_eq!(state.content(), "");
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);
    }

    #[test]
    fn insert_single_char() {
        let mut state = InputLineState::new();
        state.insert_char('a');
        assert_eq!(state.content(), "a");
        assert_eq!(state.cursor_position(), 1);
        assert_eq!(state.cursor_byte_offset(), 1);
    }

    #[test]
    fn insert_multiple_chars() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        assert_eq!(state.content(), "hello");
        assert_eq!(state.cursor_position(), 5);
    }

    #[test]
    fn insert_at_middle() {
        let mut state = InputLineState::new();
        for ch in "hllo".chars() {
            state.insert_char(ch);
        }
        // Move cursor after 'h'.
        state.move_home();
        state.move_right();
        state.insert_char('e');
        assert_eq!(state.content(), "hello");
        assert_eq!(state.cursor_position(), 2);
    }

    // ── Backspace ─────────────────────────────────────────────────

    #[test]
    fn backspace_at_start() {
        let mut state = InputLineState::new();
        assert!(!state.backspace());
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        assert!(state.backspace());
        assert_eq!(state.content(), "ab");
        assert_eq!(state.cursor_position(), 2);
    }

    #[test]
    fn backspace_in_middle() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        state.move_left(); // after 'b'
        assert!(state.backspace());
        assert_eq!(state.content(), "ac");
        assert_eq!(state.cursor_position(), 1);
    }

    // ── Delete ────────────────────────────────────────────────────

    #[test]
    fn delete_at_end() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        assert!(!state.delete());
    }

    #[test]
    fn delete_at_start() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        assert!(state.delete());
        assert_eq!(state.content(), "bc");
        assert_eq!(state.cursor_position(), 0);
    }

    #[test]
    fn delete_in_middle() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        state.move_right();
        assert!(state.delete());
        assert_eq!(state.content(), "ac");
        assert_eq!(state.cursor_position(), 1);
    }

    // ── Cursor movement ───────────────────────────────────────────

    #[test]
    fn move_left_at_start() {
        let mut state = InputLineState::new();
        state.move_left(); // no-op
        assert_eq!(state.cursor_position(), 0);
    }

    #[test]
    fn move_right_at_end() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        state.move_right(); // no-op
        assert_eq!(state.cursor_position(), 3);
    }

    #[test]
    fn move_left_and_right() {
        let mut state = InputLineState::new();
        for ch in "abc".chars() {
            state.insert_char(ch);
        }
        state.move_left();
        assert_eq!(state.cursor_position(), 2);
        state.move_left();
        assert_eq!(state.cursor_position(), 1);
        state.move_right();
        assert_eq!(state.cursor_position(), 2);
    }

    #[test]
    fn home_and_end() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);
        state.move_end();
        assert_eq!(state.cursor_position(), 5);
        assert_eq!(state.cursor_byte_offset(), 5);
    }

    // ── Ctrl shortcuts ────────────────────────────────────────────

    #[test]
    fn kill_line_clears_all() {
        let mut state = InputLineState::new();
        for ch in "hello world".chars() {
            state.insert_char(ch);
        }
        state.kill_line();
        assert!(state.is_empty());
        assert_eq!(state.cursor_position(), 0);
    }

    #[test]
    fn kill_to_end() {
        let mut state = InputLineState::new();
        for ch in "hello world".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        for _ in 0..5 {
            state.move_right();
        }
        state.kill_to_end();
        assert_eq!(state.content(), "hello");
        assert_eq!(state.cursor_position(), 5);
    }

    #[test]
    fn kill_to_end_at_end() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        state.kill_to_end();
        assert_eq!(state.content(), "hello");
    }

    #[test]
    fn delete_word_backward_single_word() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        state.delete_word_backward();
        assert_eq!(state.content(), "");
        assert_eq!(state.cursor_position(), 0);
    }

    #[test]
    fn delete_word_backward_two_words() {
        let mut state = InputLineState::new();
        for ch in "hello world".chars() {
            state.insert_char(ch);
        }
        state.delete_word_backward();
        assert_eq!(state.content(), "hello ");
        assert_eq!(state.cursor_position(), 6);
    }

    #[test]
    fn delete_word_backward_trailing_spaces() {
        let mut state = InputLineState::new();
        for ch in "hello   ".chars() {
            state.insert_char(ch);
        }
        state.delete_word_backward();
        assert_eq!(state.content(), "");
        assert_eq!(state.cursor_position(), 0);
    }

    #[test]
    fn delete_word_backward_at_start() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        state.delete_word_backward();
        assert_eq!(state.content(), "hello");
    }

    #[test]
    fn delete_word_backward_in_middle() {
        let mut state = InputLineState::new();
        for ch in "hello world test".chars() {
            state.insert_char(ch);
        }
        // Cursor after "hello world " (position 12).
        state.move_home();
        for _ in 0..12 {
            state.move_right();
        }
        state.delete_word_backward();
        assert_eq!(state.content(), "hello test");
        assert_eq!(state.cursor_position(), 6);
    }

    // ── Clear ─────────────────────────────────────────────────────

    #[test]
    fn clear_resets_all() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        state.clear();
        assert!(state.is_empty());
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);
        assert_eq!(state.scroll_offset(), 0);
    }

    // ── UTF-8 multi-byte characters ──────────────────────────────

    #[test]
    fn insert_emoji() {
        let mut state = InputLineState::new();
        state.insert_char('😀'); // 4-byte UTF-8
        assert_eq!(state.content(), "😀");
        assert_eq!(state.cursor_position(), 1);
        assert_eq!(state.cursor_byte_offset(), 4);
    }

    #[test]
    fn insert_cjk() {
        let mut state = InputLineState::new();
        state.insert_char('日'); // 3-byte UTF-8
        state.insert_char('本'); // 3-byte UTF-8
        assert_eq!(state.content(), "日本");
        assert_eq!(state.cursor_position(), 2);
        assert_eq!(state.cursor_byte_offset(), 6);
    }

    #[test]
    fn backspace_emoji() {
        let mut state = InputLineState::new();
        state.insert_char('a');
        state.insert_char('😀');
        state.insert_char('b');
        state.backspace(); // remove 'b'
        assert_eq!(state.content(), "a😀");
        state.backspace(); // remove emoji
        assert_eq!(state.content(), "a");
        assert_eq!(state.cursor_position(), 1);
        assert_eq!(state.cursor_byte_offset(), 1);
    }

    #[test]
    fn delete_cjk() {
        let mut state = InputLineState::new();
        for ch in "日本語".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        state.delete(); // delete '日'
        assert_eq!(state.content(), "本語");
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);
    }

    #[test]
    fn move_through_mixed_ascii_and_emoji() {
        let mut state = InputLineState::new();
        for ch in "a😀b".chars() {
            state.insert_char(ch);
        }
        // cursor at end: char=3, byte=6
        assert_eq!(state.cursor_position(), 3);
        assert_eq!(state.cursor_byte_offset(), 6);

        state.move_left(); // before 'b'
        assert_eq!(state.cursor_position(), 2);
        assert_eq!(state.cursor_byte_offset(), 5);

        state.move_left(); // before '😀'
        assert_eq!(state.cursor_position(), 1);
        assert_eq!(state.cursor_byte_offset(), 1);

        state.move_left(); // before 'a'
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);

        state.move_right(); // after 'a'
        assert_eq!(state.cursor_position(), 1);
        assert_eq!(state.cursor_byte_offset(), 1);

        state.move_right(); // after '😀'
        assert_eq!(state.cursor_position(), 2);
        assert_eq!(state.cursor_byte_offset(), 5);
    }

    #[test]
    fn delete_word_backward_with_cjk() {
        let mut state = InputLineState::new();
        for ch in "hello 日本語".chars() {
            state.insert_char(ch);
        }
        state.delete_word_backward();
        // CJK chars have no spaces so they're one "word".
        assert_eq!(state.content(), "hello ");
        assert_eq!(state.cursor_position(), 6);
    }

    #[test]
    fn insert_after_emoji_cursor_correct() {
        let mut state = InputLineState::new();
        state.insert_char('😀');
        state.insert_char('a');
        assert_eq!(state.content(), "😀a");
        assert_eq!(state.cursor_position(), 2);
        assert_eq!(state.cursor_byte_offset(), 5);
    }

    #[test]
    fn home_end_with_multibyte() {
        let mut state = InputLineState::new();
        for ch in "a😀日b".chars() {
            state.insert_char(ch);
        }
        state.move_home();
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);
        state.move_end();
        assert_eq!(state.cursor_position(), 4);
        // a(1) + 😀(4) + 日(3) + b(1) = 9 bytes
        assert_eq!(state.cursor_byte_offset(), 9);
    }

    // ── Edge cases ────────────────────────────────────────────────

    #[test]
    fn backspace_on_empty() {
        let mut state = InputLineState::new();
        assert!(!state.backspace());
        assert!(state.is_empty());
    }

    #[test]
    fn delete_on_empty() {
        let mut state = InputLineState::new();
        assert!(!state.delete());
        assert!(state.is_empty());
    }

    #[test]
    fn kill_to_end_on_empty() {
        let mut state = InputLineState::new();
        state.kill_to_end();
        assert!(state.is_empty());
    }

    #[test]
    fn delete_word_backward_on_empty() {
        let mut state = InputLineState::new();
        state.delete_word_backward();
        assert!(state.is_empty());
    }

    #[test]
    fn many_insertions_and_deletions() {
        let mut state = InputLineState::new();
        for ch in "the quick brown fox".chars() {
            state.insert_char(ch);
        }
        // Delete everything via backspace.
        while state.backspace() {}
        assert!(state.is_empty());
        assert_eq!(state.cursor_position(), 0);
        assert_eq!(state.cursor_byte_offset(), 0);
    }

    #[test]
    fn long_line_operations() {
        let mut state = InputLineState::new();
        let text = "a".repeat(1000);
        for ch in text.chars() {
            state.insert_char(ch);
        }
        assert_eq!(state.char_count(), 1000);
        assert_eq!(state.cursor_position(), 1000);
        state.move_home();
        assert_eq!(state.cursor_position(), 0);
        state.move_end();
        assert_eq!(state.cursor_position(), 1000);
    }

    // ── Scroll offset ─────────────────────────────────────────────

    #[test]
    fn scroll_offset_basic() {
        let mut state = InputLineState::new();
        for ch in "hello world, this is a long line".chars() {
            state.insert_char(ch);
        }
        // Visible width 10, cursor at end (pos 32).
        let offset = state.update_scroll(10);
        assert_eq!(offset, 23); // 32 - 10 + 1

        state.move_home();
        let offset = state.update_scroll(10);
        assert_eq!(offset, 0);
    }

    #[test]
    fn scroll_offset_zero_width() {
        let mut state = InputLineState::new();
        for ch in "hello".chars() {
            state.insert_char(ch);
        }
        let offset = state.update_scroll(0);
        assert_eq!(offset, 0);
    }

    #[test]
    fn scroll_no_adjustment_needed() {
        let mut state = InputLineState::new();
        for ch in "hi".chars() {
            state.insert_char(ch);
        }
        let offset = state.update_scroll(80);
        assert_eq!(offset, 0);
    }

    // ── Default impl ──────────────────────────────────────────────

    #[test]
    fn default_is_empty() {
        let state = InputLineState::default();
        assert!(state.is_empty());
    }
}
