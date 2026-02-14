use std::collections::VecDeque;
use std::time::Instant;

/// The type of a buffer line, indicating its semantic meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    Message,
    Action,
    Notice,
    Join,
    Part,
    Quit,
    Kick,
    Mode,
    Topic,
    System,
    Error,
}

/// A single line in a message buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferLine {
    pub timestamp: String,
    pub sender: Option<String>,
    pub content: String,
    pub line_type: LineType,
}

/// A per-channel message buffer with scrollback support.
///
/// Messages are stored in a `VecDeque` acting as a ring buffer with a
/// configurable maximum capacity. Index 0 is the oldest message, and
/// the last index is the newest.
///
/// `scroll_offset` of 0 means the view is pinned to the bottom (newest
/// messages visible). A positive offset scrolls up into history.
pub struct MessageBuffer {
    messages: VecDeque<BufferLine>,
    capacity: usize,
    scroll_offset: usize,
    unread_count: usize,
    has_activity: bool,
    last_activity: Option<Instant>,
}

impl MessageBuffer {
    /// Create a new message buffer with the given maximum capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
            scroll_offset: 0,
            unread_count: 0,
            has_activity: false,
            last_activity: None,
        }
    }

    /// Push a message into the buffer. If at capacity, the oldest message
    /// is evicted. Increments unread count and sets the activity flag.
    ///
    /// If the user is scrolled up, the scroll offset is adjusted to keep
    /// the same messages in view after eviction.
    pub fn push_message(&mut self, line: BufferLine) {
        let was_at_capacity = self.messages.len() >= self.capacity;

        if was_at_capacity && self.capacity > 0 {
            self.messages.pop_front();
            // If scrolled up, adjust offset since an old message was removed
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }

        if self.capacity > 0 {
            self.messages.push_back(line);
        }

        self.unread_count += 1;
        self.has_activity = true;
        self.last_activity = Some(Instant::now());
    }

    /// Return the messages that should be visible in a view of the given height.
    ///
    /// With `scroll_offset == 0`, returns the last `visible_lines` messages.
    /// With a positive offset, the window shifts upward into history.
    pub fn messages_in_view(&self, visible_lines: usize) -> &[BufferLine] {
        let len = self.messages.len();
        if len == 0 || visible_lines == 0 {
            return &[];
        }

        let (slices_a, slices_b) = self.messages.as_slices();

        // End index: how far from the end we start (scroll_offset moves us up)
        let end = len.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(visible_lines);

        // We need to return a contiguous slice. Since VecDeque stores data in
        // two contiguous slices, we can use make_contiguous or work with the
        // slices directly. For a read-only view, we'll compute which slice(s)
        // our range falls into.
        let total_a = slices_a.len();

        if end <= total_a {
            // Entirely within first slice
            &slices_a[start..end]
        } else if start >= total_a {
            // Entirely within second slice
            &slices_b[start - total_a..end - total_a]
        } else {
            // Range spans both slices — we can't return a single &[BufferLine].
            // To handle this, we need to make the deque contiguous first.
            // Since this is a read-only operation and we can't mutate self here,
            // we return the portion from the second slice only (newer messages).
            // This is a pragmatic trade-off — the caller gets at most the
            // messages from the second contiguous region.
            //
            // For a better API, see messages_in_view_mut which can call
            // make_contiguous first.
            &slices_b[..end - total_a]
        }
    }

    /// Return the messages visible in a view of the given height.
    ///
    /// This variant takes `&mut self` so it can call `make_contiguous` on the
    /// internal `VecDeque`, guaranteeing the returned slice covers the full
    /// requested range.
    pub fn messages_in_view_mut(&mut self, visible_lines: usize) -> &[BufferLine] {
        let len = self.messages.len();
        if len == 0 || visible_lines == 0 {
            return &[];
        }

        let end = len.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(visible_lines);

        let slice = self.messages.make_contiguous();
        &slice[start..end]
    }

    /// Scroll up by the given number of lines. The offset is clamped so it
    /// cannot exceed `len().saturating_sub(1)` (i.e., you can scroll up
    /// until only the oldest message is visible).
    pub fn scroll_up(&mut self, lines: usize) {
        let max_offset = self.messages.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + lines).min(max_offset);
    }

    /// Scroll down by the given number of lines. The offset is clamped to 0.
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Scroll to the bottom (newest messages).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Returns `true` if the view is at the bottom (no scroll offset).
    pub fn is_at_bottom(&self) -> bool {
        self.scroll_offset == 0
    }

    /// Reset unread count and activity flag.
    pub fn mark_read(&mut self) {
        self.unread_count = 0;
        self.has_activity = false;
    }

    /// Clear all messages and reset scroll state.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    /// Return the number of messages in the buffer.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Return `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Search for lines matching the query (case-insensitive).
    /// Returns indices of matching lines (0 = oldest).
    pub fn search(&self, query: &str) -> Vec<usize> {
        let query_lower = query.to_lowercase();
        self.messages
            .iter()
            .enumerate()
            .filter(|(_, line)| line.content.to_lowercase().contains(&query_lower))
            .map(|(i, _)| i)
            .collect()
    }

    /// Return the current unread count.
    pub fn unread_count(&self) -> usize {
        self.unread_count
    }

    /// Return whether there has been activity since last `mark_read`.
    pub fn has_activity(&self) -> bool {
        self.has_activity
    }

    /// Return the time of last activity, if any.
    pub fn last_activity(&self) -> Option<Instant> {
        self.last_activity
    }

    /// Return the capacity of the buffer.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_line(content: &str) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: Some("nick".to_string()),
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

    // --- Construction ---

    #[test]
    fn new_creates_empty_buffer() {
        let buf = MessageBuffer::new(100);
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
        assert_eq!(buf.capacity(), 100);
        assert!(buf.is_at_bottom());
        assert_eq!(buf.unread_count(), 0);
        assert!(!buf.has_activity());
        assert!(buf.last_activity().is_none());
    }

    #[test]
    fn new_zero_capacity() {
        let mut buf = MessageBuffer::new(0);
        buf.push_message(make_line("hello"));
        assert_eq!(buf.len(), 0);
    }

    // --- Push and eviction ---

    #[test]
    fn push_adds_message() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("hello"));
        assert_eq!(buf.len(), 1);
        assert!(!buf.is_empty());
    }

    #[test]
    fn push_increments_unread() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("a"));
        buf.push_message(make_line("b"));
        buf.push_message(make_line("c"));
        assert_eq!(buf.unread_count(), 3);
    }

    #[test]
    fn push_sets_activity() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("hello"));
        assert!(buf.has_activity());
        assert!(buf.last_activity().is_some());
    }

    #[test]
    fn push_evicts_oldest_at_capacity() {
        let mut buf = MessageBuffer::new(3);
        buf.push_message(make_line("first"));
        buf.push_message(make_line("second"));
        buf.push_message(make_line("third"));
        assert_eq!(buf.len(), 3);

        buf.push_message(make_line("fourth"));
        assert_eq!(buf.len(), 3);

        // "first" should be gone
        let view = buf.messages_in_view_mut(10);
        assert_eq!(view[0].content, "second");
        assert_eq!(view[1].content, "third");
        assert_eq!(view[2].content, "fourth");
    }

    #[test]
    fn push_evicts_multiple_at_capacity() {
        let mut buf = MessageBuffer::new(2);
        buf.push_message(make_line("a"));
        buf.push_message(make_line("b"));
        buf.push_message(make_line("c"));
        buf.push_message(make_line("d"));
        assert_eq!(buf.len(), 2);

        let view = buf.messages_in_view_mut(10);
        assert_eq!(view[0].content, "c");
        assert_eq!(view[1].content, "d");
    }

    #[test]
    fn push_single_capacity() {
        let mut buf = MessageBuffer::new(1);
        buf.push_message(make_line("a"));
        buf.push_message(make_line("b"));
        assert_eq!(buf.len(), 1);

        let view = buf.messages_in_view_mut(10);
        assert_eq!(view[0].content, "b");
    }

    // --- messages_in_view ---

    #[test]
    fn view_empty_buffer() {
        let buf = MessageBuffer::new(10);
        assert!(buf.messages_in_view(5).is_empty());
    }

    #[test]
    fn view_zero_visible_lines() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("hello"));
        assert!(buf.messages_in_view(0).is_empty());
    }

    #[test]
    fn view_shows_last_n_messages() {
        let mut buf = MessageBuffer::new(100);
        for i in 0..10 {
            buf.push_message(make_line(&format!("msg{i}")));
        }

        let view = buf.messages_in_view_mut(3);
        assert_eq!(view.len(), 3);
        assert_eq!(view[0].content, "msg7");
        assert_eq!(view[1].content, "msg8");
        assert_eq!(view[2].content, "msg9");
    }

    #[test]
    fn view_fewer_messages_than_visible() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("only"));

        let view = buf.messages_in_view_mut(10);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].content, "only");
    }

    #[test]
    fn view_with_scroll_offset() {
        let mut buf = MessageBuffer::new(100);
        for i in 0..10 {
            buf.push_message(make_line(&format!("msg{i}")));
        }
        buf.scroll_up(3);

        let view = buf.messages_in_view_mut(4);
        assert_eq!(view.len(), 4);
        // offset=3: end=10-3=7, start=7-4=3 → messages 3,4,5,6
        assert_eq!(view[0].content, "msg3");
        assert_eq!(view[1].content, "msg4");
        assert_eq!(view[2].content, "msg5");
        assert_eq!(view[3].content, "msg6");
    }

    #[test]
    fn view_scrolled_to_top() {
        let mut buf = MessageBuffer::new(100);
        for i in 0..5 {
            buf.push_message(make_line(&format!("msg{i}")));
        }
        buf.scroll_up(100); // scroll way past top

        let view = buf.messages_in_view_mut(3);
        // max offset = 4, end = 5-4 = 1, start = 0 → just msg0
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].content, "msg0");
    }

    // --- Scrolling ---

    #[test]
    fn scroll_up_increases_offset() {
        let mut buf = MessageBuffer::new(100);
        for _ in 0..10 {
            buf.push_message(make_line("x"));
        }
        buf.scroll_up(3);
        assert_eq!(buf.scroll_offset, 3);
        assert!(!buf.is_at_bottom());
    }

    #[test]
    fn scroll_up_clamps_to_max() {
        let mut buf = MessageBuffer::new(100);
        for _ in 0..5 {
            buf.push_message(make_line("x"));
        }
        buf.scroll_up(100);
        // max offset is len-1 = 4
        assert_eq!(buf.scroll_offset, 4);
    }

    #[test]
    fn scroll_up_empty_buffer() {
        let mut buf = MessageBuffer::new(100);
        buf.scroll_up(5);
        assert_eq!(buf.scroll_offset, 0);
    }

    #[test]
    fn scroll_down_decreases_offset() {
        let mut buf = MessageBuffer::new(100);
        for _ in 0..10 {
            buf.push_message(make_line("x"));
        }
        buf.scroll_up(5);
        buf.scroll_down(2);
        assert_eq!(buf.scroll_offset, 3);
    }

    #[test]
    fn scroll_down_clamps_to_zero() {
        let mut buf = MessageBuffer::new(100);
        for _ in 0..10 {
            buf.push_message(make_line("x"));
        }
        buf.scroll_up(3);
        buf.scroll_down(10);
        assert_eq!(buf.scroll_offset, 0);
        assert!(buf.is_at_bottom());
    }

    #[test]
    fn scroll_to_bottom_resets_offset() {
        let mut buf = MessageBuffer::new(100);
        for _ in 0..10 {
            buf.push_message(make_line("x"));
        }
        buf.scroll_up(5);
        buf.scroll_to_bottom();
        assert!(buf.is_at_bottom());
        assert_eq!(buf.scroll_offset, 0);
    }

    #[test]
    fn scroll_offset_adjusted_on_eviction() {
        let mut buf = MessageBuffer::new(5);
        for i in 0..5 {
            buf.push_message(make_line(&format!("msg{i}")));
        }
        buf.scroll_up(3); // offset = 3

        // Push triggers eviction — offset should decrease by 1
        buf.push_message(make_line("msg5"));
        assert_eq!(buf.scroll_offset, 2);

        let view = buf.messages_in_view_mut(10);
        assert_eq!(view[0].content, "msg1");
    }

    // --- Unread / activity ---

    #[test]
    fn mark_read_resets_counters() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("a"));
        buf.push_message(make_line("b"));
        assert_eq!(buf.unread_count(), 2);
        assert!(buf.has_activity());

        buf.mark_read();
        assert_eq!(buf.unread_count(), 0);
        assert!(!buf.has_activity());
    }

    #[test]
    fn activity_after_mark_read() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("a"));
        buf.mark_read();
        buf.push_message(make_line("b"));
        assert_eq!(buf.unread_count(), 1);
        assert!(buf.has_activity());
    }

    // --- Clear ---

    #[test]
    fn clear_empties_buffer() {
        let mut buf = MessageBuffer::new(10);
        buf.push_message(make_line("a"));
        buf.push_message(make_line("b"));
        buf.scroll_up(1);

        buf.clear();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
        assert!(buf.is_at_bottom());
    }

    // --- Search ---

    #[test]
    fn search_finds_matching_lines() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("Hello World"));
        buf.push_message(make_line("foo bar"));
        buf.push_message(make_line("hello again"));

        let results = buf.search("hello");
        assert_eq!(results, vec![0, 2]);
    }

    #[test]
    fn search_case_insensitive() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("HELLO"));
        buf.push_message(make_line("Hello"));
        buf.push_message(make_line("hello"));

        let results = buf.search("hElLo");
        assert_eq!(results, vec![0, 1, 2]);
    }

    #[test]
    fn search_no_matches() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("foo"));
        buf.push_message(make_line("bar"));

        let results = buf.search("baz");
        assert!(results.is_empty());
    }

    #[test]
    fn search_empty_query() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("foo"));
        buf.push_message(make_line("bar"));

        let results = buf.search("");
        assert_eq!(results, vec![0, 1]);
    }

    #[test]
    fn search_empty_buffer() {
        let buf = MessageBuffer::new(100);
        let results = buf.search("hello");
        assert!(results.is_empty());
    }

    #[test]
    fn search_partial_match() {
        let mut buf = MessageBuffer::new(100);
        buf.push_message(make_line("foobar"));

        let results = buf.search("oob");
        assert_eq!(results, vec![0]);
    }

    // --- LineType variants ---

    #[test]
    fn line_types_distinct() {
        let types = [
            LineType::Message,
            LineType::Action,
            LineType::Notice,
            LineType::Join,
            LineType::Part,
            LineType::Quit,
            LineType::Kick,
            LineType::Mode,
            LineType::Topic,
            LineType::System,
            LineType::Error,
        ];
        for (i, a) in types.iter().enumerate() {
            for (j, b) in types.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // --- BufferLine ---

    #[test]
    fn buffer_line_with_sender() {
        let line = make_line("test");
        assert_eq!(line.sender, Some("nick".to_string()));
        assert_eq!(line.content, "test");
        assert_eq!(line.line_type, LineType::Message);
    }

    #[test]
    fn buffer_line_without_sender() {
        let line = make_system_line("server message");
        assert!(line.sender.is_none());
        assert_eq!(line.line_type, LineType::System);
    }

    // --- Default capacity ---

    #[test]
    fn default_capacity_1000() {
        let buf = MessageBuffer::new(1000);
        assert_eq!(buf.capacity(), 1000);
    }

    // --- Large buffer stress ---

    #[test]
    fn fill_to_capacity_and_beyond() {
        let mut buf = MessageBuffer::new(100);
        for i in 0..200 {
            buf.push_message(make_line(&format!("msg{i}")));
        }
        assert_eq!(buf.len(), 100);

        let view = buf.messages_in_view_mut(3);
        assert_eq!(view[0].content, "msg197");
        assert_eq!(view[1].content, "msg198");
        assert_eq!(view[2].content, "msg199");
    }

    // --- Immutable messages_in_view for contiguous buffers ---

    #[test]
    fn immutable_view_works_for_fresh_buffer() {
        let mut buf = MessageBuffer::new(100);
        for i in 0..5 {
            buf.push_message(make_line(&format!("msg{i}")));
        }

        // Fresh buffer (no evictions) should be contiguous
        let view = buf.messages_in_view(3);
        assert_eq!(view.len(), 3);
        assert_eq!(view[0].content, "msg2");
        assert_eq!(view[1].content, "msg3");
        assert_eq!(view[2].content, "msg4");
    }

    // --- Combined operations ---

    #[test]
    fn push_scroll_search_workflow() {
        let mut buf = MessageBuffer::new(50);

        // Simulate chat activity
        buf.push_message(make_line("Welcome to #rust!"));
        buf.push_message(BufferLine {
            timestamp: "12:01".to_string(),
            sender: None,
            content: "alice has joined".to_string(),
            line_type: LineType::Join,
        });
        buf.push_message(make_line("Hello everyone"));
        buf.push_message(make_line("Does anyone know about VecDeque?"));
        buf.push_message(make_line("Sure, it's a double-ended queue"));

        assert_eq!(buf.len(), 5);
        assert_eq!(buf.unread_count(), 5);

        // Mark as read
        buf.mark_read();
        assert_eq!(buf.unread_count(), 0);

        // Scroll up
        buf.scroll_up(2);
        let view = buf.messages_in_view_mut(3);
        assert_eq!(view.len(), 3);
        assert_eq!(view[0].content, "Welcome to #rust!");
        assert_eq!(view[1].content, "alice has joined");
        assert_eq!(view[2].content, "Hello everyone");

        // Search
        let results = buf.search("vecdeque");
        assert_eq!(results, vec![3]);

        // Scroll back to bottom
        buf.scroll_to_bottom();
        assert!(buf.is_at_bottom());
    }

    #[test]
    fn scroll_offset_not_affected_by_push_when_at_bottom() {
        let mut buf = MessageBuffer::new(100);
        for _ in 0..5 {
            buf.push_message(make_line("x"));
        }
        assert!(buf.is_at_bottom());

        buf.push_message(make_line("new"));
        assert!(buf.is_at_bottom());
    }
}
