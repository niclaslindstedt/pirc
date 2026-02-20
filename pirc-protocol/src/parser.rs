use pirc_common::Nickname;

use crate::command::{Command, PircSubcommand};
use crate::error::ProtocolError;
use crate::message::{Message, MAX_PARAMS};
use crate::prefix::Prefix;

/// Maximum message length in bytes (IRC standard).
pub const MAX_MESSAGE_LEN: usize = 512;

/// Find the position of a byte in a string slice using byte-level scanning.
#[inline]
fn find_byte(s: &str, needle: u8) -> Option<usize> {
    memchr::memchr(needle, s.as_bytes())
}

/// Parse a raw protocol line into a [`Message`].
///
/// The input may or may not include the trailing `\r\n` delimiter — it is
/// stripped if present. The wire format is:
///
/// ```text
/// :<prefix> <command> <params...> :<trailing>\r\n
/// ```
///
/// # Errors
///
/// Returns a [`ProtocolError`] if the input is empty, too long, contains an
/// unknown command, has a malformed prefix, or exceeds the parameter limit.
pub fn parse(input: &str) -> Result<Message, ProtocolError> {
    let bytes = input.as_bytes();
    let len = bytes.len();

    // Check length upfront before any work
    if len > MAX_MESSAGE_LEN {
        return Err(ProtocolError::MessageTooLong {
            length: len,
            max: MAX_MESSAGE_LEN,
        });
    }

    // Strip trailing \r\n or \n using byte operations
    let line_end = if len >= 2 && bytes[len - 2] == b'\r' && bytes[len - 1] == b'\n' {
        len - 2
    } else if len >= 1 && bytes[len - 1] == b'\n' {
        len - 1
    } else {
        len
    };

    // Reject empty / whitespace-only (check bytes directly)
    let line = &input[..line_end];
    if line.bytes().all(|b| b == b' ' || b == b'\t') {
        return Err(ProtocolError::EmptyMessage);
    }

    let mut rest = line;

    // Parse optional prefix (starts with ':')
    let prefix = if rest.as_bytes().first() == Some(&b':') {
        // Skip the leading ':'
        rest = &rest[1..];
        let end = find_byte(rest, b' ').ok_or(ProtocolError::MissingCommand)?;
        let prefix_str = &rest[..end];
        rest = &rest[end + 1..];
        Some(parse_prefix(prefix_str)?)
    } else {
        None
    };

    // Skip leading spaces between prefix and command (byte-level)
    rest = skip_spaces(rest);

    if rest.is_empty() {
        return Err(ProtocolError::MissingCommand);
    }

    // Extract command token
    let (cmd_str, remainder) = match find_byte(rest, b' ') {
        Some(pos) => (&rest[..pos], &rest[pos + 1..]),
        None => (rest, ""),
    };

    // Handle PIRC extension commands specially: the subcommand keyword
    // is the first token after PIRC (e.g., "PIRC VERSION 1.0").
    if cmd_str.len() == 4 && cmd_str == "PIRC" {
        return parse_pirc_command(prefix, remainder);
    }

    let command = Command::from_keyword(cmd_str)
        .ok_or_else(|| ProtocolError::UnknownCommand(cmd_str.to_owned()))?;

    // Parse parameters
    let params = parse_params(remainder)?;

    Ok(match prefix {
        Some(p) => Message::with_prefix(p, command, params),
        None => Message::new(command, params),
    })
}

/// Skip leading ASCII space characters.
#[inline]
fn skip_spaces(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    &s[i..]
}

/// Parse a `PIRC` extension command.
///
/// The subcommand keyword (e.g., `VERSION`, `CAP`) is extracted from the first
/// token of `remainder`. For namespaced subcommands (`CLUSTER`, `P2P`), the
/// next token is also consumed as the inner keyword. Everything after the
/// subcommand keyword(s) is parsed as normal parameters of the resulting message.
fn parse_pirc_command(prefix: Option<Prefix>, remainder: &str) -> Result<Message, ProtocolError> {
    let remainder = skip_spaces(remainder);

    if remainder.is_empty() {
        return Err(ProtocolError::UnknownCommand(
            "PIRC (missing subcommand)".to_owned(),
        ));
    }

    let (sub_str, after_sub) = match find_byte(remainder, b' ') {
        Some(pos) => (&remainder[..pos], &remainder[pos + 1..]),
        None => (remainder, ""),
    };

    // Check for namespaced subcommands (CLUSTER, INVITE-KEY, NETWORK, P2P, GROUP)
    if is_pirc_namespace(sub_str) {
        let after_sub = skip_spaces(after_sub);
        if after_sub.is_empty() {
            return Err(ProtocolError::UnknownCommand(format!(
                "PIRC {sub_str} (missing subcommand)"
            )));
        }

        let (inner_str, params_str) = match find_byte(after_sub, b' ') {
            Some(pos) => (&after_sub[..pos], &after_sub[pos + 1..]),
            None => (after_sub, ""),
        };

        let subcommand = PircSubcommand::from_namespace(sub_str, inner_str)
            .ok_or_else(|| ProtocolError::UnknownCommand(format!("PIRC {sub_str} {inner_str}")))?;

        let command = Command::Pirc(subcommand);
        let params = parse_params(params_str)?;

        return Ok(match prefix {
            Some(p) => Message::with_prefix(p, command, params),
            None => Message::new(command, params),
        });
    }

    // Flat subcommand (VERSION, CAP, encryption keywords)
    let subcommand = PircSubcommand::from_keyword(sub_str)
        .ok_or_else(|| ProtocolError::UnknownCommand(format!("PIRC {sub_str}")))?;

    let command = Command::Pirc(subcommand);
    let params = parse_params(after_sub)?;

    Ok(match prefix {
        Some(p) => Message::with_prefix(p, command, params),
        None => Message::new(command, params),
    })
}

/// Check if a string is a PIRC namespace prefix.
#[inline]
fn is_pirc_namespace(s: &str) -> bool {
    matches!(s, "CLUSTER" | "P2P" | "INVITE-KEY" | "NETWORK" | "GROUP")
}

/// Parse the prefix string (without the leading `:`).
///
/// If it contains `!` and `@`, it's a user prefix (`nick!user@host`).
/// Otherwise it's a server prefix.
fn parse_prefix(s: &str) -> Result<Prefix, ProtocolError> {
    if s.is_empty() {
        return Err(ProtocolError::InvalidPrefix("empty prefix".to_owned()));
    }

    // Check for user prefix pattern: nick!user@host
    if let Some(bang_pos) = find_byte(s, b'!') {
        let nick_str = &s[..bang_pos];
        let after_bang = &s[bang_pos + 1..];

        let at_pos = find_byte(after_bang, b'@').ok_or_else(|| {
            ProtocolError::InvalidPrefix(format!("missing '@' in user prefix: {s}"))
        })?;

        let user = &after_bang[..at_pos];
        let host = &after_bang[at_pos + 1..];

        if user.is_empty() {
            return Err(ProtocolError::InvalidPrefix(format!(
                "empty user in prefix: {s}"
            )));
        }
        if host.is_empty() {
            return Err(ProtocolError::InvalidPrefix(format!(
                "empty host in prefix: {s}"
            )));
        }

        let nick =
            Nickname::new(nick_str).map_err(|e| ProtocolError::InvalidNickname(e.to_string()))?;

        Ok(Prefix::User {
            nick,
            user: user.to_owned(),
            host: host.to_owned(),
        })
    } else {
        // Server prefix — just a hostname/server name
        Ok(Prefix::Server(s.to_owned()))
    }
}

/// Parse the parameter portion of a message into a `Vec<String>`.
///
/// The trailing parameter (prefixed with `:`) consumes the rest of the line
/// and may contain spaces. Normal parameters are space-separated.
fn parse_params(input: &str) -> Result<Vec<String>, ProtocolError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    // Pre-allocate with a reasonable estimate. Most IRC messages have 1-4
    // params; this avoids reallocations for the common case.
    let mut params = Vec::with_capacity(4);
    let mut rest = input;

    while !rest.is_empty() {
        if rest.as_bytes()[0] == b':' {
            // Trailing parameter — everything after the ':' is one param
            params.push(rest[1..].to_owned());
            break;
        }

        let (param, remainder) = match find_byte(rest, b' ') {
            Some(pos) => (&rest[..pos], &rest[pos + 1..]),
            None => (rest, ""),
        };

        if !param.is_empty() {
            params.push(param.to_owned());
        }
        rest = remainder;

        if params.len() == MAX_PARAMS - 1 && !rest.is_empty() {
            // The 15th (last) param consumes the rest, even without ':'.
            // Strip leading ':' to stay consistent with the trailing handler.
            if rest.as_bytes()[0] == b':' {
                params.push(rest[1..].to_owned());
            } else {
                params.push(rest.to_owned());
            }
            break;
        }
    }

    if params.len() > MAX_PARAMS {
        return Err(ProtocolError::TooManyParams {
            count: params.len(),
            max: MAX_PARAMS,
        });
    }

    Ok(params)
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "parser_tests_pirc.rs"]
mod tests_pirc;
