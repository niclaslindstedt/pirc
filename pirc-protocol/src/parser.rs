use pirc_common::Nickname;

use crate::command::{Command, PircSubcommand};
use crate::error::ProtocolError;
use crate::message::{Message, MAX_PARAMS};
use crate::prefix::Prefix;

/// Maximum message length in bytes (IRC standard).
pub const MAX_MESSAGE_LEN: usize = 512;

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
    // Strip trailing \r\n or \n
    let line = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(input);

    // Reject empty / whitespace-only
    if line.trim().is_empty() {
        return Err(ProtocolError::EmptyMessage);
    }

    // Check length (against the original input including any CRLF)
    if input.len() > MAX_MESSAGE_LEN {
        return Err(ProtocolError::MessageTooLong {
            length: input.len(),
            max: MAX_MESSAGE_LEN,
        });
    }

    let mut rest = line;

    // Parse optional prefix (starts with ':')
    let prefix = if rest.starts_with(':') {
        // Skip the leading ':'
        rest = &rest[1..];
        let end = rest.find(' ').ok_or(ProtocolError::MissingCommand)?;
        let prefix_str = &rest[..end];
        rest = &rest[end + 1..];
        Some(parse_prefix(prefix_str)?)
    } else {
        None
    };

    // Skip any extra spaces between prefix and command
    rest = rest.trim_start();

    if rest.is_empty() {
        return Err(ProtocolError::MissingCommand);
    }

    // Extract command token
    let (cmd_str, remainder) = match rest.find(' ') {
        Some(pos) => (&rest[..pos], &rest[pos + 1..]),
        None => (rest, ""),
    };

    // Handle PIRC extension commands specially: the subcommand keyword
    // is the first token after PIRC (e.g., "PIRC VERSION 1.0").
    if cmd_str == "PIRC" {
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

/// Parse a `PIRC` extension command.
///
/// The subcommand keyword (e.g., `VERSION`, `CAP`) is extracted from the first
/// token of `remainder`. For namespaced subcommands (`CLUSTER`, `P2P`), the
/// next token is also consumed as the inner keyword. Everything after the
/// subcommand keyword(s) is parsed as normal parameters of the resulting message.
fn parse_pirc_command(prefix: Option<Prefix>, remainder: &str) -> Result<Message, ProtocolError> {
    let remainder = remainder.trim_start();

    if remainder.is_empty() {
        return Err(ProtocolError::UnknownCommand(
            "PIRC (missing subcommand)".to_owned(),
        ));
    }

    let (sub_str, after_sub) = match remainder.find(' ') {
        Some(pos) => (&remainder[..pos], &remainder[pos + 1..]),
        None => (remainder, ""),
    };

    // Check for namespaced subcommands (CLUSTER, INVITE-KEY, NETWORK, P2P) first
    if sub_str == "CLUSTER" || sub_str == "P2P" || sub_str == "INVITE-KEY" || sub_str == "NETWORK"
    {
        let after_sub = after_sub.trim_start();
        if after_sub.is_empty() {
            return Err(ProtocolError::UnknownCommand(format!(
                "PIRC {sub_str} (missing subcommand)"
            )));
        }

        let (inner_str, params_str) = match after_sub.find(' ') {
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

/// Parse the prefix string (without the leading `:`).
///
/// If it contains `!` and `@`, it's a user prefix (`nick!user@host`).
/// Otherwise it's a server prefix.
fn parse_prefix(s: &str) -> Result<Prefix, ProtocolError> {
    if s.is_empty() {
        return Err(ProtocolError::InvalidPrefix("empty prefix".to_owned()));
    }

    // Check for user prefix pattern: nick!user@host
    if let Some(bang_pos) = s.find('!') {
        let nick_str = &s[..bang_pos];
        let after_bang = &s[bang_pos + 1..];

        let at_pos = after_bang.find('@').ok_or_else(|| {
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

    let mut params = Vec::new();
    let mut rest = input;

    while !rest.is_empty() {
        if let Some(trailing) = rest.strip_prefix(':') {
            // Trailing parameter — everything after the ':' is one param
            params.push(trailing.to_owned());
            break;
        }

        let (param, remainder) = match rest.find(' ') {
            Some(pos) => (&rest[..pos], &rest[pos + 1..]),
            None => (rest, ""),
        };

        if !param.is_empty() {
            params.push(param.to_owned());
        }
        rest = remainder;

        if params.len() == MAX_PARAMS - 1 && !rest.is_empty() {
            // The 15th (last) param consumes the rest, even without ':'
            params.push(rest.to_owned());
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
