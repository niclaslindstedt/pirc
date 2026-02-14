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
/// Server info: `<servername> <version> <available user modes> <available channel modes>`.
pub const RPL_MYINFO: u16 = 4;
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

// ---- List replies ----

/// An entry in the channel list: `<channel> <visible> :<topic>`.
pub const RPL_LIST: u16 = 322;
/// End of /LIST.
pub const RPL_LISTEND: u16 = 323;

// ---- Channel mode replies ----

/// Channel mode string: `<channel> <mode> <mode params>`.
pub const RPL_CHANNELMODEIS: u16 = 324;

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

// ---- Topic replies ----

/// No topic is set for channel.
pub const RPL_NOTOPIC: u16 = 331;
/// Channel topic: `<channel> :<topic>`.
pub const RPL_TOPIC: u16 = 332;
/// Topic set by / at: `<channel> <nick> <setat>`.
pub const RPL_TOPICWHOTIME: u16 = 333;

// ---- Invite replies ----

/// Invitation sent: `<channel> <nick>`.
pub const RPL_INVITING: u16 = 341;

// ---- Kill replies ----

/// Kill done (optional).
pub const RPL_KILLDONE: u16 = 361;

// ---- Operator replies ----

/// You are now an IRC operator.
pub const RPL_YOUREOPER: u16 = 381;

// ---- Ban list replies ----

/// Ban list entry: `<channel> <banmask>`.
pub const RPL_BANLIST: u16 = 367;
/// End of channel ban list.
pub const RPL_ENDOFBANLIST: u16 = 368;

// ---- Error replies ----

/// No such nick/channel.
pub const ERR_NOSUCHNICK: u16 = 401;
/// No such channel.
pub const ERR_NOSUCHCHANNEL: u16 = 403;
/// Cannot send to channel.
pub const ERR_CANNOTSENDTOCHAN: u16 = 404;
/// Too many channels joined.
pub const ERR_TOOMANYCHANNELS: u16 = 405;
/// No nickname given.
pub const ERR_NONICKNAMEGIVEN: u16 = 431;
/// Erroneous nickname.
pub const ERR_ERRONEUSNICKNAME: u16 = 432;
/// Nickname is already in use.
pub const ERR_NICKNAMEINUSE: u16 = 433;
/// Nick collision (KILL): `<nick> :<reason>`.
pub const ERR_NICKCOLLISION: u16 = 436;
/// MOTD file is missing.
pub const ERR_NOMOTD: u16 = 422;
/// User not in channel: `<nick> <channel>`.
pub const ERR_USERNOTINCHANNEL: u16 = 441;
/// You're not on that channel.
pub const ERR_NOTONCHANNEL: u16 = 442;
/// User is already on channel: `<user> <channel>`.
pub const ERR_USERONCHANNEL: u16 = 443;
/// Not enough parameters.
pub const ERR_NEEDMOREPARAMS: u16 = 461;
/// You may not reregister.
pub const ERR_ALREADYREGISTERED: u16 = 462;
/// Password incorrect (used by OPER).
pub const ERR_PASSWDMISMATCH: u16 = 464;
/// Cannot join channel (+l): channel is full.
pub const ERR_CHANNELISFULL: u16 = 471;
/// Unknown mode character.
pub const ERR_UNKNOWNMODE: u16 = 472;
/// Cannot join channel (+i): invite only.
pub const ERR_INVITEONLYCHAN: u16 = 473;
/// Cannot join channel (+b): banned.
pub const ERR_BANNEDCHANNEL: u16 = 474;
/// Cannot join channel (+k): bad channel key.
pub const ERR_BADCHANNELKEY: u16 = 475;
/// Bad channel mask.
pub const ERR_BADCHANMASK: u16 = 476;
/// Permission denied - You're not an IRC operator.
pub const ERR_NOPRIVILEGES: u16 = 481;
/// Channel operator privileges needed.
pub const ERR_CHANOPRIVSNEEDED: u16 = 482;
/// Cannot kill server.
pub const ERR_CANTKILLSERVER: u16 = 483;
/// No O-lines for your host.
pub const ERR_NOOPERHOST: u16 = 491;
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
        RPL_MYINFO => Some("RPL_MYINFO"),
        RPL_UMODEIS => Some("RPL_UMODEIS"),
        RPL_LIST => Some("RPL_LIST"),
        RPL_LISTEND => Some("RPL_LISTEND"),
        RPL_CHANNELMODEIS => Some("RPL_CHANNELMODEIS"),
        RPL_AWAY => Some("RPL_AWAY"),
        RPL_UNAWAY => Some("RPL_UNAWAY"),
        RPL_NOWAWAY => Some("RPL_NOWAWAY"),
        RPL_WHOISUSER => Some("RPL_WHOISUSER"),
        RPL_WHOISSERVER => Some("RPL_WHOISSERVER"),
        RPL_WHOISOPERATOR => Some("RPL_WHOISOPERATOR"),
        RPL_WHOISIDLE => Some("RPL_WHOISIDLE"),
        RPL_ENDOFWHOIS => Some("RPL_ENDOFWHOIS"),
        RPL_WHOISCHANNELS => Some("RPL_WHOISCHANNELS"),
        RPL_NOTOPIC => Some("RPL_NOTOPIC"),
        RPL_TOPIC => Some("RPL_TOPIC"),
        RPL_TOPICWHOTIME => Some("RPL_TOPICWHOTIME"),
        RPL_INVITING => Some("RPL_INVITING"),
        RPL_NAMREPLY => Some("RPL_NAMREPLY"),
        RPL_ENDOFNAMES => Some("RPL_ENDOFNAMES"),
        RPL_MOTD => Some("RPL_MOTD"),
        RPL_MOTDSTART => Some("RPL_MOTDSTART"),
        RPL_ENDOFMOTD => Some("RPL_ENDOFMOTD"),
        RPL_KILLDONE => Some("RPL_KILLDONE"),
        RPL_BANLIST => Some("RPL_BANLIST"),
        RPL_ENDOFBANLIST => Some("RPL_ENDOFBANLIST"),
        RPL_YOUREOPER => Some("RPL_YOUREOPER"),
        ERR_NOSUCHNICK => Some("ERR_NOSUCHNICK"),
        ERR_NOSUCHCHANNEL => Some("ERR_NOSUCHCHANNEL"),
        ERR_CANNOTSENDTOCHAN => Some("ERR_CANNOTSENDTOCHAN"),
        ERR_TOOMANYCHANNELS => Some("ERR_TOOMANYCHANNELS"),
        ERR_NOMOTD => Some("ERR_NOMOTD"),
        ERR_NONICKNAMEGIVEN => Some("ERR_NONICKNAMEGIVEN"),
        ERR_ERRONEUSNICKNAME => Some("ERR_ERRONEUSNICKNAME"),
        ERR_NICKNAMEINUSE => Some("ERR_NICKNAMEINUSE"),
        ERR_NICKCOLLISION => Some("ERR_NICKCOLLISION"),
        ERR_USERNOTINCHANNEL => Some("ERR_USERNOTINCHANNEL"),
        ERR_NOTONCHANNEL => Some("ERR_NOTONCHANNEL"),
        ERR_USERONCHANNEL => Some("ERR_USERONCHANNEL"),
        ERR_NEEDMOREPARAMS => Some("ERR_NEEDMOREPARAMS"),
        ERR_ALREADYREGISTERED => Some("ERR_ALREADYREGISTERED"),
        ERR_PASSWDMISMATCH => Some("ERR_PASSWDMISMATCH"),
        ERR_CHANNELISFULL => Some("ERR_CHANNELISFULL"),
        ERR_UNKNOWNMODE => Some("ERR_UNKNOWNMODE"),
        ERR_INVITEONLYCHAN => Some("ERR_INVITEONLYCHAN"),
        ERR_BANNEDCHANNEL => Some("ERR_BANNEDCHANNEL"),
        ERR_BADCHANNELKEY => Some("ERR_BADCHANNELKEY"),
        ERR_BADCHANMASK => Some("ERR_BADCHANMASK"),
        ERR_NOPRIVILEGES => Some("ERR_NOPRIVILEGES"),
        ERR_CHANOPRIVSNEEDED => Some("ERR_CHANOPRIVSNEEDED"),
        ERR_CANTKILLSERVER => Some("ERR_CANTKILLSERVER"),
        ERR_NOOPERHOST => Some("ERR_NOOPERHOST"),
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
        assert_eq!(RPL_MYINFO, 4);
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
        assert_eq!(RPL_KILLDONE, 361);
        assert_eq!(RPL_MOTD, 372);
        assert_eq!(RPL_MOTDSTART, 375);
        assert_eq!(RPL_ENDOFMOTD, 376);
        assert_eq!(RPL_YOUREOPER, 381);
    }

    #[test]
    fn channel_reply_constants() {
        assert_eq!(RPL_LIST, 322);
        assert_eq!(RPL_LISTEND, 323);
        assert_eq!(RPL_CHANNELMODEIS, 324);
        assert_eq!(RPL_NOTOPIC, 331);
        assert_eq!(RPL_TOPIC, 332);
        assert_eq!(RPL_TOPICWHOTIME, 333);
        assert_eq!(RPL_INVITING, 341);
        assert_eq!(RPL_BANLIST, 367);
        assert_eq!(RPL_ENDOFBANLIST, 368);
    }

    #[test]
    fn error_reply_constants() {
        assert_eq!(ERR_NOSUCHNICK, 401);
        assert_eq!(ERR_NOSUCHCHANNEL, 403);
        assert_eq!(ERR_CANNOTSENDTOCHAN, 404);
        assert_eq!(ERR_TOOMANYCHANNELS, 405);
        assert_eq!(ERR_NOMOTD, 422);
        assert_eq!(ERR_NONICKNAMEGIVEN, 431);
        assert_eq!(ERR_ERRONEUSNICKNAME, 432);
        assert_eq!(ERR_NICKNAMEINUSE, 433);
        assert_eq!(ERR_NICKCOLLISION, 436);
        assert_eq!(ERR_USERNOTINCHANNEL, 441);
        assert_eq!(ERR_NOTONCHANNEL, 442);
        assert_eq!(ERR_USERONCHANNEL, 443);
        assert_eq!(ERR_NEEDMOREPARAMS, 461);
        assert_eq!(ERR_ALREADYREGISTERED, 462);
        assert_eq!(ERR_PASSWDMISMATCH, 464);
        assert_eq!(ERR_CHANNELISFULL, 471);
        assert_eq!(ERR_UNKNOWNMODE, 472);
        assert_eq!(ERR_INVITEONLYCHAN, 473);
        assert_eq!(ERR_BANNEDCHANNEL, 474);
        assert_eq!(ERR_BADCHANNELKEY, 475);
        assert_eq!(ERR_BADCHANMASK, 476);
        assert_eq!(ERR_NOPRIVILEGES, 481);
        assert_eq!(ERR_CHANOPRIVSNEEDED, 482);
        assert_eq!(ERR_CANTKILLSERVER, 483);
        assert_eq!(ERR_NOOPERHOST, 491);
        assert_eq!(ERR_UMODEUNKNOWNFLAG, 501);
        assert_eq!(ERR_USERSDONTMATCH, 502);
    }

    #[test]
    fn reply_name_known_codes() {
        assert_eq!(reply_name(RPL_WELCOME), Some("RPL_WELCOME"));
        assert_eq!(reply_name(RPL_YOURHOST), Some("RPL_YOURHOST"));
        assert_eq!(reply_name(RPL_CREATED), Some("RPL_CREATED"));
        assert_eq!(reply_name(RPL_MYINFO), Some("RPL_MYINFO"));
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
        assert_eq!(reply_name(RPL_KILLDONE), Some("RPL_KILLDONE"));
        assert_eq!(reply_name(RPL_MOTDSTART), Some("RPL_MOTDSTART"));
        assert_eq!(reply_name(RPL_ENDOFMOTD), Some("RPL_ENDOFMOTD"));
        assert_eq!(reply_name(RPL_YOUREOPER), Some("RPL_YOUREOPER"));
        assert_eq!(reply_name(ERR_NOSUCHNICK), Some("ERR_NOSUCHNICK"));
        assert_eq!(reply_name(ERR_NOMOTD), Some("ERR_NOMOTD"));
        assert_eq!(reply_name(ERR_NONICKNAMEGIVEN), Some("ERR_NONICKNAMEGIVEN"));
        assert_eq!(
            reply_name(ERR_ERRONEUSNICKNAME),
            Some("ERR_ERRONEUSNICKNAME")
        );
        assert_eq!(reply_name(ERR_NICKNAMEINUSE), Some("ERR_NICKNAMEINUSE"));
        assert_eq!(reply_name(ERR_NICKCOLLISION), Some("ERR_NICKCOLLISION"));
        assert_eq!(reply_name(ERR_NEEDMOREPARAMS), Some("ERR_NEEDMOREPARAMS"));
        assert_eq!(
            reply_name(ERR_ALREADYREGISTERED),
            Some("ERR_ALREADYREGISTERED")
        );
        assert_eq!(
            reply_name(ERR_PASSWDMISMATCH),
            Some("ERR_PASSWDMISMATCH")
        );
        assert_eq!(
            reply_name(ERR_UMODEUNKNOWNFLAG),
            Some("ERR_UMODEUNKNOWNFLAG")
        );
        assert_eq!(reply_name(ERR_USERSDONTMATCH), Some("ERR_USERSDONTMATCH"));
    }

    #[test]
    fn reply_name_channel_codes() {
        assert_eq!(reply_name(RPL_LIST), Some("RPL_LIST"));
        assert_eq!(reply_name(RPL_LISTEND), Some("RPL_LISTEND"));
        assert_eq!(reply_name(RPL_CHANNELMODEIS), Some("RPL_CHANNELMODEIS"));
        assert_eq!(reply_name(RPL_NOTOPIC), Some("RPL_NOTOPIC"));
        assert_eq!(reply_name(RPL_TOPIC), Some("RPL_TOPIC"));
        assert_eq!(reply_name(RPL_TOPICWHOTIME), Some("RPL_TOPICWHOTIME"));
        assert_eq!(reply_name(RPL_INVITING), Some("RPL_INVITING"));
        assert_eq!(reply_name(RPL_BANLIST), Some("RPL_BANLIST"));
        assert_eq!(reply_name(RPL_ENDOFBANLIST), Some("RPL_ENDOFBANLIST"));
        assert_eq!(reply_name(ERR_NOSUCHCHANNEL), Some("ERR_NOSUCHCHANNEL"));
        assert_eq!(
            reply_name(ERR_CANNOTSENDTOCHAN),
            Some("ERR_CANNOTSENDTOCHAN")
        );
        assert_eq!(reply_name(ERR_TOOMANYCHANNELS), Some("ERR_TOOMANYCHANNELS"));
        assert_eq!(
            reply_name(ERR_USERNOTINCHANNEL),
            Some("ERR_USERNOTINCHANNEL")
        );
        assert_eq!(reply_name(ERR_NOTONCHANNEL), Some("ERR_NOTONCHANNEL"));
        assert_eq!(reply_name(ERR_USERONCHANNEL), Some("ERR_USERONCHANNEL"));
        assert_eq!(reply_name(ERR_CHANNELISFULL), Some("ERR_CHANNELISFULL"));
        assert_eq!(reply_name(ERR_UNKNOWNMODE), Some("ERR_UNKNOWNMODE"));
        assert_eq!(reply_name(ERR_INVITEONLYCHAN), Some("ERR_INVITEONLYCHAN"));
        assert_eq!(reply_name(ERR_BANNEDCHANNEL), Some("ERR_BANNEDCHANNEL"));
        assert_eq!(reply_name(ERR_BADCHANNELKEY), Some("ERR_BADCHANNELKEY"));
        assert_eq!(reply_name(ERR_BADCHANMASK), Some("ERR_BADCHANMASK"));
        assert_eq!(reply_name(ERR_NOPRIVILEGES), Some("ERR_NOPRIVILEGES"));
        assert_eq!(
            reply_name(ERR_CHANOPRIVSNEEDED),
            Some("ERR_CHANOPRIVSNEEDED")
        );
        assert_eq!(
            reply_name(ERR_CANTKILLSERVER),
            Some("ERR_CANTKILLSERVER")
        );
        assert_eq!(reply_name(ERR_NOOPERHOST), Some("ERR_NOOPERHOST"));
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
        assert!(is_error(ERR_NOSUCHCHANNEL));
        assert!(is_error(ERR_CANNOTSENDTOCHAN));
        assert!(is_error(ERR_TOOMANYCHANNELS));
        assert!(is_error(ERR_NOMOTD));
        assert!(is_error(ERR_NONICKNAMEGIVEN));
        assert!(is_error(ERR_ERRONEUSNICKNAME));
        assert!(is_error(ERR_NICKNAMEINUSE));
        assert!(is_error(ERR_NICKCOLLISION));
        assert!(is_error(ERR_USERNOTINCHANNEL));
        assert!(is_error(ERR_NOTONCHANNEL));
        assert!(is_error(ERR_USERONCHANNEL));
        assert!(is_error(ERR_NEEDMOREPARAMS));
        assert!(is_error(ERR_ALREADYREGISTERED));
        assert!(is_error(ERR_CHANNELISFULL));
        assert!(is_error(ERR_UNKNOWNMODE));
        assert!(is_error(ERR_INVITEONLYCHAN));
        assert!(is_error(ERR_BANNEDCHANNEL));
        assert!(is_error(ERR_BADCHANNELKEY));
        assert!(is_error(ERR_BADCHANMASK));
        assert!(is_error(ERR_PASSWDMISMATCH));
        assert!(is_error(ERR_NOPRIVILEGES));
        assert!(is_error(ERR_CHANOPRIVSNEEDED));
        assert!(is_error(ERR_CANTKILLSERVER));
        assert!(is_error(ERR_NOOPERHOST));
        assert!(is_error(ERR_UMODEUNKNOWNFLAG));
        assert!(is_error(ERR_USERSDONTMATCH));
        assert!(is_error(400));
        assert!(is_error(599));
    }

    #[test]
    fn is_error_for_non_error_codes() {
        assert!(!is_error(RPL_WELCOME));
        assert!(!is_error(RPL_MYINFO));
        assert!(!is_error(RPL_UMODEIS));
        assert!(!is_error(RPL_LIST));
        assert!(!is_error(RPL_CHANNELMODEIS));
        assert!(!is_error(RPL_NOTOPIC));
        assert!(!is_error(RPL_TOPIC));
        assert!(!is_error(RPL_INVITING));
        assert!(!is_error(RPL_AWAY));
        assert!(!is_error(RPL_WHOISUSER));
        assert!(!is_error(RPL_NAMREPLY));
        assert!(!is_error(RPL_KILLDONE));
        assert!(!is_error(RPL_BANLIST));
        assert!(!is_error(RPL_MOTD));
        assert!(!is_error(RPL_YOUREOPER));
        assert!(!is_error(0));
        assert!(!is_error(399));
        assert!(!is_error(600));
    }
}
