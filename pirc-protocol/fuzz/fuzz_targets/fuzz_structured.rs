#![no_main]

use libfuzzer_sys::fuzz_target;

// Fuzz with structured, near-valid IRC messages built from random bytes.
// Exercises prefix parsing, PIRC extensions, parameter edge cases,
// and command parsing more effectively than pure random input.
fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // Use the first byte to select a message construction strategy
    let strategy = data[0] % 6;
    let rest = &data[1..];

    let input = match strategy {
        0 => build_with_prefix(rest),
        1 => build_pirc_command(rest),
        2 => build_many_params(rest),
        3 => build_near_valid_command(rest),
        4 => build_numeric_reply(rest),
        _ => build_raw_with_crlf(rest),
    };

    if let Some(input) = input {
        let _ = pirc_protocol::parse(&input);
    }
});

/// Build a message with a prefix (server or user)
fn build_with_prefix(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }
    let use_user_prefix = data[0] & 1 == 0;
    let rest = &data[1..];

    let input = if use_user_prefix {
        // nick!user@host format
        let parts: Vec<&str> = safe_str(rest)?.splitn(4, |c: char| c.is_ascii_whitespace()).collect();
        if parts.len() >= 2 {
            format!(
                ":{nick}!{user}@{host} {cmd}",
                nick = parts.first().unwrap_or(&"n"),
                user = parts.get(1).unwrap_or(&"u"),
                host = parts.get(2).unwrap_or(&"h"),
                cmd = parts.get(3).unwrap_or(&"PING")
            )
        } else {
            format!(":{} PING", safe_str(rest)?)
        }
    } else {
        format!(":server.{} PING", safe_str(rest)?)
    };

    // Clamp to max IRC message length
    Some(clamp_len(&input))
}

/// Build a PIRC extension command
fn build_pirc_command(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    let subcmd_idx = data[0] % 12;
    let rest = safe_str(&data[1..])?;

    let subcmd = match subcmd_idx {
        0 => "VERSION",
        1 => "CAP",
        2 => "KEYEXCHANGE",
        3 => "ENCRYPTED",
        4 => "CLUSTER JOIN",
        5 => "CLUSTER SYNC",
        6 => "P2P OFFER",
        7 => "P2P ICE",
        8 => "GROUP CREATE",
        9 => "GROUP MSG",
        10 => "INVITE-KEY GENERATE",
        _ => "NETWORK INFO",
    };

    let input = if rest.is_empty() {
        format!("PIRC {subcmd}")
    } else {
        format!("PIRC {subcmd} {rest}")
    };

    Some(clamp_len(&input))
}

/// Build a message with many parameters to test the 15-param limit
fn build_many_params(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None;
    }

    let param_count = (data[0] % 20) as usize; // 0..19 params
    let rest = safe_str(&data[1..])?;

    let mut msg = String::from("PRIVMSG");
    for i in 0..param_count {
        if i == param_count - 1 {
            // Last param might be trailing
            msg.push_str(" :");
            msg.push_str(&rest.get(..20.min(rest.len())).unwrap_or("trail"));
        } else {
            msg.push(' ');
            msg.push_str(&format!("p{i}"));
        }
    }

    Some(clamp_len(&msg))
}

/// Build near-valid commands with slight mutations
fn build_near_valid_command(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    let cmd_idx = data[0] % 16;
    let rest = safe_str(&data[1..])?;

    let cmd = match cmd_idx {
        0 => "NICK",
        1 => "JOIN",
        2 => "PART",
        3 => "PRIVMSG",
        4 => "NOTICE",
        5 => "QUIT",
        6 => "MODE",
        7 => "TOPIC",
        8 => "KICK",
        9 => "WHOIS",
        10 => "PING",
        11 => "PONG",
        12 => "USER",
        13 => "AWAY",
        14 => "LIST",
        _ => "NAMES",
    };

    let input = if rest.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {rest}")
    };

    Some(clamp_len(&input))
}

/// Build a numeric reply
fn build_numeric_reply(data: &[u8]) -> Option<String> {
    if data.len() < 3 {
        return None;
    }

    // Build a 3-digit numeric code
    let code = format!(
        "{}{}{}",
        data[0] % 10,
        data[1] % 10,
        data[2] % 10
    );
    let rest = safe_str(&data[3..])?;

    let input = if rest.is_empty() {
        code
    } else {
        format!("{code} {rest}")
    };

    Some(clamp_len(&input))
}

/// Build raw input with CRLF variations appended
fn build_raw_with_crlf(data: &[u8]) -> Option<String> {
    let s = safe_str(data)?;
    if s.is_empty() {
        return None;
    }

    // Append various line endings
    let ending_idx = data.last().copied().unwrap_or(0) % 4;
    let ending = match ending_idx {
        0 => "\r\n",
        1 => "\n",
        2 => "",
        _ => "\r",
    };

    Some(clamp_len(&format!("{s}{ending}")))
}

/// Safely convert bytes to a UTF-8 str, returning None for invalid UTF-8
fn safe_str(data: &[u8]) -> Option<&str> {
    std::str::from_utf8(data).ok()
}

/// Clamp string to max IRC message length (512 bytes).
/// Uses char-boundary-aware slicing to avoid panicking on multi-byte UTF-8.
fn clamp_len(s: &str) -> String {
    if s.len() <= 512 {
        s.to_string()
    } else {
        // Walk backwards from byte 512 to find a valid char boundary.
        let mut end = 512;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}
