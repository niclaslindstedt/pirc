/// Parsed representation of user input from the input line.
///
/// Input starting with `/` is parsed as a command (with name and arguments).
/// Input starting with `//` is treated as a chat message with a literal `/` prefix.
/// Everything else is a plain chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedInput {
    /// A plain chat message (no command prefix).
    ChatMessage(String),
    /// A `/`-prefixed command with its name (lowercase) and arguments.
    Command { name: String, args: Vec<String> },
}

/// Parse raw input text into a [`ParsedInput`].
///
/// Rules:
/// - Empty or whitespace-only input becomes `ChatMessage("")`.
/// - `//` prefix: the leading `/` is stripped and the rest is a chat message.
/// - `/` prefix: the first word (after `/`) is the command name (lowercased),
///   remaining words are space-delimited arguments. A trailing text segment
///   (everything after the last recognized positional arg) is preserved as a
///   single string — this matches IRC conventions where e.g.
///   `/msg nick hello world` yields args `["nick", "hello world"]`.
/// - Bare `/` with nothing after it becomes `Command { name: "", args: [] }`.
/// - Anything else is a `ChatMessage`.
///
/// This function performs **no validation** of command names — it only splits
/// the input into structured form.
pub fn parse(input: &str) -> ParsedInput {
    // Empty / whitespace-only → chat message.
    if input.is_empty() || input.chars().all(char::is_whitespace) {
        return ParsedInput::ChatMessage(String::new());
    }

    // `//` escape: strip one leading `/`, rest is chat.
    if input.starts_with("//") {
        return ParsedInput::ChatMessage(input[1..].to_owned());
    }

    // `/` prefix: this is a command.
    if let Some(rest) = input.strip_prefix('/') {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            return ParsedInput::Command {
                name: String::new(),
                args: Vec::new(),
            };
        }

        // Split into command name and the remainder.
        let (name, remainder) = match trimmed.find(char::is_whitespace) {
            Some(pos) => (&trimmed[..pos], trimmed[pos..].trim_start()),
            None => (trimmed, ""),
        };

        let name = name.to_ascii_lowercase();

        let args = if remainder.is_empty() {
            Vec::new()
        } else {
            split_args(remainder)
        };

        return ParsedInput::Command { name, args };
    }

    // Everything else is a chat message.
    ParsedInput::ChatMessage(input.to_owned())
}

/// Split argument text into space-delimited tokens.
///
/// The last token is always "trailing" — it preserves any internal whitespace.
/// For a single token, this is the token itself. For multiple space-separated
/// words, the first N-1 tokens are single words and the last token is the
/// entire remaining text (IRC trailing style).
///
/// Example: `"nick hello world"` → `["nick", "hello world"]`
fn split_args(text: &str) -> Vec<String> {
    // Find first whitespace boundary after first token.
    let first_space = text.find(char::is_whitespace);

    match first_space {
        None => {
            // Single token — no trailing text.
            vec![text.to_owned()]
        }
        Some(pos) => {
            let first = &text[..pos];
            let rest = text[pos..].trim_start();
            if rest.is_empty() {
                vec![first.to_owned()]
            } else {
                vec![first.to_owned(), rest.to_owned()]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Chat messages ────────────────────────────────────────────

    #[test]
    fn plain_text_is_chat_message() {
        assert_eq!(
            parse("hello world"),
            ParsedInput::ChatMessage("hello world".to_owned())
        );
    }

    #[test]
    fn empty_input_is_chat_message() {
        assert_eq!(parse(""), ParsedInput::ChatMessage(String::new()));
    }

    #[test]
    fn whitespace_only_is_chat_message() {
        assert_eq!(parse("   "), ParsedInput::ChatMessage(String::new()));
        assert_eq!(parse("\t\n"), ParsedInput::ChatMessage(String::new()));
    }

    #[test]
    fn text_not_starting_with_slash_is_chat() {
        assert_eq!(
            parse("just some text"),
            ParsedInput::ChatMessage("just some text".to_owned())
        );
    }

    // ── // escape ────────────────────────────────────────────────

    #[test]
    fn double_slash_is_chat_with_leading_slash() {
        assert_eq!(
            parse("//me waves"),
            ParsedInput::ChatMessage("/me waves".to_owned())
        );
    }

    #[test]
    fn double_slash_alone() {
        assert_eq!(parse("//"), ParsedInput::ChatMessage("/".to_owned()));
    }

    #[test]
    fn triple_slash_strips_one() {
        assert_eq!(
            parse("///test"),
            ParsedInput::ChatMessage("//test".to_owned())
        );
    }

    // ── Command detection ────────────────────────────────────────

    #[test]
    fn bare_slash_is_empty_command() {
        assert_eq!(
            parse("/"),
            ParsedInput::Command {
                name: String::new(),
                args: Vec::new(),
            }
        );
    }

    #[test]
    fn slash_with_trailing_space_is_empty_command() {
        assert_eq!(
            parse("/  "),
            ParsedInput::Command {
                name: String::new(),
                args: Vec::new(),
            }
        );
    }

    #[test]
    fn simple_command_no_args() {
        assert_eq!(
            parse("/quit"),
            ParsedInput::Command {
                name: "quit".to_owned(),
                args: Vec::new(),
            }
        );
    }

    #[test]
    fn command_with_one_arg() {
        assert_eq!(
            parse("/join #channel"),
            ParsedInput::Command {
                name: "join".to_owned(),
                args: vec!["#channel".to_owned()],
            }
        );
    }

    #[test]
    fn command_with_trailing_text() {
        assert_eq!(
            parse("/msg nick hello world"),
            ParsedInput::Command {
                name: "msg".to_owned(),
                args: vec!["nick".to_owned(), "hello world".to_owned()],
            }
        );
    }

    // ── Case insensitivity ───────────────────────────────────────

    #[test]
    fn command_name_lowercased() {
        assert_eq!(
            parse("/QUIT"),
            ParsedInput::Command {
                name: "quit".to_owned(),
                args: Vec::new(),
            }
        );
    }

    #[test]
    fn mixed_case_command() {
        assert_eq!(
            parse("/JoIn #test"),
            ParsedInput::Command {
                name: "join".to_owned(),
                args: vec!["#test".to_owned()],
            }
        );
    }

    // ── Argument splitting ───────────────────────────────────────

    #[test]
    fn trailing_text_preserves_spaces() {
        assert_eq!(
            parse("/msg user  hello   world"),
            ParsedInput::Command {
                name: "msg".to_owned(),
                args: vec!["user".to_owned(), "hello   world".to_owned()],
            }
        );
    }

    #[test]
    fn command_with_extra_leading_spaces() {
        assert_eq!(
            parse("/  join  #channel"),
            ParsedInput::Command {
                name: "join".to_owned(),
                args: vec!["#channel".to_owned()],
            }
        );
    }

    #[test]
    fn command_with_trailing_whitespace_on_arg() {
        // Trailing whitespace after a single arg is trimmed.
        assert_eq!(
            parse("/join #channel  "),
            ParsedInput::Command {
                name: "join".to_owned(),
                args: vec!["#channel".to_owned()],
            }
        );
    }

    #[test]
    fn me_action() {
        // Generic split: first word is arg[0], rest is arg[1].
        // Command-specific handling (e.g. treating all text as one arg) is
        // done by the dispatch layer, not the parser.
        assert_eq!(
            parse("/me waves at everyone"),
            ParsedInput::Command {
                name: "me".to_owned(),
                args: vec!["waves".to_owned(), "at everyone".to_owned()],
            }
        );
    }

    #[test]
    fn topic_with_long_text() {
        assert_eq!(
            parse("/topic #channel Welcome to the channel! Please read the rules."),
            ParsedInput::Command {
                name: "topic".to_owned(),
                args: vec![
                    "#channel".to_owned(),
                    "Welcome to the channel! Please read the rules.".to_owned(),
                ],
            }
        );
    }

    #[test]
    fn kick_with_reason() {
        assert_eq!(
            parse("/kick #chan baduser You have been kicked"),
            ParsedInput::Command {
                name: "kick".to_owned(),
                args: vec![
                    "#chan".to_owned(),
                    "baduser You have been kicked".to_owned(),
                ],
            }
        );
    }

    #[test]
    fn nick_command_single_arg() {
        assert_eq!(
            parse("/nick newnick"),
            ParsedInput::Command {
                name: "nick".to_owned(),
                args: vec!["newnick".to_owned()],
            }
        );
    }

    // ── Edge cases ───────────────────────────────────────────────

    #[test]
    fn very_long_input() {
        let long_text = "a".repeat(10_000);
        let input = format!("/msg target {long_text}");
        let parsed = parse(&input);
        match parsed {
            ParsedInput::Command { name, args } => {
                assert_eq!(name, "msg");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], "target");
                assert_eq!(args[1].len(), 10_000);
            }
            _ => panic!("expected Command"),
        }
    }

    #[test]
    fn unicode_in_chat_message() {
        assert_eq!(
            parse("日本語テスト 🌍"),
            ParsedInput::ChatMessage("日本語テスト 🌍".to_owned())
        );
    }

    #[test]
    fn unicode_in_command_args() {
        assert_eq!(
            parse("/msg user 日本語テスト"),
            ParsedInput::Command {
                name: "msg".to_owned(),
                args: vec!["user".to_owned(), "日本語テスト".to_owned()],
            }
        );
    }

    #[test]
    fn command_only_spaces_after_name() {
        assert_eq!(
            parse("/quit   "),
            ParsedInput::Command {
                name: "quit".to_owned(),
                args: Vec::new(),
            }
        );
    }

    #[test]
    fn part_with_message() {
        assert_eq!(
            parse("/part #channel Goodbye everyone!"),
            ParsedInput::Command {
                name: "part".to_owned(),
                args: vec!["#channel".to_owned(), "Goodbye everyone!".to_owned(),],
            }
        );
    }

    #[test]
    fn notice_command() {
        assert_eq!(
            parse("/notice user This is a notice"),
            ParsedInput::Command {
                name: "notice".to_owned(),
                args: vec!["user".to_owned(), "This is a notice".to_owned()],
            }
        );
    }

    #[test]
    fn mode_command() {
        assert_eq!(
            parse("/mode #channel +o user"),
            ParsedInput::Command {
                name: "mode".to_owned(),
                args: vec!["#channel".to_owned(), "+o user".to_owned()],
            }
        );
    }

    #[test]
    fn whois_command() {
        assert_eq!(
            parse("/whois someuser"),
            ParsedInput::Command {
                name: "whois".to_owned(),
                args: vec!["someuser".to_owned()],
            }
        );
    }

    // ── split_args unit tests ────────────────────────────────────

    #[test]
    fn split_args_single_word() {
        assert_eq!(split_args("word"), vec!["word".to_owned()]);
    }

    #[test]
    fn split_args_two_words() {
        assert_eq!(
            split_args("first second"),
            vec!["first".to_owned(), "second".to_owned()]
        );
    }

    #[test]
    fn split_args_trailing_text() {
        assert_eq!(
            split_args("target hello world"),
            vec!["target".to_owned(), "hello world".to_owned()]
        );
    }

    #[test]
    fn split_args_trailing_whitespace() {
        assert_eq!(split_args("word   "), vec!["word".to_owned()]);
    }
}
