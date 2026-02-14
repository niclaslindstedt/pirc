/// Input history with up/down arrow navigation.
///
/// Stores previous input lines and allows the user to navigate through them
/// using up/down arrows, preserving any in-progress draft text.
///
/// Design:
/// - `entries[0]` is the oldest entry, `entries[len-1]` is the most recent.
/// - `cursor` indexes into entries when navigating. A value of `entries.len()`
///   means "not browsing history" (i.e. showing the draft/current line).
/// - `draft` holds whatever the user had typed before they started pressing Up,
///   so pressing Down back to the bottom restores it.
#[derive(Debug, Clone)]
pub struct InputHistory {
    entries: Vec<String>,
    max_size: usize,
    /// Current navigation position. Equal to `entries.len()` when not browsing.
    cursor: usize,
    /// The in-progress text that was on the input line before history navigation began.
    draft: String,
}

impl InputHistory {
    /// Create a new history with the given maximum capacity.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_size,
            cursor: 0,
            draft: String::new(),
        }
    }

    /// Push a completed input line into history.
    ///
    /// - Empty or whitespace-only strings are ignored.
    /// - Consecutive duplicates are suppressed.
    /// - If the history exceeds `max_size`, the oldest entry is removed.
    ///
    /// After pushing, the navigation cursor is reset to the end (no browsing).
    pub fn push(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }

        // Suppress consecutive duplicates.
        if self.entries.last().is_some_and(|last| last == line) {
            self.reset_navigation();
            return;
        }

        self.entries.push(line.to_owned());

        // Evict oldest if over capacity.
        if self.entries.len() > self.max_size {
            self.entries.remove(0);
        }

        self.reset_navigation();
    }

    /// Navigate up (toward older entries).
    ///
    /// If `current_text` is the text currently on the input line and the cursor
    /// is at the bottom (not browsing), it is saved as the draft.
    ///
    /// Returns `Some(&str)` with the history entry to display, or `None` if
    /// already at the oldest entry or history is empty.
    pub fn navigate_up(&mut self, current_text: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        // If we are at the bottom (not browsing yet), save draft.
        if self.cursor == self.entries.len() {
            current_text.clone_into(&mut self.draft);
        }

        if self.cursor == 0 {
            // Already at oldest — don't wrap around.
            return None;
        }

        self.cursor -= 1;
        Some(&self.entries[self.cursor])
    }

    /// Navigate down (toward newer entries / the draft).
    ///
    /// Returns `Some(&str)` with the history entry or draft to display, or
    /// `None` if already at the bottom.
    pub fn navigate_down(&mut self) -> Option<&str> {
        if self.cursor >= self.entries.len() {
            // Already at bottom — nothing to do.
            return None;
        }

        self.cursor += 1;

        if self.cursor == self.entries.len() {
            // Back to draft.
            Some(&self.draft)
        } else {
            Some(&self.entries[self.cursor])
        }
    }

    /// Reset navigation state to "not browsing".
    ///
    /// Called after pushing a new entry or when the user submits input.
    pub fn reset_navigation(&mut self) {
        self.cursor = self.entries.len();
        self.draft.clear();
    }

    /// Return the number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return `true` if currently browsing history (cursor is not at the bottom).
    pub fn is_browsing(&self) -> bool {
        self.cursor < self.entries.len()
    }
}

impl Default for InputHistory {
    fn default() -> Self {
        Self::new(500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ──────────────────────────────────────────────

    #[test]
    fn new_is_empty() {
        let h = InputHistory::new(100);
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(!h.is_browsing());
    }

    #[test]
    fn default_has_capacity() {
        let h = InputHistory::default();
        assert!(h.is_empty());
        assert_eq!(h.max_size, 500);
    }

    // ── Push ─────────────────────────────────────────────────────

    #[test]
    fn push_adds_entry() {
        let mut h = InputHistory::new(10);
        h.push("hello");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn push_ignores_empty() {
        let mut h = InputHistory::new(10);
        h.push("");
        h.push("   ");
        h.push("\t\n");
        assert!(h.is_empty());
    }

    #[test]
    fn push_suppresses_consecutive_duplicates() {
        let mut h = InputHistory::new(10);
        h.push("hello");
        h.push("hello");
        h.push("hello");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn push_allows_non_consecutive_duplicates() {
        let mut h = InputHistory::new(10);
        h.push("hello");
        h.push("world");
        h.push("hello");
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn push_evicts_oldest_when_full() {
        let mut h = InputHistory::new(3);
        h.push("one");
        h.push("two");
        h.push("three");
        h.push("four");
        assert_eq!(h.len(), 3);
        // "one" was evicted; oldest is now "two".
        let mut h2 = h.clone();
        let entry = h2.navigate_up("").unwrap();
        assert_eq!(entry, "four");
    }

    #[test]
    fn push_resets_navigation() {
        let mut h = InputHistory::new(10);
        h.push("one");
        h.push("two");
        h.navigate_up("");
        assert!(h.is_browsing());
        h.push("three");
        assert!(!h.is_browsing());
    }

    // ── Navigate up ──────────────────────────────────────────────

    #[test]
    fn navigate_up_empty_history() {
        let mut h = InputHistory::new(10);
        assert!(h.navigate_up("draft").is_none());
    }

    #[test]
    fn navigate_up_returns_most_recent_first() {
        let mut h = InputHistory::new(10);
        h.push("first");
        h.push("second");
        h.push("third");

        assert_eq!(h.navigate_up("draft"), Some("third"));
        assert_eq!(h.navigate_up("draft"), Some("second"));
        assert_eq!(h.navigate_up("draft"), Some("first"));
    }

    #[test]
    fn navigate_up_stops_at_oldest() {
        let mut h = InputHistory::new(10);
        h.push("only");

        assert_eq!(h.navigate_up("draft"), Some("only"));
        assert!(h.navigate_up("draft").is_none()); // Already at oldest.
    }

    #[test]
    fn navigate_up_saves_draft() {
        let mut h = InputHistory::new(10);
        h.push("old");

        h.navigate_up("my draft text");
        assert!(h.is_browsing());
        // Navigate back down to restore draft.
        let restored = h.navigate_down().unwrap();
        assert_eq!(restored, "my draft text");
    }

    // ── Navigate down ────────────────────────────────────────────

    #[test]
    fn navigate_down_at_bottom() {
        let mut h = InputHistory::new(10);
        h.push("entry");
        // Not browsing — down does nothing.
        assert!(h.navigate_down().is_none());
    }

    #[test]
    fn navigate_down_through_entries() {
        let mut h = InputHistory::new(10);
        h.push("first");
        h.push("second");
        h.push("third");

        // Go all the way up.
        h.navigate_up("draft");
        h.navigate_up("draft");
        h.navigate_up("draft");

        // Now go back down.
        assert_eq!(h.navigate_down(), Some("second"));
        assert_eq!(h.navigate_down(), Some("third"));
        assert_eq!(h.navigate_down(), Some("draft")); // Back to draft.
        assert!(h.navigate_down().is_none()); // Already at bottom.
    }

    #[test]
    fn navigate_down_restores_draft() {
        let mut h = InputHistory::new(10);
        h.push("old entry");

        h.navigate_up("typing something");
        let draft = h.navigate_down().unwrap();
        assert_eq!(draft, "typing something");
    }

    // ── Full cycle: up, down, up ─────────────────────────────────

    #[test]
    fn full_navigation_cycle() {
        let mut h = InputHistory::new(10);
        h.push("alpha");
        h.push("beta");
        h.push("gamma");

        // Up from bottom.
        assert_eq!(h.navigate_up("current"), Some("gamma"));
        assert_eq!(h.navigate_up("current"), Some("beta"));

        // Down partway.
        assert_eq!(h.navigate_down(), Some("gamma"));

        // Up again.
        assert_eq!(h.navigate_up("current"), Some("beta"));
        assert_eq!(h.navigate_up("current"), Some("alpha"));
        assert!(h.navigate_up("current").is_none()); // At oldest.

        // Down all the way to draft.
        assert_eq!(h.navigate_down(), Some("beta"));
        assert_eq!(h.navigate_down(), Some("gamma"));
        assert_eq!(h.navigate_down(), Some("current"));
        assert!(h.navigate_down().is_none());
    }

    // ── Draft preservation ───────────────────────────────────────

    #[test]
    fn draft_only_saved_once() {
        let mut h = InputHistory::new(10);
        h.push("entry1");
        h.push("entry2");

        // First Up saves the draft.
        h.navigate_up("my draft");
        // Second Up should NOT overwrite the draft.
        h.navigate_up("this should be ignored");

        // Go all the way down.
        h.navigate_down();
        let draft = h.navigate_down().unwrap();
        assert_eq!(draft, "my draft");
    }

    #[test]
    fn draft_cleared_on_push() {
        let mut h = InputHistory::new(10);
        h.push("entry");

        h.navigate_up("draft text");
        h.push("new entry");

        // Draft was cleared; start fresh.
        h.navigate_up("");
        let down = h.navigate_down().unwrap();
        assert_eq!(down, "");
    }

    // ── Edge cases ───────────────────────────────────────────────

    #[test]
    fn single_entry_up_down() {
        let mut h = InputHistory::new(10);
        h.push("only");

        assert_eq!(h.navigate_up("draft"), Some("only"));
        assert_eq!(h.navigate_down(), Some("draft"));
        assert!(!h.is_browsing());
    }

    #[test]
    fn max_size_one() {
        let mut h = InputHistory::new(1);
        h.push("first");
        h.push("second");
        assert_eq!(h.len(), 1);
        assert_eq!(h.navigate_up(""), Some("second"));
    }

    #[test]
    fn push_after_browsing_resets_cursor() {
        let mut h = InputHistory::new(10);
        h.push("one");
        h.push("two");

        h.navigate_up("draft");
        assert!(h.is_browsing());

        h.push("three");
        assert!(!h.is_browsing());

        // Next up should give "three".
        assert_eq!(h.navigate_up(""), Some("three"));
    }

    #[test]
    fn navigate_up_with_empty_draft() {
        let mut h = InputHistory::new(10);
        h.push("entry");

        assert_eq!(h.navigate_up(""), Some("entry"));
        assert_eq!(h.navigate_down(), Some(""));
    }

    #[test]
    fn unicode_entries() {
        let mut h = InputHistory::new(10);
        h.push("日本語テスト");
        h.push("Hello 🌍");

        assert_eq!(h.navigate_up("草稿"), Some("Hello 🌍"));
        assert_eq!(h.navigate_up("草稿"), Some("日本語テスト"));
        assert_eq!(h.navigate_down(), Some("Hello 🌍"));
        assert_eq!(h.navigate_down(), Some("草稿"));
    }

    #[test]
    fn reset_navigation_clears_browsing() {
        let mut h = InputHistory::new(10);
        h.push("entry");
        h.navigate_up("draft");
        assert!(h.is_browsing());
        h.reset_navigation();
        assert!(!h.is_browsing());
    }

    #[test]
    fn many_entries_navigate_all() {
        let mut h = InputHistory::new(100);
        for i in 0..50 {
            h.push(&format!("entry {i}"));
        }
        assert_eq!(h.len(), 50);

        // Navigate all the way up.
        for i in (0..50).rev() {
            let entry = h.navigate_up("draft").unwrap();
            assert_eq!(entry, format!("entry {i}"));
        }
        assert!(h.navigate_up("draft").is_none());

        // Navigate all the way back down.
        for i in 1..50 {
            let entry = h.navigate_down().unwrap();
            assert_eq!(entry, format!("entry {i}"));
        }
        let draft = h.navigate_down().unwrap();
        assert_eq!(draft, "draft");
    }
}
