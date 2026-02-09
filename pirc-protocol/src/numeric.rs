// Well-known IRC numeric reply codes used by pirc.
//
// These constants map standard numeric reply names to their 3-digit codes.
// They are used with `Command::Numeric` to construct reply messages.

// ---- Success replies ----

/// Welcome to the IRC network.
pub const RPL_WELCOME: u16 = 1;
/// Your host is ..., running version ...
pub const RPL_YOURHOST: u16 = 2;
/// This server was created ...
pub const RPL_CREATED: u16 = 3;
/// List of nicks in a channel (part of NAMES reply).
pub const RPL_NAMREPLY: u16 = 353;
/// End of /NAMES list.
pub const RPL_ENDOFNAMES: u16 = 366;
/// Message of the day text line.
pub const RPL_MOTD: u16 = 372;
/// Start of message of the day.
pub const RPL_MOTDSTART: u16 = 375;
/// End of message of the day.
pub const RPL_ENDOFMOTD: u16 = 376;

// ---- Error replies ----

/// No nickname given.
pub const ERR_NONICKNAMEGIVEN: u16 = 431;
/// Erroneous nickname.
pub const ERR_ERRONEUSNICKNAME: u16 = 432;
/// Nickname is already in use.
pub const ERR_NICKNAMEINUSE: u16 = 433;
/// Not enough parameters.
pub const ERR_NEEDMOREPARAMS: u16 = 461;
/// You may not reregister.
pub const ERR_ALREADYREGISTERED: u16 = 462;

/// Returns the standard name for a known numeric reply code, if any.
///
/// # Examples
///
/// ```
/// use pirc_protocol::numeric;
///
/// assert_eq!(numeric::reply_name(1), Some("RPL_WELCOME"));
/// assert_eq!(numeric::reply_name(433), Some("ERR_NICKNAMEINUSE"));
/// assert_eq!(numeric::reply_name(999), None);
/// ```
pub fn reply_name(code: u16) -> Option<&'static str> {
    match code {
        RPL_WELCOME => Some("RPL_WELCOME"),
        RPL_YOURHOST => Some("RPL_YOURHOST"),
        RPL_CREATED => Some("RPL_CREATED"),
        RPL_NAMREPLY => Some("RPL_NAMREPLY"),
        RPL_ENDOFNAMES => Some("RPL_ENDOFNAMES"),
        RPL_MOTD => Some("RPL_MOTD"),
        RPL_MOTDSTART => Some("RPL_MOTDSTART"),
        RPL_ENDOFMOTD => Some("RPL_ENDOFMOTD"),
        ERR_NONICKNAMEGIVEN => Some("ERR_NONICKNAMEGIVEN"),
        ERR_ERRONEUSNICKNAME => Some("ERR_ERRONEUSNICKNAME"),
        ERR_NICKNAMEINUSE => Some("ERR_NICKNAMEINUSE"),
        ERR_NEEDMOREPARAMS => Some("ERR_NEEDMOREPARAMS"),
        ERR_ALREADYREGISTERED => Some("ERR_ALREADYREGISTERED"),
        _ => None,
    }
}

/// Returns `true` if the numeric code represents an error reply (400–599).
pub fn is_error(code: u16) -> bool {
    (400..600).contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_reply_constants() {
        assert_eq!(RPL_WELCOME, 1);
        assert_eq!(RPL_YOURHOST, 2);
        assert_eq!(RPL_CREATED, 3);
        assert_eq!(RPL_NAMREPLY, 353);
        assert_eq!(RPL_ENDOFNAMES, 366);
        assert_eq!(RPL_MOTD, 372);
        assert_eq!(RPL_MOTDSTART, 375);
        assert_eq!(RPL_ENDOFMOTD, 376);
    }

    #[test]
    fn error_reply_constants() {
        assert_eq!(ERR_NONICKNAMEGIVEN, 431);
        assert_eq!(ERR_ERRONEUSNICKNAME, 432);
        assert_eq!(ERR_NICKNAMEINUSE, 433);
        assert_eq!(ERR_NEEDMOREPARAMS, 461);
        assert_eq!(ERR_ALREADYREGISTERED, 462);
    }

    #[test]
    fn reply_name_known_codes() {
        assert_eq!(reply_name(RPL_WELCOME), Some("RPL_WELCOME"));
        assert_eq!(reply_name(RPL_YOURHOST), Some("RPL_YOURHOST"));
        assert_eq!(reply_name(RPL_CREATED), Some("RPL_CREATED"));
        assert_eq!(reply_name(RPL_NAMREPLY), Some("RPL_NAMREPLY"));
        assert_eq!(reply_name(RPL_ENDOFNAMES), Some("RPL_ENDOFNAMES"));
        assert_eq!(reply_name(RPL_MOTD), Some("RPL_MOTD"));
        assert_eq!(reply_name(RPL_MOTDSTART), Some("RPL_MOTDSTART"));
        assert_eq!(reply_name(RPL_ENDOFMOTD), Some("RPL_ENDOFMOTD"));
        assert_eq!(reply_name(ERR_NONICKNAMEGIVEN), Some("ERR_NONICKNAMEGIVEN"));
        assert_eq!(
            reply_name(ERR_ERRONEUSNICKNAME),
            Some("ERR_ERRONEUSNICKNAME")
        );
        assert_eq!(reply_name(ERR_NICKNAMEINUSE), Some("ERR_NICKNAMEINUSE"));
        assert_eq!(reply_name(ERR_NEEDMOREPARAMS), Some("ERR_NEEDMOREPARAMS"));
        assert_eq!(
            reply_name(ERR_ALREADYREGISTERED),
            Some("ERR_ALREADYREGISTERED")
        );
    }

    #[test]
    fn reply_name_unknown_code() {
        assert_eq!(reply_name(0), None);
        assert_eq!(reply_name(999), None);
        assert_eq!(reply_name(500), None);
    }

    #[test]
    fn is_error_for_error_codes() {
        assert!(is_error(ERR_NONICKNAMEGIVEN));
        assert!(is_error(ERR_ERRONEUSNICKNAME));
        assert!(is_error(ERR_NICKNAMEINUSE));
        assert!(is_error(ERR_NEEDMOREPARAMS));
        assert!(is_error(ERR_ALREADYREGISTERED));
        assert!(is_error(400));
        assert!(is_error(599));
    }

    #[test]
    fn is_error_for_non_error_codes() {
        assert!(!is_error(RPL_WELCOME));
        assert!(!is_error(RPL_NAMREPLY));
        assert!(!is_error(RPL_MOTD));
        assert!(!is_error(0));
        assert!(!is_error(399));
        assert!(!is_error(600));
    }
}
