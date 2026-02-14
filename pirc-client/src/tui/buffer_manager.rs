use std::fmt;

use super::message_buffer::{BufferLine, MessageBuffer};

/// Identifies a buffer by its type and name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BufferId {
    /// The server/status window (always exists, index 0).
    Status,
    /// A channel buffer (e.g., #general).
    Channel(String),
    /// A private message / query buffer.
    Query(String),
}

impl fmt::Display for BufferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BufferId::Status => write!(f, "Status"),
            BufferId::Channel(name) => write!(f, "{name}"),
            BufferId::Query(nick) => write!(f, "{nick}"),
        }
    }
}

/// Internal entry pairing a buffer ID with its message buffer and display label.
struct BufferEntry {
    id: BufferId,
    messages: MessageBuffer,
    label: String,
}

/// Manages multiple named buffers and tracks which one is active.
///
/// Pure state — no I/O. The Status buffer always lives at index 0 and
/// cannot be closed.
pub struct BufferManager {
    buffers: Vec<BufferEntry>,
    active_index: usize,
    default_scrollback: usize,
}

impl BufferManager {
    /// Create a new buffer manager with a Status buffer at index 0.
    pub fn new(default_scrollback: usize) -> Self {
        let status = BufferEntry {
            id: BufferId::Status,
            messages: MessageBuffer::new(default_scrollback),
            label: "Status".to_string(),
        };
        Self {
            buffers: vec![status],
            active_index: 0,
            default_scrollback,
        }
    }

    /// Open or get an existing buffer and switch to it.
    /// Returns a mutable reference to its `MessageBuffer`.
    pub fn open(&mut self, id: BufferId) -> &mut MessageBuffer {
        let index = if let Some(idx) = self.find_index(&id) {
            idx
        } else {
            let label = id.to_string();
            self.buffers.push(BufferEntry {
                id: id.clone(),
                messages: MessageBuffer::new(self.default_scrollback),
                label,
            });
            self.buffers.len() - 1
        };
        // Switch to this buffer and mark it read
        self.buffers[self.active_index].messages.mark_read();
        self.active_index = index;
        self.buffers[index].messages.mark_read();
        &mut self.buffers[index].messages
    }

    /// Ensure a buffer exists without switching to it.
    /// If the buffer already exists, this is a no-op.
    /// Returns `true` if the buffer was newly created.
    pub fn ensure_open(&mut self, id: BufferId) -> bool {
        if self.find_index(&id).is_some() {
            return false;
        }
        let label = id.to_string();
        self.buffers.push(BufferEntry {
            id,
            messages: MessageBuffer::new(self.default_scrollback),
            label,
        });
        true
    }

    /// Close a buffer. Cannot close the Status buffer.
    /// If the active buffer is closed, switches to the nearest neighbor.
    /// Returns `true` if the buffer was found and closed.
    pub fn close(&mut self, id: &BufferId) -> bool {
        if *id == BufferId::Status {
            return false;
        }
        let Some(index) = self.find_index(id) else {
            return false;
        };

        self.buffers.remove(index);

        // Adjust active_index after removal
        if self.active_index == index {
            // Closed the active buffer — pick nearest neighbor
            if self.active_index >= self.buffers.len() {
                self.active_index = self.buffers.len() - 1;
            }
            // Mark the newly-active buffer as read
            self.buffers[self.active_index].messages.mark_read();
        } else if self.active_index > index {
            self.active_index -= 1;
        }

        true
    }

    /// Get an immutable reference to a buffer's messages.
    pub fn get(&self, id: &BufferId) -> Option<&MessageBuffer> {
        self.buffers
            .iter()
            .find(|e| e.id == *id)
            .map(|e| &e.messages)
    }

    /// Get a mutable reference to a buffer's messages.
    pub fn get_mut(&mut self, id: &BufferId) -> Option<&mut MessageBuffer> {
        self.buffers
            .iter_mut()
            .find(|e| e.id == *id)
            .map(|e| &mut e.messages)
    }

    /// Get an immutable reference to the active buffer's messages.
    pub fn active(&self) -> &MessageBuffer {
        &self.buffers[self.active_index].messages
    }

    /// Get a mutable reference to the active buffer's messages.
    pub fn active_mut(&mut self) -> &mut MessageBuffer {
        &mut self.buffers[self.active_index].messages
    }

    /// Get the ID of the currently active buffer.
    pub fn active_id(&self) -> &BufferId {
        &self.buffers[self.active_index].id
    }

    /// Switch to the buffer with the given ID. Marks it as read.
    /// Returns `true` if the buffer was found and switched to.
    pub fn switch_to(&mut self, id: &BufferId) -> bool {
        let Some(index) = self.find_index(id) else {
            return false;
        };
        self.active_index = index;
        self.buffers[index].messages.mark_read();
        true
    }

    /// Cycle to the next buffer (wraps around).
    pub fn switch_next(&mut self) {
        self.active_index = (self.active_index + 1) % self.buffers.len();
        self.buffers[self.active_index].messages.mark_read();
    }

    /// Cycle to the previous buffer (wraps around).
    pub fn switch_prev(&mut self) {
        if self.active_index == 0 {
            self.active_index = self.buffers.len() - 1;
        } else {
            self.active_index -= 1;
        }
        self.buffers[self.active_index].messages.mark_read();
    }

    /// Switch to a buffer by its position index. Marks it as read.
    /// Returns `true` if the index was valid.
    pub fn switch_by_index(&mut self, index: usize) -> bool {
        if index >= self.buffers.len() {
            return false;
        }
        self.active_index = index;
        self.buffers[index].messages.mark_read();
        true
    }

    /// Move a buffer from one position to another.
    /// Returns `false` if either index is out of bounds or if attempting to
    /// move from/to index 0 (Status buffer stays pinned).
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from == 0 || to == 0 {
            return false;
        }
        if from >= self.buffers.len() || to >= self.buffers.len() {
            return false;
        }
        if from == to {
            return true;
        }

        let entry = self.buffers.remove(from);
        self.buffers.insert(to, entry);

        // Update active_index to track whichever buffer was active
        if self.active_index == from {
            self.active_index = to;
        } else if from < to {
            // Moved forward: indices in (from..=to) shifted left by 1
            if self.active_index > from && self.active_index <= to {
                self.active_index -= 1;
            }
        } else {
            // Moved backward: indices in (to..from) shifted right by 1
            if self.active_index >= to && self.active_index < from {
                self.active_index += 1;
            }
        }

        true
    }

    /// Returns buffer metadata for tab-bar rendering:
    /// `(id, label, unread_count, has_activity)` for each buffer.
    pub fn buffer_list(&self) -> Vec<(BufferId, &str, usize, bool)> {
        self.buffers
            .iter()
            .map(|e| {
                (
                    e.id.clone(),
                    e.label.as_str(),
                    e.messages.unread_count(),
                    e.messages.has_activity(),
                )
            })
            .collect()
    }

    /// Return the number of buffers.
    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// Push a message to a specific buffer. For `Query` buffers, the buffer
    /// is auto-created if it doesn't exist. The unread count is only
    /// incremented by `MessageBuffer::push_message` itself; the active
    /// buffer is immediately marked read so the count stays at zero for it.
    pub fn push_to(&mut self, id: &BufferId, line: BufferLine) {
        let is_active_target = self.find_index(id) == Some(self.active_index);

        // Auto-create query buffers
        if self.find_index(id).is_none() {
            if let BufferId::Query(_) = id {
                let label = id.to_string();
                self.buffers.push(BufferEntry {
                    id: id.clone(),
                    messages: MessageBuffer::new(self.default_scrollback),
                    label,
                });
            } else {
                return; // Non-query buffers must be explicitly opened
            }
        }

        if let Some(entry) = self.buffers.iter_mut().find(|e| e.id == *id) {
            entry.messages.push_message(line);
            if is_active_target {
                entry.messages.mark_read();
            }
        }
    }

    /// Find the index of a buffer by its ID.
    pub fn find_index(&self, id: &BufferId) -> Option<usize> {
        self.buffers.iter().position(|e| e.id == *id)
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

    // --- Construction ---

    #[test]
    fn new_creates_status_buffer() {
        let mgr = BufferManager::new(100);
        assert_eq!(mgr.buffer_count(), 1);
        assert_eq!(*mgr.active_id(), BufferId::Status);
        assert_eq!(mgr.active().len(), 0);
    }

    #[test]
    fn status_always_at_index_zero() {
        let mgr = BufferManager::new(100);
        assert_eq!(mgr.find_index(&BufferId::Status), Some(0));
    }

    // --- BufferId Display ---

    #[test]
    fn buffer_id_display() {
        assert_eq!(BufferId::Status.to_string(), "Status");
        assert_eq!(BufferId::Channel("#rust".into()).to_string(), "#rust");
        assert_eq!(BufferId::Query("alice".into()).to_string(), "alice");
    }

    // --- Open ---

    #[test]
    fn open_creates_new_buffer() {
        let mut mgr = BufferManager::new(100);
        let buf = mgr.open(BufferId::Channel("#test".into()));
        assert_eq!(buf.len(), 0);
        assert_eq!(mgr.buffer_count(), 2);
        assert_eq!(*mgr.active_id(), BufferId::Channel("#test".into()));
    }

    #[test]
    fn open_existing_switches_to_it() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));

        mgr.open(BufferId::Channel("#a".into()));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#a".into()));
        assert_eq!(mgr.buffer_count(), 3); // Status + #a + #b
    }

    #[test]
    fn open_marks_buffer_read() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));

        // Push a message to #a while it's not active
        mgr.switch_to(&BufferId::Status);
        mgr.push_to(&BufferId::Channel("#a".into()), make_line("hello"));
        assert_eq!(mgr.get(&BufferId::Channel("#a".into())).unwrap().unread_count(), 1);

        // Opening #a should mark it read
        mgr.open(BufferId::Channel("#a".into()));
        assert_eq!(mgr.active().unread_count(), 0);
    }

    // --- Close ---

    #[test]
    fn close_removes_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        assert_eq!(mgr.buffer_count(), 2);

        assert!(mgr.close(&BufferId::Channel("#a".into())));
        assert_eq!(mgr.buffer_count(), 1);
        assert!(mgr.get(&BufferId::Channel("#a".into())).is_none());
    }

    #[test]
    fn close_status_fails() {
        let mut mgr = BufferManager::new(100);
        assert!(!mgr.close(&BufferId::Status));
        assert_eq!(mgr.buffer_count(), 1);
    }

    #[test]
    fn close_nonexistent_fails() {
        let mut mgr = BufferManager::new(100);
        assert!(!mgr.close(&BufferId::Channel("#nope".into())));
    }

    #[test]
    fn close_active_switches_to_neighbor() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.open(BufferId::Channel("#c".into()));
        // Active is #c (index 3)
        assert_eq!(*mgr.active_id(), BufferId::Channel("#c".into()));

        // Close #c → should switch to #b (index 2)
        mgr.close(&BufferId::Channel("#c".into()));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));
    }

    #[test]
    fn close_active_middle_switches_to_next() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.open(BufferId::Channel("#c".into()));
        mgr.switch_to(&BufferId::Channel("#b".into()));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));

        // Close #b (index 2) → #c takes index 2, active stays 2
        mgr.close(&BufferId::Channel("#b".into()));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#c".into()));
    }

    #[test]
    fn close_before_active_adjusts_index() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.switch_to(&BufferId::Channel("#b".into()));
        // Active is #b (index 2)

        mgr.close(&BufferId::Channel("#a".into()));
        // #a removed, active should still be #b (now at index 1)
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));
    }

    // --- Get / Get Mut ---

    #[test]
    fn get_returns_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#test".into()));
        assert!(mgr.get(&BufferId::Channel("#test".into())).is_some());
        assert!(mgr.get(&BufferId::Channel("#nope".into())).is_none());
    }

    #[test]
    fn get_mut_returns_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#test".into()));
        let buf = mgr.get_mut(&BufferId::Channel("#test".into())).unwrap();
        buf.push_message(make_line("hello"));
        assert_eq!(mgr.get(&BufferId::Channel("#test".into())).unwrap().len(), 1);
    }

    // --- Active ---

    #[test]
    fn active_returns_current() {
        let mut mgr = BufferManager::new(100);
        mgr.active_mut().push_message(make_line("status msg"));
        assert_eq!(mgr.active().len(), 1);
    }

    // --- Switch To ---

    #[test]
    fn switch_to_changes_active() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));

        assert!(mgr.switch_to(&BufferId::Channel("#a".into())));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#a".into()));
    }

    #[test]
    fn switch_to_marks_read() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.switch_to(&BufferId::Status);
        mgr.push_to(&BufferId::Channel("#a".into()), make_line("msg"));
        assert_eq!(mgr.get(&BufferId::Channel("#a".into())).unwrap().unread_count(), 1);

        mgr.switch_to(&BufferId::Channel("#a".into()));
        assert_eq!(mgr.active().unread_count(), 0);
    }

    #[test]
    fn switch_to_nonexistent_fails() {
        let mut mgr = BufferManager::new(100);
        assert!(!mgr.switch_to(&BufferId::Channel("#nope".into())));
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    // --- Switch Next / Prev ---

    #[test]
    fn switch_next_cycles() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.switch_to(&BufferId::Status);

        mgr.switch_next();
        assert_eq!(*mgr.active_id(), BufferId::Channel("#a".into()));
        mgr.switch_next();
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));
        mgr.switch_next(); // wraps
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    #[test]
    fn switch_prev_cycles() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.switch_to(&BufferId::Status);

        mgr.switch_prev(); // wraps to end
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));
        mgr.switch_prev();
        assert_eq!(*mgr.active_id(), BufferId::Channel("#a".into()));
        mgr.switch_prev();
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    #[test]
    fn switch_next_single_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.switch_next();
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    #[test]
    fn switch_prev_single_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.switch_prev();
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    // --- Switch By Index ---

    #[test]
    fn switch_by_index_works() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));

        assert!(mgr.switch_by_index(0));
        assert_eq!(*mgr.active_id(), BufferId::Status);
        assert!(mgr.switch_by_index(1));
        assert_eq!(*mgr.active_id(), BufferId::Channel("#a".into()));
    }

    #[test]
    fn switch_by_index_out_of_bounds() {
        let mut mgr = BufferManager::new(100);
        assert!(!mgr.switch_by_index(5));
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    // --- Reorder ---

    #[test]
    fn reorder_moves_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.open(BufferId::Channel("#c".into()));
        // Order: Status, #a, #b, #c

        assert!(mgr.reorder(1, 3)); // move #a to end
        let list: Vec<_> = mgr.buffer_list().into_iter().map(|(id, ..)| id).collect();
        assert_eq!(list, vec![
            BufferId::Status,
            BufferId::Channel("#b".into()),
            BufferId::Channel("#c".into()),
            BufferId::Channel("#a".into()),
        ]);
    }

    #[test]
    fn reorder_status_fails() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        assert!(!mgr.reorder(0, 1));
        assert!(!mgr.reorder(1, 0));
    }

    #[test]
    fn reorder_out_of_bounds() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        assert!(!mgr.reorder(1, 5));
        assert!(!mgr.reorder(5, 1));
    }

    #[test]
    fn reorder_same_position() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        assert!(mgr.reorder(1, 1));
    }

    #[test]
    fn reorder_tracks_active_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.open(BufferId::Channel("#c".into()));
        // Active is #c (index 3)
        mgr.switch_to(&BufferId::Channel("#a".into())); // index 1

        // Move #a from 1 to 3
        mgr.reorder(1, 3);
        assert_eq!(*mgr.active_id(), BufferId::Channel("#a".into()));
    }

    #[test]
    fn reorder_adjusts_active_when_moved_forward() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.open(BufferId::Channel("#c".into()));
        mgr.switch_to(&BufferId::Channel("#b".into())); // index 2

        // Move #a (1) to 3 — #b shifts from index 2 to 1
        mgr.reorder(1, 3);
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));
    }

    #[test]
    fn reorder_adjusts_active_when_moved_backward() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));
        mgr.open(BufferId::Channel("#c".into()));
        mgr.switch_to(&BufferId::Channel("#b".into())); // index 2

        // Move #c (3) to 1 — #b shifts from index 2 to 3
        mgr.reorder(3, 1);
        assert_eq!(*mgr.active_id(), BufferId::Channel("#b".into()));
    }

    // --- Buffer List ---

    #[test]
    fn buffer_list_returns_all_buffers() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Query("bob".into()));

        let list = mgr.buffer_list();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].0, BufferId::Status);
        assert_eq!(list[0].1, "Status");
        assert_eq!(list[1].0, BufferId::Channel("#a".into()));
        assert_eq!(list[1].1, "#a");
        assert_eq!(list[2].0, BufferId::Query("bob".into()));
        assert_eq!(list[2].1, "bob");
    }

    #[test]
    fn buffer_list_shows_unread_and_activity() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.switch_to(&BufferId::Status);

        mgr.push_to(&BufferId::Channel("#a".into()), make_line("msg1"));
        mgr.push_to(&BufferId::Channel("#a".into()), make_line("msg2"));

        let list = mgr.buffer_list();
        // #a should have 2 unread and activity
        assert_eq!(list[1].2, 2); // unread_count
        assert!(list[1].3); // has_activity
    }

    // --- Push To ---

    #[test]
    fn push_to_existing_buffer() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.switch_to(&BufferId::Status);

        mgr.push_to(&BufferId::Channel("#a".into()), make_line("hello"));
        assert_eq!(mgr.get(&BufferId::Channel("#a".into())).unwrap().len(), 1);
    }

    #[test]
    fn push_to_active_buffer_marks_read() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        // #a is active

        mgr.push_to(&BufferId::Channel("#a".into()), make_line("hello"));
        assert_eq!(mgr.active().unread_count(), 0);
    }

    #[test]
    fn push_to_inactive_buffer_increments_unread() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.switch_to(&BufferId::Status);

        mgr.push_to(&BufferId::Channel("#a".into()), make_line("hello"));
        assert_eq!(mgr.get(&BufferId::Channel("#a".into())).unwrap().unread_count(), 1);
    }

    #[test]
    fn push_to_auto_creates_query() {
        let mut mgr = BufferManager::new(100);
        mgr.push_to(&BufferId::Query("alice".into()), make_line("hi"));
        assert_eq!(mgr.buffer_count(), 2);
        assert_eq!(mgr.get(&BufferId::Query("alice".into())).unwrap().len(), 1);
    }

    #[test]
    fn push_to_does_not_auto_create_channel() {
        let mut mgr = BufferManager::new(100);
        mgr.push_to(&BufferId::Channel("#nope".into()), make_line("hi"));
        assert_eq!(mgr.buffer_count(), 1); // Only Status
    }

    #[test]
    fn push_to_status() {
        let mut mgr = BufferManager::new(100);
        mgr.push_to(&BufferId::Status, make_line("server msg"));
        assert_eq!(mgr.get(&BufferId::Status).unwrap().len(), 1);
        // Status is active, so unread should be 0
        assert_eq!(mgr.active().unread_count(), 0);
    }

    // --- Find Index ---

    #[test]
    fn find_index_returns_correct_position() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));

        assert_eq!(mgr.find_index(&BufferId::Status), Some(0));
        assert_eq!(mgr.find_index(&BufferId::Channel("#a".into())), Some(1));
        assert_eq!(mgr.find_index(&BufferId::Channel("#b".into())), Some(2));
        assert_eq!(mgr.find_index(&BufferId::Channel("#c".into())), None);
    }

    // --- Combined workflows ---

    #[test]
    fn full_workflow() {
        let mut mgr = BufferManager::new(100);

        // Open channels
        mgr.open(BufferId::Channel("#general".into()));
        mgr.open(BufferId::Channel("#rust".into()));
        assert_eq!(mgr.buffer_count(), 3);

        // Push messages while viewing #rust
        mgr.push_to(&BufferId::Channel("#general".into()), make_line("hello"));
        mgr.push_to(&BufferId::Channel("#general".into()), make_line("world"));
        mgr.push_to(&BufferId::Channel("#rust".into()), make_line("hi"));

        // #general has unread, #rust (active) does not
        assert_eq!(mgr.get(&BufferId::Channel("#general".into())).unwrap().unread_count(), 2);
        assert_eq!(mgr.active().unread_count(), 0);

        // Switch to #general — unread clears
        mgr.switch_to(&BufferId::Channel("#general".into()));
        assert_eq!(mgr.active().unread_count(), 0);

        // Close #rust
        mgr.close(&BufferId::Channel("#rust".into()));
        assert_eq!(mgr.buffer_count(), 2);

        // Cycle through remaining
        mgr.switch_next(); // #general → Status (wraps from index 1 to 0)
        assert_eq!(*mgr.active_id(), BufferId::Status);
        mgr.switch_next();
        assert_eq!(*mgr.active_id(), BufferId::Channel("#general".into()));
    }

    #[test]
    fn query_auto_create_workflow() {
        let mut mgr = BufferManager::new(100);

        // Receive a private message from alice (auto-creates query buffer)
        mgr.push_to(&BufferId::Query("alice".into()), make_line("hey!"));
        assert_eq!(mgr.buffer_count(), 2);

        // It should have 1 unread (since Status is active)
        assert_eq!(mgr.get(&BufferId::Query("alice".into())).unwrap().unread_count(), 1);

        // Switch to alice's query
        mgr.switch_to(&BufferId::Query("alice".into()));
        assert_eq!(mgr.active().unread_count(), 0);

        // Close the query
        mgr.close(&BufferId::Query("alice".into()));
        assert_eq!(mgr.buffer_count(), 1);
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }

    #[test]
    fn buffer_list_order_matches_insertion() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#c".into()));
        mgr.open(BufferId::Channel("#a".into()));
        mgr.open(BufferId::Channel("#b".into()));

        let labels: Vec<&str> = mgr.buffer_list().iter().map(|e| e.1).collect();
        assert_eq!(labels, vec!["Status", "#c", "#a", "#b"]);
    }

    #[test]
    fn close_last_buffer_switches_to_previous() {
        let mut mgr = BufferManager::new(100);
        mgr.open(BufferId::Channel("#a".into()));
        // Active is #a (index 1)

        mgr.close(&BufferId::Channel("#a".into()));
        assert_eq!(*mgr.active_id(), BufferId::Status);
    }
}
