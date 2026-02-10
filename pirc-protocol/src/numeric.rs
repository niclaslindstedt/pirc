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

// ---- User mode replies ----

/// Current user mode string.
pub const RPL_UMODEIS: u16 = 221;

// ---- Away replies ----

/// User is away.
pub const RPL_AWAY: u16 = 301;
/// You are no longer marked as being away.
pub const RPL_UNAWAY: u16 = 305;
/// You have been marked as being away.
pub const RPL_NOWAWAY: u16 = 306;

// ---- WHOIS replies ----

/// WHOIS user info: `<nick> <user> <host> * :<realname>`.
pub const RPL_WHOISUSER: u16 = 311;
/// WHOIS server info: `<nick> <server> :<server info>`.
pub const RPL_WHOISSERVER: u16 = 312;
/// WHOIS operator status: `<nick> :is an IRC operator`.
pub const RPL_WHOISOPERATOR: u16 = 313;
/// WHOIS idle time: `<nick> <integer> :seconds idle`.
pub const RPL_WHOISIDLE: u16 = 317;
/// End of WHOIS list.
pub const RPL_ENDOFWHOIS: u16 = 318;
/// WHOIS channel list: `<nick> :{[@|+]<channel><space>}`.
pub const RPL_WHOISCHANNELS: u16 = 319;

// ---- Error replies ----

/// No such nick/channel.
pub const ERR_NOSUCHNICK: u16 = 401;
/// No nickname given.
pub const ERR_NONICKNAMEGIVEN: u16 = 431;
/// Erroneous nickname.
pub const ERR_ERRONEUSNICKNAME: u16 = 432;
/// Nickname is already in use.
pub const ERR_NICKNAMEINUSE: u16 = 433;
/// MOTD file is missing.
pub const ERR_NOMOTD: u16 = 422;
/// Not enough parameters.
pub const ERR_NEEDMOREPARAMS: u16 = 461;
/// You may not reregister.
pub const ERR_ALREADYREGISTERED: u16 = 462;
/// Unknown MODE flag.
pub const ERR_UMODEUNKNOWNFLAG: u16 = 501;
/// Cannot change mode for other users.
pub const ERR_USERSDONTMATCH: u16 = 502;

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
        RPL_UMODEIS => Some("RPL_UMODEIS"),
        RPL_AWAY => Some("RPL_AWAY"),
        RPL_UNAWAY => Some("RPL_UNAWAY"),
        RPL_NOWAWAY => Some("RPL_NOWAWAY"),
        RPL_WHOISUSER => Some("RPL_WHOISUSER"),
        RPL_WHOISSERVER => Some("RPL_WHOISSERVER"),
        RPL_WHOISOPERATOR => Some("RPL_WHOISOPERATOR"),
        RPL_WHOISIDLE => Some("RPL_WHOISIDLE"),
        RPL_ENDOFWHOIS => Some("RPL_ENDOFWHOIS"),
        RPL_WHOISCHANNELS => Some("RPL_WHOISCHANNELS"),
        RPL_NAMREPLY => Some("RPL_NAMREPLY"),
        RPL_ENDOFNAMES => Some("RPL_ENDOFNAMES"),
        RPL_MOTD => Some("RPL_MOTD"),
        RPL_MOTDSTART => Some("RPL_MOTDSTART"),
        RPL_ENDOFMOTD => Some("RPL_ENDOFMOTD"),
        ERR_NOSUCHNICK => Some("ERR_NOSUCHNICK"),
        ERR_NOMOTD => Some("ERR_NOMOTD"),
        ERR_NONICKNAMEGIVEN => Some("ERR_NONICKNAMEGIVEN"),
        ERR_ERRONEUSNICKNAME => Some("ERR_ERRONEUSNICKNAME"),
        ERR_NICKNAMEINUSE => Some("ERR_NICKNAMEINUSE"),
        ERR_NEEDMOREPARAMS => Some("ERR_NEEDMOREPARAMS"),
        ERR_ALREADYREGISTERED => Some("ERR_ALREADYREGISTERED"),
        ERR_UMODEUNKNOWNFLAG => Some("ERR_UMODEUNKNOWNFLAG"),
        ERR_USERSDONTMATCH => Some("ERR_USERSDONTMATCH"),
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
        assert_eq!(RPL_UMODEIS, 221);
        assert_eq!(RPL_AWAY, 301);
        assert_eq!(RPL_UNAWAY, 305);
        assert_eq!(RPL_NOWAWAY, 306);
        assert_eq!(RPL_WHOISUSER, 311);
        assert_eq!(RPL_WHOISSERVER, 312);
        assert_eq!(RPL_WHOISOPERATOR, 313);
        assert_eq!(RPL_WHOISIDLE, 317);
        assert_eq!(RPL_ENDOFWHOIS, 318);
        assert_eq!(RPL_WHOISCHANNELS, 319);
        assert_eq!(RPL_NAMREPLY, 353);
        assert_eq!(RPL_ENDOFNAMES, 366);
        assert_eq!(RPL_MOTD, 372);
        assert_eq!(RPL_MOTDSTART, 375);
        assert_eq!(RPL_ENDOFMOTD, 376);
    }

    #[test]
    fn error_reply_constants() {
        assert_eq!(ERR_NOSUCHNICK, 401);
        assert_eq!(ERR_NOMOTD, 422);
        assert_eq!(ERR_NONICKNAMEGIVEN, 431);
        assert_eq!(ERR_ERRONEUSNICKNAME, 432);
        assert_eq!(ERR_NICKNAMEINUSE, 433);
        assert_eq!(ERR_NEEDMOREPARAMS, 461);
        assert_eq!(ERR_ALREADYREGISTERED, 462);
        assert_eq!(ERR_UMODEUNKNOWNFLAG, 501);
        assert_eq!(ERR_USERSDONTMATCH, 502);
    }

    #[test]
    fn reply_name_known_codes() {
        assert_eq!(reply_name(RPL_WELCOME), Some("RPL_WELCOME"));
        assert_eq!(reply_name(RPL_YOURHOST), Some("RPL_YOURHOST"));
        assert_eq!(reply_name(RPL_CREATED), Some("RPL_CREATED"));
        assert_eq!(reply_name(RPL_UMODEIS), Some("RPL_UMODEIS"));
        assert_eq!(reply_name(RPL_AWAY), Some("RPL_AWAY"));
        assert_eq!(reply_name(RPL_UNAWAY), Some("RPL_UNAWAY"));
        assert_eq!(reply_name(RPL_NOWAWAY), Some("RPL_NOWAWAY"));
        assert_eq!(reply_name(RPL_WHOISUSER), Some("RPL_WHOISUSER"));
        assert_eq!(reply_name(RPL_WHOISSERVER), Some("RPL_WHOISSERVER"));
        assert_eq!(reply_name(RPL_WHOISOPERATOR), Some("RPL_WHOISOPERATOR"));
        assert_eq!(reply_name(RPL_WHOISIDLE), Some("RPL_WHOISIDLE"));
        assert_eq!(reply_name(RPL_ENDOFWHOIS), Some("RPL_ENDOFWHOIS"));
        assert_eq!(reply_name(RPL_WHOISCHANNELS), Some("RPL_WHOISCHANNELS"));
        assert_eq!(reply_name(RPL_NAMREPLY), Some("RPL_NAMREPLY"));
        assert_eq!(reply_name(RPL_ENDOFNAMES), Some("RPL_ENDOFNAMES"));
        assert_eq!(reply_name(RPL_MOTD), Some("RPL_MOTD"));
        assert_eq!(reply_name(RPL_MOTDSTART), Some("RPL_MOTDSTART"));
        assert_eq!(reply_name(RPL_ENDOFMOTD), Some("RPL_ENDOFMOTD"));
        assert_eq!(reply_name(ERR_NOSUCHNICK), Some("ERR_NOSUCHNICK"));
        assert_eq!(reply_name(ERR_NOMOTD), Some("ERR_NOMOTD"));
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
        assert_eq!(
            reply_name(ERR_UMODEUNKNOWNFLAG),
            Some("ERR_UMODEUNKNOWNFLAG")
        );
        assert_eq!(reply_name(ERR_USERSDONTMATCH), Some("ERR_USERSDONTMATCH"));
    }

    #[test]
    fn reply_name_unknown_code() {
        assert_eq!(reply_name(0), None);
        assert_eq!(reply_name(999), None);
        assert_eq!(reply_name(500), None);
    }

    #[test]
    fn is_error_for_error_codes() {
        assert!(is_error(ERR_NOSUCHNICK));
        assert!(is_error(ERR_NOMOTD));
        assert!(is_error(ERR_NONICKNAMEGIVEN));
        assert!(is_error(ERR_ERRONEUSNICKNAME));
        assert!(is_error(ERR_NICKNAMEINUSE));
        assert!(is_error(ERR_NEEDMOREPARAMS));
        assert!(is_error(ERR_ALREADYREGISTERED));
        assert!(is_error(ERR_UMODEUNKNOWNFLAG));
        assert!(is_error(ERR_USERSDONTMATCH));
        assert!(is_error(400));
        assert!(is_error(599));
    }

    #[test]
    fn is_error_for_non_error_codes() {
        assert!(!is_error(RPL_WELCOME));
        assert!(!is_error(RPL_UMODEIS));
        assert!(!is_error(RPL_AWAY));
        assert!(!is_error(RPL_WHOISUSER));
        assert!(!is_error(RPL_NAMREPLY));
        assert!(!is_error(RPL_MOTD));
        assert!(!is_error(0));
        assert!(!is_error(399));
        assert!(!is_error(600));
    }
}
