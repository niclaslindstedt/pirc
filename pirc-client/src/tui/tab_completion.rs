/// Tab completion engine for nicks, channel names, and commands.
///
/// Provides prefix-matched, case-insensitive completion with cycling through
/// multiple matches via repeated Tab presses. Shift+Tab cycles backward.
/// Completion state resets when any non-Tab input occurs.

/// All recognised IRC command names (without the leading `/`).
///
/// This list drives command completion when the user types `/` followed by
/// a partial command name.
const COMMAND_NAMES: &[&str] = &[
    "away",
    "ban",
    "cluster",
    "ctcp",
    "die",
    "help",
    "invite",
    "invite-key",
    "join",
    "kick",
    "kill",
    "list",
    "me",
    "mode",
    "msg",
    "names",
    "network",
    "nick",
    "notice",
    "oper",
    "part",
    "query",
    "quit",
    "restart",
    "topic",
    "whois",
];

/// Commands whose first argument is a channel name.
const CHANNEL_ARG_COMMANDS: &[&str] = &[
    "ban", "join", "kick", "mode", "names", "part", "topic",
];

/// Manages tab completion state across key presses.
///
/// The completer is context-aware:
/// - `/` at the start of input triggers command name completion.
/// - After a channel-argument command (`/join`, `/part`, etc.), completes
///   channel names from the provided candidate list.
/// - Everywhere else, completes nicknames from the provided candidate list.
///
/// Nick completion appends `: ` when the completion is at the very start
/// of the line (mIRC convention), or a single space when mid-line.
#[derive(Debug, Clone)]
pub struct TabCompleter {
    /// The original word fragment being completed (before any cycling).
    prefix: String,
    /// The text before the word being completed.
    before: String,
    /// The text after the cursor when completion started.
    after: String,
    /// Matching candidates for the current completion session.
    candidates: Vec<String>,
    /// Index into `candidates` for the current selection.
    index: usize,
    /// Whether there is an active completion session.
    active: bool,
    /// The kind of completion in progress.
    kind: CompletionKind,
}

/// What kind of token is being completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionKind {
    /// Command name (after `/`).
    Command,
    /// Channel name (starts with `#`).
    Channel,
    /// Nickname.
    Nick,
}

impl TabCompleter {
    /// Create a new completer with no active session.
    pub fn new() -> Self {
        Self {
            prefix: String::new(),
            before: String::new(),
            after: String::new(),
            candidates: Vec::new(),
            index: 0,
            active: false,
            kind: CompletionKind::Nick,
        }
    }

    /// Returns `true` if a completion session is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Reset the completion state. Call this when any non-Tab key is pressed.
    pub fn reset(&mut self) {
        self.active = false;
        self.prefix.clear();
        self.before.clear();
        self.after.clear();
        self.candidates.clear();
        self.index = 0;
        self.kind = CompletionKind::Nick;
    }

    /// Attempt tab completion on the current input line.
    ///
    /// `line` is the full current input text, `cursor` is the char-index
    /// cursor position. `nicks` and `channels` are the candidate lists
    /// for the current context.
    ///
    /// Returns `Some((new_line, new_cursor))` if a completion was applied,
    /// or `None` if no completions are available.
    pub fn complete(
        &mut self,
        line: &str,
        cursor: usize,
        nicks: &[&str],
        channels: &[&str],
    ) -> Option<(String, usize)> {
        if self.active {
            return self.cycle_forward();
        }
        self.start_completion(line, cursor, nicks, channels)
    }

    /// Cycle to the previous completion candidate (Shift+Tab).
    ///
    /// Returns `Some((new_line, new_cursor))` if cycling occurred,
    /// or `None` if no completion session is active.
    pub fn complete_backward(&mut self) -> Option<(String, usize)> {
        if !self.active || self.candidates.is_empty() {
            return None;
        }
        if self.index == 0 {
            self.index = self.candidates.len() - 1;
        } else {
            self.index -= 1;
        }
        Some(self.build_result())
    }

    /// Start a new completion session based on the current input context.
    fn start_completion(
        &mut self,
        line: &str,
        cursor: usize,
        nicks: &[&str],
        channels: &[&str],
    ) -> Option<(String, usize)> {
        // Split line into chars for accurate indexing.
        let chars: Vec<char> = line.chars().collect();
        let cursor = cursor.min(chars.len());

        // Find the word being completed: scan backward from cursor to find
        // the start of the current token.
        let mut word_start = cursor;
        while word_start > 0 && !chars[word_start - 1].is_whitespace() {
            word_start -= 1;
        }

        let word: String = chars[word_start..cursor].iter().collect();
        let before: String = chars[..word_start].iter().collect();
        let after: String = chars[cursor..].iter().collect();

        // Determine completion kind based on context.
        let kind = determine_completion_kind(&before, &word);

        // Build candidate list.
        let candidates = match kind {
            CompletionKind::Command => {
                // The word includes the `/` prefix; match against command
                // names without it.
                let prefix = word.strip_prefix('/').unwrap_or(&word);
                if prefix.is_empty() {
                    return None;
                }
                let prefix_lower = prefix.to_ascii_lowercase();
                COMMAND_NAMES
                    .iter()
                    .filter(|cmd| cmd.starts_with(&prefix_lower))
                    .map(|cmd| format!("/{cmd}"))
                    .collect::<Vec<_>>()
            }
            CompletionKind::Channel => {
                let prefix_lower = word.to_ascii_lowercase();
                // Match channels both by full name (#general) and by the
                // name portion without the leading `#` (general). This
                // allows `/join gen<Tab>` to complete to `#general`.
                let has_hash = prefix_lower.starts_with('#');
                channels
                    .iter()
                    .filter(|ch| {
                        let ch_lower = ch.to_ascii_lowercase();
                        if has_hash {
                            ch_lower.starts_with(&prefix_lower)
                        } else {
                            ch_lower
                                .strip_prefix('#')
                                .is_some_and(|name| name.starts_with(&prefix_lower))
                        }
                    })
                    .map(|ch| (*ch).to_owned())
                    .collect::<Vec<_>>()
            }
            CompletionKind::Nick => {
                let prefix_lower = word.to_ascii_lowercase();
                nicks
                    .iter()
                    .filter(|n| n.to_ascii_lowercase().starts_with(&prefix_lower))
                    .map(|n| (*n).to_owned())
                    .collect::<Vec<_>>()
            }
        };

        if candidates.is_empty() || word.is_empty() {
            return None;
        }

        self.prefix = word;
        self.before = before;
        self.after = after;
        self.candidates = candidates;
        self.index = 0;
        self.active = true;
        self.kind = kind;

        Some(self.build_result())
    }

    /// Cycle forward through the candidate list.
    fn cycle_forward(&mut self) -> Option<(String, usize)> {
        if self.candidates.is_empty() {
            return None;
        }
        self.index = (self.index + 1) % self.candidates.len();
        Some(self.build_result())
    }

    /// Build the completed line and cursor position from current state.
    fn build_result(&self) -> (String, usize) {
        let candidate = &self.candidates[self.index];
        let suffix = self.completion_suffix(candidate);

        let completed = format!("{}{candidate}{suffix}{}", self.before, self.after);
        let new_cursor = self.before.chars().count()
            + candidate.chars().count()
            + suffix.chars().count();

        (completed, new_cursor)
    }

    /// Determine the suffix to append after a completion.
    ///
    /// - Commands: append a space.
    /// - Channels: append a space.
    /// - Nicks at start of line: append `: ` (mIRC convention).
    /// - Nicks mid-line: append a space.
    fn completion_suffix(&self, _candidate: &str) -> &'static str {
        match self.kind {
            CompletionKind::Command | CompletionKind::Channel => " ",
            CompletionKind::Nick => {
                if self.before.is_empty() {
                    ": "
                } else {
                    " "
                }
            }
        }
    }
}

impl Default for TabCompleter {
    fn default() -> Self {
        Self::new()
    }
}

/// Determine what kind of completion to perform based on context.
///
/// - If the word starts with `/` and there's nothing before it, complete commands.
/// - If the text before the cursor is a command that takes a channel argument
///   and this is the first argument position, complete channels.
/// - If the word starts with `#`, complete channels.
/// - Otherwise, complete nicknames.
fn determine_completion_kind(before: &str, word: &str) -> CompletionKind {
    // Word starts with `/` at the beginning of the line → command completion.
    if word.starts_with('/') && before.is_empty() {
        return CompletionKind::Command;
    }

    // Word starts with `#` → channel completion.
    if word.starts_with('#') {
        return CompletionKind::Channel;
    }

    // Check if we're in the first argument position of a channel-arg command.
    let trimmed = before.trim_start();
    if let Some(rest) = trimmed.strip_prefix('/') {
        // Extract command name.
        let cmd_name = rest
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        // Count how many arguments have been completed before the current word.
        // `before` includes the command and any completed args. After stripping
        // the command name, count whitespace-separated tokens.
        let after_cmd = rest
            .get(cmd_name.len()..)
            .unwrap_or("")
            .trim_start();

        // If there are no completed args yet (after_cmd is empty or only
        // whitespace that led to the current word position) we're at arg #1.
        let completed_args: Vec<&str> = if after_cmd.is_empty() {
            Vec::new()
        } else {
            after_cmd.split_whitespace().collect()
        };

        if completed_args.is_empty()
            && CHANNEL_ARG_COMMANDS.contains(&cmd_name.as_str())
        {
            return CompletionKind::Channel;
        }
    }

    CompletionKind::Nick
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper ───────────────────────────────────────────────────────

    fn tc() -> TabCompleter {
        TabCompleter::new()
    }

    fn nicks() -> Vec<&'static str> {
        vec!["Alice", "alice2", "Bob", "bobby", "Charlie"]
    }

    fn channels() -> Vec<&'static str> {
        vec!["#general", "#random", "#rust", "#gaming"]
    }

    // ── Command completion ───────────────────────────────────────────

    #[test]
    fn complete_command_partial() {
        let mut c = tc();
        let result = c.complete("/jo", 3, &[], &[]);
        assert_eq!(result, Some(("/join ".to_owned(), 6)));
    }

    #[test]
    fn complete_command_full_match() {
        let mut c = tc();
        let result = c.complete("/quit", 5, &[], &[]);
        assert_eq!(result, Some(("/quit ".to_owned(), 6)));
    }

    #[test]
    fn complete_command_multiple_matches_cycle() {
        let mut c = tc();
        // /ki → kill, kick
        let r1 = c.complete("/ki", 3, &[], &[]);
        assert_eq!(r1, Some(("/kick ".to_owned(), 6)));
        let r2 = c.complete("/ki", 3, &[], &[]);
        assert_eq!(r2, Some(("/kill ".to_owned(), 6)));
        // Wrap around
        let r3 = c.complete("/ki", 3, &[], &[]);
        assert_eq!(r3, Some(("/kick ".to_owned(), 6)));
    }

    #[test]
    fn complete_command_case_insensitive() {
        let mut c = tc();
        let result = c.complete("/JO", 3, &[], &[]);
        assert_eq!(result, Some(("/join ".to_owned(), 6)));
    }

    #[test]
    fn complete_command_no_match() {
        let mut c = tc();
        let result = c.complete("/xyz", 4, &[], &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn complete_command_empty_prefix() {
        let mut c = tc();
        // Just "/" with nothing after → empty word, no completion
        let result = c.complete("/", 1, &[], &[]);
        assert_eq!(result, None);
    }

    // ── Nick completion ──────────────────────────────────────────────

    #[test]
    fn complete_nick_at_start_of_line() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("al", 2, &n, &[]);
        // At start of line → ": " suffix
        assert_eq!(result, Some(("Alice: ".to_owned(), 7)));
    }

    #[test]
    fn complete_nick_cycle_at_start() {
        let mut c = tc();
        let n = nicks();
        let r1 = c.complete("al", 2, &n, &[]);
        assert_eq!(r1, Some(("Alice: ".to_owned(), 7)));
        let r2 = c.complete("al", 2, &n, &[]);
        assert_eq!(r2, Some(("alice2: ".to_owned(), 8)));
        // Wrap around
        let r3 = c.complete("al", 2, &n, &[]);
        assert_eq!(r3, Some(("Alice: ".to_owned(), 7)));
    }

    #[test]
    fn complete_nick_mid_line() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("hello al", 8, &n, &[]);
        // Mid-line → space suffix
        assert_eq!(result, Some(("hello Alice ".to_owned(), 12)));
    }

    #[test]
    fn complete_nick_case_insensitive() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("BOB", 3, &n, &[]);
        assert_eq!(result, Some(("Bob: ".to_owned(), 5)));
    }

    #[test]
    fn complete_nick_no_match() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("zzz", 3, &n, &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn complete_nick_empty_candidates() {
        let mut c = tc();
        let result = c.complete("al", 2, &[], &[]);
        assert_eq!(result, None);
    }

    // ── Channel completion ──────────────────────────────────────────

    #[test]
    fn complete_channel_with_hash() {
        let mut c = tc();
        let ch = channels();
        let result = c.complete("#gen", 4, &[], &ch);
        assert_eq!(result, Some(("#general ".to_owned(), 9)));
    }

    #[test]
    fn complete_channel_multiple() {
        let mut c = tc();
        let ch = channels();
        let r1 = c.complete("#r", 2, &[], &ch);
        assert_eq!(r1, Some(("#random ".to_owned(), 8)));
        let r2 = c.complete("#r", 2, &[], &ch);
        assert_eq!(r2, Some(("#rust ".to_owned(), 6)));
        let r3 = c.complete("#r", 2, &[], &ch);
        assert_eq!(r3, Some(("#random ".to_owned(), 8)));
    }

    #[test]
    fn complete_channel_case_insensitive() {
        let mut c = tc();
        let ch = channels();
        let result = c.complete("#GEN", 4, &[], &ch);
        assert_eq!(result, Some(("#general ".to_owned(), 9)));
    }

    #[test]
    fn complete_channel_after_join_command() {
        let mut c = tc();
        let ch = channels();
        let result = c.complete("/join gen", 9, &[], &ch);
        // First arg of /join → channel completion even without #
        assert_eq!(result, Some(("/join #general ".to_owned(), 15)));
    }

    #[test]
    fn complete_channel_after_part_command() {
        let mut c = tc();
        let ch = channels();
        let result = c.complete("/part #r", 8, &[], &ch);
        assert_eq!(result, Some(("/part #random ".to_owned(), 14)));
    }

    #[test]
    fn complete_channel_no_match() {
        let mut c = tc();
        let ch = channels();
        let result = c.complete("#xyz", 4, &[], &ch);
        assert_eq!(result, None);
    }

    // ── Context-aware completion ────────────────────────────────────

    #[test]
    fn nick_completion_after_non_channel_command() {
        let mut c = tc();
        let n = nicks();
        // /msg takes a target (nick), not a channel as first arg
        let result = c.complete("/msg al", 7, &n, &[]);
        assert_eq!(result, Some(("/msg Alice ".to_owned(), 11)));
    }

    #[test]
    fn nick_completion_in_message_body() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("/msg #general bo", 16, &n, &[]);
        assert_eq!(result, Some(("/msg #general Bob ".to_owned(), 18)));
    }

    // ── Backward cycling ────────────────────────────────────────────

    #[test]
    fn backward_cycle() {
        let mut c = tc();
        let n = nicks();
        // Start with "bo" → Bob, bobby
        let r1 = c.complete("bo", 2, &n, &[]);
        assert_eq!(r1, Some(("Bob: ".to_owned(), 5)));
        // Backward → wraps to last candidate (bobby)
        let r2 = c.complete_backward();
        assert_eq!(r2, Some(("bobby: ".to_owned(), 7)));
        // Backward again → Bob
        let r3 = c.complete_backward();
        assert_eq!(r3, Some(("Bob: ".to_owned(), 5)));
    }

    #[test]
    fn backward_when_not_active() {
        let mut c = tc();
        let result = c.complete_backward();
        assert_eq!(result, None);
    }

    // ── Reset ───────────────────────────────────────────────────────

    #[test]
    fn reset_clears_state() {
        let mut c = tc();
        let n = nicks();
        c.complete("al", 2, &n, &[]);
        assert!(c.is_active());
        c.reset();
        assert!(!c.is_active());
    }

    #[test]
    fn reset_allows_new_completion() {
        let mut c = tc();
        let n = nicks();
        let r1 = c.complete("al", 2, &n, &[]);
        assert_eq!(r1, Some(("Alice: ".to_owned(), 7)));

        // Simulate non-Tab input by resetting.
        c.reset();

        // Now complete with a different prefix.
        let r2 = c.complete("bo", 2, &n, &[]);
        assert_eq!(r2, Some(("Bob: ".to_owned(), 5)));
    }

    // ── Text after cursor preserved ─────────────────────────────────

    #[test]
    fn text_after_cursor_preserved() {
        let mut c = tc();
        let n = nicks();
        // Cursor in the middle: "al| more text"
        let result = c.complete("al more text", 2, &n, &[]);
        assert_eq!(result, Some(("Alice:  more text".to_owned(), 7)));
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn empty_line_no_completion() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("", 0, &n, &channels());
        assert_eq!(result, None);
    }

    #[test]
    fn whitespace_only_no_completion() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("   ", 3, &n, &channels());
        assert_eq!(result, None);
    }

    #[test]
    fn cursor_at_start_no_completion() {
        let mut c = tc();
        let n = nicks();
        let result = c.complete("alice", 0, &n, &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn single_candidate_no_cycle() {
        let mut c = tc();
        let n = nicks();
        // Only one match: "Char" → Charlie
        let r1 = c.complete("Char", 4, &n, &[]);
        assert_eq!(r1, Some(("Charlie: ".to_owned(), 9)));
        // Cycling wraps to the same candidate
        let r2 = c.complete("Char", 4, &n, &[]);
        assert_eq!(r2, Some(("Charlie: ".to_owned(), 9)));
    }

    // ── determine_completion_kind unit tests ─────────────────────────

    #[test]
    fn kind_command_at_start() {
        assert_eq!(
            determine_completion_kind("", "/jo"),
            CompletionKind::Command
        );
    }

    #[test]
    fn kind_channel_with_hash() {
        assert_eq!(
            determine_completion_kind("", "#gen"),
            CompletionKind::Channel
        );
    }

    #[test]
    fn kind_channel_after_join() {
        assert_eq!(
            determine_completion_kind("/join ", "gen"),
            CompletionKind::Channel
        );
    }

    #[test]
    fn kind_nick_default() {
        assert_eq!(
            determine_completion_kind("", "al"),
            CompletionKind::Nick
        );
    }

    #[test]
    fn kind_nick_after_msg() {
        assert_eq!(
            determine_completion_kind("/msg ", "al"),
            CompletionKind::Nick
        );
    }

    #[test]
    fn kind_nick_second_arg_of_channel_command() {
        // /kick #chan <nick> — second arg should be nick, not channel
        assert_eq!(
            determine_completion_kind("/kick #chan ", "bo"),
            CompletionKind::Nick
        );
    }
}
