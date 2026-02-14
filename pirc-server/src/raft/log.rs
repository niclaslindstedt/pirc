use super::types::{LogEntry, LogIndex, Term};

/// In-memory replicated log for Raft consensus.
///
/// Log indices are 1-based: the first entry has index 1, and index 0
/// means "no entries". After log compaction, entries before the snapshot
/// point are removed. The `offset` field tracks the index of the last
/// compacted entry so logical indices still map correctly.
///
/// When `offset > 0`, the internal `Vec` starts at `offset + 1`.
/// For example, if `offset = 5`, then `entries[0]` corresponds to
/// `LogIndex(6)`.
///
/// The `snapshot_term` field stores the term of the last compacted entry
/// so that `term_at(offset)` can return the correct value for the
/// consistency check when building `AppendEntries`.
#[derive(Debug, Clone)]
pub struct RaftLog<T> {
    entries: Vec<LogEntry<T>>,
    /// Log index of the last entry included in the most recent snapshot.
    /// Entries up to and including this index have been compacted away.
    offset: u64,
    /// Term of the entry at `offset` (needed for `prev_log_term` checks).
    snapshot_term: Term,
}

#[allow(clippy::cast_possible_truncation)] // u64→usize: log indices fit on 64-bit
impl<T: Clone + PartialEq> RaftLog<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            offset: 0,
            snapshot_term: Term::default(),
        }
    }

    /// The log offset (index of the last compacted/snapshotted entry).
    pub fn offset(&self) -> LogIndex {
        LogIndex::new(self.offset)
    }

    /// The term of the last compacted/snapshotted entry.
    pub fn snapshot_term(&self) -> Term {
        self.snapshot_term
    }

    /// Convert a logical 1-based log index to an internal array index.
    /// Returns `None` if the index is at or below the offset, or beyond
    /// the end of the log.
    fn to_internal(&self, index: LogIndex) -> Option<usize> {
        let i = index.as_u64();
        if i <= self.offset {
            return None;
        }
        let internal = (i - self.offset) as usize - 1;
        if internal >= self.entries.len() {
            return None;
        }
        Some(internal)
    }

    /// Append an entry to the end of the log.
    pub fn append(&mut self, entry: LogEntry<T>) {
        self.entries.push(entry);
    }

    /// Get the entry at the given 1-based log index.
    pub fn get(&self, index: LogIndex) -> Option<&LogEntry<T>> {
        self.to_internal(index).map(|i| &self.entries[i])
    }

    /// The index of the last log entry, or `LogIndex(offset)` if the log
    /// has no entries beyond the snapshot.
    pub fn last_index(&self) -> LogIndex {
        LogIndex::new(self.offset + self.entries.len() as u64)
    }

    /// The term of the last log entry, or the snapshot term if the log
    /// has no entries beyond the snapshot.
    pub fn last_term(&self) -> Term {
        self.entries
            .last()
            .map_or(self.snapshot_term, |e| e.term)
    }

    /// Return a slice of entries in the range `[from, to)` (1-based, exclusive end).
    ///
    /// Returns an empty slice if the range is invalid or out of bounds.
    pub fn slice(&self, from: LogIndex, to: LogIndex) -> &[LogEntry<T>] {
        let start = from.as_u64();
        let end = to.as_u64();
        if start <= self.offset || start >= end {
            return &[];
        }
        let start_internal = (start - self.offset) as usize - 1;
        if start_internal >= self.entries.len() {
            return &[];
        }
        let end_internal = std::cmp::min(
            (end - self.offset) as usize - 1,
            self.entries.len(),
        );
        if end_internal <= start_internal {
            return &[];
        }
        &self.entries[start_internal..end_internal]
    }

    /// Remove all entries from `index` onward (inclusive, 1-based).
    ///
    /// Used for conflict resolution during log replication.
    /// Does not remove entries at or below the offset.
    pub fn truncate_from(&mut self, index: LogIndex) {
        let i = index.as_u64();
        if i <= self.offset {
            return;
        }
        let keep = (i - self.offset) as usize - 1;
        self.entries.truncate(keep);
    }

    /// Return a slice of all entries starting from `index` (1-based, inclusive).
    ///
    /// Returns an empty slice if the index is beyond the log end or at/below
    /// the offset.
    pub fn entries_from(&self, index: LogIndex) -> &[LogEntry<T>] {
        let i = index.as_u64();
        if i <= self.offset || i > self.offset + self.entries.len() as u64 {
            return &[];
        }
        let start = (i - self.offset) as usize - 1;
        &self.entries[start..]
    }

    /// Get the term of the entry at the given 1-based index.
    ///
    /// If the index equals the offset, returns the snapshot term (used for
    /// `prev_log_term` checks after compaction).
    pub fn term_at(&self, index: LogIndex) -> Option<Term> {
        let i = index.as_u64();
        if i == self.offset && self.offset > 0 {
            return Some(self.snapshot_term);
        }
        self.get(index).map(|e| e.term)
    }

    /// Number of entries currently in the log (not counting compacted entries).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log has no entries beyond the snapshot point.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append entries from a leader, handling conflict detection.
    ///
    /// If an existing entry conflicts with a new one (same index but different
    /// term), the existing entry and all that follow it are deleted, then the
    /// new entries are appended.
    ///
    /// Returns `true` if the `prev_log` check passed and entries were applied,
    /// `false` if the log doesn't contain an entry at `prev_log_index` with
    /// `prev_log_term` (log inconsistency).
    pub fn append_entries(
        &mut self,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        entries: &[LogEntry<T>],
    ) -> bool {
        // Check the log matching property: if prev_log_index > 0, the log must
        // contain an entry at that index with the matching term.
        if prev_log_index.as_u64() > 0 {
            match self.term_at(prev_log_index) {
                Some(term) if term == prev_log_term => {}
                _ => return false,
            }
        }

        // Append new entries, resolving conflicts.
        for entry in entries {
            let log_idx = entry.index.as_u64();
            // Skip entries that are at or below the compacted offset.
            if log_idx <= self.offset {
                continue;
            }
            let internal_pos = (log_idx - self.offset) as usize;
            if internal_pos <= self.entries.len() {
                // Entry position exists in current log — check for conflict.
                let existing = &self.entries[internal_pos - 1];
                if existing.term != entry.term {
                    // Conflict: delete this entry and everything after it.
                    self.entries.truncate(internal_pos - 1);
                    self.entries.push(entry.clone());
                }
                // If terms match, the entry is already correct — skip it.
            } else {
                // Beyond the current log end — just append.
                self.entries.push(entry.clone());
            }
        }

        true
    }

    /// Create a `RaftLog` from a vector of entries (e.g. loaded from storage).
    pub fn from_entries(entries: Vec<LogEntry<T>>) -> Self {
        Self {
            entries,
            offset: 0,
            snapshot_term: Term::default(),
        }
    }

    /// Get a reference to all entries as a slice.
    pub fn all_entries(&self) -> &[LogEntry<T>] {
        &self.entries
    }

    /// Compact the log up to the given index (inclusive).
    ///
    /// Removes all entries with index <= `up_to_index`. The term of the
    /// entry at `up_to_index` is saved as `snapshot_term` so that
    /// `term_at(offset)` still works for consistency checks.
    ///
    /// If `up_to_index` is beyond the log end or at/below the current
    /// offset, this is a no-op.
    pub fn compact_to(&mut self, up_to_index: LogIndex) {
        let target = up_to_index.as_u64();
        if target <= self.offset {
            return;
        }
        let last = self.last_index().as_u64();
        if target > last {
            return;
        }

        // Look up the term at the compaction point before removing entries.
        let compacted_term = self
            .get(up_to_index)
            .map_or(self.snapshot_term, |e| e.term);

        // Number of entries to remove from the front of the internal vec.
        let remove_count = (target - self.offset) as usize;
        self.entries.drain(..remove_count);
        self.offset = target;
        self.snapshot_term = compacted_term;
    }

    /// Discard the entire log and set the offset/term to the given
    /// snapshot point. Used when installing a snapshot that is ahead
    /// of our entire log.
    pub fn reset_to_snapshot(&mut self, last_included_index: LogIndex, last_included_term: Term) {
        self.entries.clear();
        self.offset = last_included_index.as_u64();
        self.snapshot_term = last_included_term;
    }

    /// Check whether a given log index has been compacted away (i.e. is
    /// at or below the snapshot offset). Returns `true` if entries at
    /// this index are no longer available in the log.
    pub fn is_compacted(&self, index: LogIndex) -> bool {
        index.as_u64() <= self.offset
    }
}

impl<T: Clone + PartialEq> Default for RaftLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: u64, index: u64) -> LogEntry<String> {
        LogEntry {
            term: Term::new(term),
            index: LogIndex::new(index),
            command: format!("cmd-{index}"),
        }
    }

    // ---- Construction ----

    #[test]
    fn new_log_is_empty() {
        let log: RaftLog<String> = RaftLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert_eq!(log.last_index(), LogIndex::new(0));
        assert_eq!(log.last_term(), Term::default());
    }

    #[test]
    fn default_log_is_empty() {
        let log: RaftLog<String> = RaftLog::default();
        assert!(log.is_empty());
    }

    // ---- Append & Get ----

    #[test]
    fn append_and_get_single_entry() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        assert_eq!(log.len(), 1);
        assert!(!log.is_empty());
        let e = log.get(LogIndex::new(1)).unwrap();
        assert_eq!(e.term, Term::new(1));
        assert_eq!(e.index, LogIndex::new(1));
    }

    #[test]
    fn append_multiple_entries() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));
        log.append(entry(2, 3));
        assert_eq!(log.len(), 3);
        assert_eq!(log.last_index(), LogIndex::new(3));
        assert_eq!(log.last_term(), Term::new(2));
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        assert!(log.get(LogIndex::new(0)).is_none());
        assert!(log.get(LogIndex::new(2)).is_none());
        assert!(log.get(LogIndex::new(100)).is_none());
    }

    #[test]
    fn get_on_empty_log() {
        let log: RaftLog<String> = RaftLog::new();
        assert!(log.get(LogIndex::new(0)).is_none());
        assert!(log.get(LogIndex::new(1)).is_none());
    }

    // ---- last_index / last_term ----

    #[test]
    fn last_index_and_term() {
        let mut log = RaftLog::new();
        assert_eq!(log.last_index(), LogIndex::new(0));
        assert_eq!(log.last_term(), Term::new(0));

        log.append(entry(3, 1));
        assert_eq!(log.last_index(), LogIndex::new(1));
        assert_eq!(log.last_term(), Term::new(3));

        log.append(entry(5, 2));
        assert_eq!(log.last_index(), LogIndex::new(2));
        assert_eq!(log.last_term(), Term::new(5));
    }

    // ---- term_at ----

    #[test]
    fn term_at_valid_index() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(2, 2));
        assert_eq!(log.term_at(LogIndex::new(1)), Some(Term::new(1)));
        assert_eq!(log.term_at(LogIndex::new(2)), Some(Term::new(2)));
    }

    #[test]
    fn term_at_invalid_index() {
        let log: RaftLog<String> = RaftLog::new();
        assert_eq!(log.term_at(LogIndex::new(0)), None);
        assert_eq!(log.term_at(LogIndex::new(1)), None);
    }

    // ---- slice ----

    #[test]
    fn slice_valid_range() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        let s = log.slice(LogIndex::new(2), LogIndex::new(4));
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].index, LogIndex::new(2));
        assert_eq!(s[1].index, LogIndex::new(3));
    }

    #[test]
    fn slice_entire_log() {
        let mut log = RaftLog::new();
        for i in 1..=3 {
            log.append(entry(1, i));
        }
        let s = log.slice(LogIndex::new(1), LogIndex::new(4));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn slice_empty_range() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        let s = log.slice(LogIndex::new(1), LogIndex::new(1));
        assert!(s.is_empty());
    }

    #[test]
    fn slice_out_of_bounds() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        assert!(log.slice(LogIndex::new(0), LogIndex::new(2)).is_empty());
        assert!(log.slice(LogIndex::new(3), LogIndex::new(5)).is_empty());
    }

    #[test]
    fn slice_clamps_to_log_end() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));
        let s = log.slice(LogIndex::new(1), LogIndex::new(100));
        assert_eq!(s.len(), 2);
    }

    // ---- entries_from ----

    #[test]
    fn entries_from_middle() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        let s = log.entries_from(LogIndex::new(3));
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].index, LogIndex::new(3));
    }

    #[test]
    fn entries_from_start() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));
        let s = log.entries_from(LogIndex::new(1));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn entries_from_beyond_end() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        assert!(log.entries_from(LogIndex::new(5)).is_empty());
    }

    #[test]
    fn entries_from_zero() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        assert!(log.entries_from(LogIndex::new(0)).is_empty());
    }

    // ---- truncate_from ----

    #[test]
    fn truncate_from_middle() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.truncate_from(LogIndex::new(3));
        assert_eq!(log.len(), 2);
        assert_eq!(log.last_index(), LogIndex::new(2));
    }

    #[test]
    fn truncate_from_start() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));
        log.truncate_from(LogIndex::new(1));
        assert!(log.is_empty());
    }

    #[test]
    fn truncate_from_beyond_end() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.truncate_from(LogIndex::new(10));
        // Entries before index 10 are kept.
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn truncate_from_zero_is_noop() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.truncate_from(LogIndex::new(0));
        assert_eq!(log.len(), 1);
    }

    // ---- append_entries (conflict detection) ----

    #[test]
    fn append_entries_empty_log_no_prev() {
        let mut log = RaftLog::new();
        let entries = vec![entry(1, 1), entry(1, 2)];
        let ok = log.append_entries(LogIndex::new(0), Term::new(0), &entries);
        assert!(ok);
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn append_entries_with_matching_prev() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        let entries = vec![entry(1, 2), entry(1, 3)];
        let ok = log.append_entries(LogIndex::new(1), Term::new(1), &entries);
        assert!(ok);
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn append_entries_prev_mismatch_returns_false() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        let entries = vec![entry(2, 2)];
        // prev_log_index=1 but prev_log_term=2 doesn't match actual term 1
        let ok = log.append_entries(LogIndex::new(1), Term::new(2), &entries);
        assert!(!ok);
        // Log should be unchanged.
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn append_entries_prev_index_missing_returns_false() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        let entries = vec![entry(2, 3)];
        // prev_log_index=2 but log only has 1 entry
        let ok = log.append_entries(LogIndex::new(2), Term::new(1), &entries);
        assert!(!ok);
    }

    #[test]
    fn append_entries_conflict_truncates_and_appends() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));
        log.append(entry(1, 3));

        // Leader sends entries for index 2 and 3 with term 2 (conflict at index 2).
        let new_entries = vec![entry(2, 2), entry(2, 3)];
        let ok = log.append_entries(LogIndex::new(1), Term::new(1), &new_entries);
        assert!(ok);
        assert_eq!(log.len(), 3);
        assert_eq!(log.get(LogIndex::new(2)).unwrap().term, Term::new(2));
        assert_eq!(log.get(LogIndex::new(3)).unwrap().term, Term::new(2));
    }

    #[test]
    fn append_entries_idempotent_no_conflict() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));

        // Re-sending same entries should be a no-op.
        let entries = vec![entry(1, 1), entry(1, 2)];
        let ok = log.append_entries(LogIndex::new(0), Term::new(0), &entries);
        assert!(ok);
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn append_entries_partial_overlap_extends() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(1, 2));

        // Entries 2 (same) and 3 (new).
        let entries = vec![entry(1, 2), entry(1, 3)];
        let ok = log.append_entries(LogIndex::new(1), Term::new(1), &entries);
        assert!(ok);
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn append_entries_empty_entries_is_heartbeat() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        let ok = log.append_entries(LogIndex::new(1), Term::new(1), &[]);
        assert!(ok);
        assert_eq!(log.len(), 1);
    }

    // ---- from_entries / all_entries ----

    #[test]
    fn from_entries_roundtrip() {
        let entries = vec![entry(1, 1), entry(2, 2), entry(2, 3)];
        let log = RaftLog::from_entries(entries.clone());
        assert_eq!(log.len(), 3);
        assert_eq!(log.all_entries(), &entries);
    }

    #[test]
    fn from_entries_empty() {
        let log: RaftLog<String> = RaftLog::from_entries(vec![]);
        assert!(log.is_empty());
    }

    // ---- compact_to ----

    #[test]
    fn compact_to_removes_entries() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(3));

        // Entries 1-3 removed, entries 4-5 remain.
        assert_eq!(log.len(), 2);
        assert_eq!(log.offset(), LogIndex::new(3));
        assert_eq!(log.last_index(), LogIndex::new(5));
        assert!(log.get(LogIndex::new(1)).is_none());
        assert!(log.get(LogIndex::new(2)).is_none());
        assert!(log.get(LogIndex::new(3)).is_none());
        assert_eq!(log.get(LogIndex::new(4)).unwrap().index, LogIndex::new(4));
        assert_eq!(log.get(LogIndex::new(5)).unwrap().index, LogIndex::new(5));
    }

    #[test]
    fn compact_to_sets_snapshot_term() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.append(entry(2, 2));
        log.append(entry(3, 3));
        log.compact_to(LogIndex::new(2));

        assert_eq!(log.snapshot_term(), Term::new(2));
        // term_at(offset) should return the snapshot term.
        assert_eq!(log.term_at(LogIndex::new(2)), Some(Term::new(2)));
    }

    #[test]
    fn compact_to_at_offset_is_noop() {
        let mut log = RaftLog::new();
        for i in 1..=3 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(2));
        let len_before = log.len();

        // Compacting to offset again should be a no-op.
        log.compact_to(LogIndex::new(2));
        assert_eq!(log.len(), len_before);
    }

    #[test]
    fn compact_to_below_offset_is_noop() {
        let mut log = RaftLog::new();
        for i in 1..=3 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(2));

        // Compacting to a point below offset is a no-op.
        log.compact_to(LogIndex::new(1));
        assert_eq!(log.offset(), LogIndex::new(2));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn compact_to_beyond_log_end_is_noop() {
        let mut log = RaftLog::new();
        log.append(entry(1, 1));
        log.compact_to(LogIndex::new(5));
        assert_eq!(log.len(), 1);
        assert_eq!(log.offset(), LogIndex::new(0));
    }

    #[test]
    fn compact_to_last_entry() {
        let mut log = RaftLog::new();
        for i in 1..=3 {
            log.append(entry(i, i));
        }
        log.compact_to(LogIndex::new(3));

        assert_eq!(log.len(), 0);
        assert_eq!(log.offset(), LogIndex::new(3));
        assert_eq!(log.last_index(), LogIndex::new(3));
        assert_eq!(log.last_term(), Term::new(3));
        assert_eq!(log.snapshot_term(), Term::new(3));
    }

    #[test]
    fn entries_from_after_compaction() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(3));

        let s = log.entries_from(LogIndex::new(4));
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].index, LogIndex::new(4));

        // Requesting at offset or below returns empty.
        assert!(log.entries_from(LogIndex::new(3)).is_empty());
        assert!(log.entries_from(LogIndex::new(1)).is_empty());
    }

    #[test]
    fn slice_after_compaction() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(2));

        let s = log.slice(LogIndex::new(3), LogIndex::new(6));
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].index, LogIndex::new(3));

        // Requesting compacted range returns empty.
        assert!(log.slice(LogIndex::new(1), LogIndex::new(3)).is_empty());
    }

    #[test]
    fn append_entries_after_compaction() {
        let mut log = RaftLog::new();
        for i in 1..=3 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(2));

        // prev_log_index=2 should match via snapshot_term.
        let entries = vec![entry(1, 4)];
        let ok = log.append_entries(LogIndex::new(3), Term::new(1), &entries);
        assert!(ok);
        assert_eq!(log.last_index(), LogIndex::new(4));
    }

    #[test]
    fn append_entries_with_prev_at_offset() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(3));

        // prev_log_index at offset (3) should use snapshot_term.
        let entries = vec![entry(2, 4), entry(2, 5)];
        let ok = log.append_entries(LogIndex::new(3), Term::new(1), &entries);
        assert!(ok);
        assert_eq!(log.get(LogIndex::new(4)).unwrap().term, Term::new(2));
    }

    #[test]
    fn truncate_from_after_compaction() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(2));

        log.truncate_from(LogIndex::new(4));
        assert_eq!(log.len(), 1);
        assert_eq!(log.last_index(), LogIndex::new(3));

        // Truncating at offset does nothing.
        log.truncate_from(LogIndex::new(2));
        assert_eq!(log.len(), 1);
    }

    // ---- reset_to_snapshot ----

    #[test]
    fn reset_to_snapshot_clears_log() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.reset_to_snapshot(LogIndex::new(10), Term::new(3));

        assert!(log.is_empty());
        assert_eq!(log.offset(), LogIndex::new(10));
        assert_eq!(log.snapshot_term(), Term::new(3));
        assert_eq!(log.last_index(), LogIndex::new(10));
        assert_eq!(log.last_term(), Term::new(3));
    }

    // ---- is_compacted ----

    #[test]
    fn is_compacted_checks_offset() {
        let mut log = RaftLog::new();
        for i in 1..=5 {
            log.append(entry(1, i));
        }
        log.compact_to(LogIndex::new(3));

        assert!(log.is_compacted(LogIndex::new(1)));
        assert!(log.is_compacted(LogIndex::new(2)));
        assert!(log.is_compacted(LogIndex::new(3)));
        assert!(!log.is_compacted(LogIndex::new(4)));
        assert!(!log.is_compacted(LogIndex::new(5)));
    }
}
