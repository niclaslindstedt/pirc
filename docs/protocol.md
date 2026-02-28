# Protocol Specification

pirc uses a text-based wire protocol compatible with RFC 2812 (IRC), extended with the `PIRC` command namespace for encryption, clustering, P2P, and group chat features.

## Wire Format

Each message is a single line of UTF-8 text terminated by `\r\n`:

```
:<prefix> <command> <param1> <param2> ... :<trailing>\r\n
```

### Constraints

- **Maximum message length:** 512 bytes (including `\r\n`)
- **Maximum parameters:** 15
- **Prefix:** Optional, indicates the message source
- **Trailing:** The last parameter, prefixed with `:`, may contain spaces

### Prefix Format

```
:<servername>                    Server origin
:<nick>!<user>@<host>           User origin
```

### Examples

```
:server.example.com 001 nick :Welcome to pirc\r\n
:alice!alice@host PRIVMSG #general :Hello everyone\r\n
NICK alice\r\n
JOIN #general\r\n
```

## Standard IRC Commands

### Connection & Registration

| Command | Syntax | Description |
|---------|--------|-------------|
| `NICK` | `NICK <nickname>` | Set or change nickname |
| `USER` | `USER <username> <mode> <unused> :<realname>` | Register username and real name |
| `QUIT` | `QUIT [:<message>]` | Disconnect from server |
| `PING` | `PING <token>` | Connection keepalive request |
| `PONG` | `PONG <token>` | Connection keepalive response |
| `MOTD` | `MOTD` | Request message of the day |

### Channel Operations

| Command | Syntax | Description |
|---------|--------|-------------|
| `JOIN` | `JOIN <channel> [<key>]` | Join a channel |
| `PART` | `PART <channel> [:<message>]` | Leave a channel |
| `TOPIC` | `TOPIC <channel> [:<new topic>]` | Get or set channel topic |
| `MODE` | `MODE <target> [<modes> [<params>]]` | Get or set channel/user modes |
| `KICK` | `KICK <channel> <nick> [:<reason>]` | Remove a user from a channel |
| `BAN` | `BAN <channel> <mask>` | Ban a user from a channel (pirc extension) |
| `INVITE` | `INVITE <nick> <channel>` | Invite a user to a channel |
| `LIST` | `LIST` | List all channels |
| `NAMES` | `NAMES <channel>` | List users in a channel |

### Messaging

| Command | Syntax | Description |
|---------|--------|-------------|
| `PRIVMSG` | `PRIVMSG <target> :<message>` | Send a message to a user or channel |
| `NOTICE` | `NOTICE <target> :<message>` | Send a notice (no auto-reply expected) |

### User Queries

| Command | Syntax | Description |
|---------|--------|-------------|
| `WHOIS` | `WHOIS <nick>` | Query user information |
| `AWAY` | `AWAY [:<message>]` | Set or clear away status |

### Operator Commands

| Command | Syntax | Description |
|---------|--------|-------------|
| `OPER` | `OPER <name> <password>` | Authenticate as IRC operator |
| `KILL` | `KILL <nick> :<reason>` | Forcibly disconnect a user |
| `DIE` | `DIE` | Shut down the server |
| `RESTART` | `RESTART` | Restart the server |
| `WALLOPS` | `WALLOPS :<message>` | Broadcast to all operators |

## Channel Modes

| Mode | Flag | Description |
|------|------|-------------|
| Invite Only | `+i` | Only invited users can join |
| Moderated | `+m` | Only voiced/ops can send messages |
| No External | `+n` | No messages from users not in channel |
| Private | `+p` | Channel is private |
| Secret | `+s` | Channel is hidden from LIST |
| Topic Protected | `+t` | Only operators can set the topic |

### Member Status Modes

| Prefix | Mode | Description |
|--------|------|-------------|
| `@` | `+o` | Channel operator |
| `+` | `+v` | Voiced (can speak in moderated channels) |

## User Modes

| Mode | Flag | Description |
|------|------|-------------|
| Away | `+a` | User is away |
| Invisible | `+i` | User is hidden from WHO |
| Wallops | `+w` | Receives WALLOPS messages |
| Operator | `+o` | Server operator |

## Numeric Replies

### Success Replies

| Code | Name | Description |
|------|------|-------------|
| 001 | `RPL_WELCOME` | Welcome to the IRC network |
| 002 | `RPL_YOURHOST` | Your host is ..., running version ... |
| 003 | `RPL_CREATED` | This server was created ... |
| 004 | `RPL_MYINFO` | Server info (name, version, modes) |
| 221 | `RPL_UMODEIS` | Current user mode string |
| 301 | `RPL_AWAY` | User is away |
| 305 | `RPL_UNAWAY` | You are no longer away |
| 306 | `RPL_NOWAWAY` | You have been marked as away |
| 311 | `RPL_WHOISUSER` | WHOIS user info |
| 312 | `RPL_WHOISSERVER` | WHOIS server info |
| 313 | `RPL_WHOISOPERATOR` | WHOIS operator status |
| 317 | `RPL_WHOISIDLE` | WHOIS idle time |
| 318 | `RPL_ENDOFWHOIS` | End of WHOIS list |
| 319 | `RPL_WHOISCHANNELS` | WHOIS channel list |
| 322 | `RPL_LIST` | Channel list entry |
| 323 | `RPL_LISTEND` | End of channel list |
| 324 | `RPL_CHANNELMODEIS` | Channel mode string |
| 331 | `RPL_NOTOPIC` | No topic is set |
| 332 | `RPL_TOPIC` | Channel topic |
| 333 | `RPL_TOPICWHOTIME` | Topic set by (nick and timestamp) |
| 341 | `RPL_INVITING` | Invitation sent |
| 353 | `RPL_NAMREPLY` | Names list for a channel |
| 366 | `RPL_ENDOFNAMES` | End of NAMES list |
| 367 | `RPL_BANLIST` | Ban list entry |
| 368 | `RPL_ENDOFBANLIST` | End of ban list |
| 372 | `RPL_MOTD` | MOTD text line |
| 375 | `RPL_MOTDSTART` | Start of MOTD |
| 376 | `RPL_ENDOFMOTD` | End of MOTD |
| 381 | `RPL_YOUREOPER` | You are now an IRC operator |

### Error Replies

| Code | Name | Description |
|------|------|-------------|
| 401 | `ERR_NOSUCHNICK` | No such nick/channel |
| 403 | `ERR_NOSUCHCHANNEL` | No such channel |
| 404 | `ERR_CANNOTSENDTOCHAN` | Cannot send to channel |
| 405 | `ERR_TOOMANYCHANNELS` | Too many channels joined |
| 422 | `ERR_NOMOTD` | MOTD file is missing |
| 431 | `ERR_NONICKNAMEGIVEN` | No nickname given |
| 432 | `ERR_ERRONEUSNICKNAME` | Erroneous nickname |
| 433 | `ERR_NICKNAMEINUSE` | Nickname is already in use |
| 436 | `ERR_NICKCOLLISION` | Nick collision (KILL) |
| 441 | `ERR_USERNOTINCHANNEL` | User not in channel |
| 442 | `ERR_NOTONCHANNEL` | You're not on that channel |
| 443 | `ERR_USERONCHANNEL` | User is already on channel |
| 461 | `ERR_NEEDMOREPARAMS` | Not enough parameters |
| 462 | `ERR_ALREADYREGISTERED` | You may not reregister |
| 464 | `ERR_PASSWDMISMATCH` | Password incorrect |
| 471 | `ERR_CHANNELISFULL` | Channel is full (+l) |
| 472 | `ERR_UNKNOWNMODE` | Unknown mode character |
| 473 | `ERR_INVITEONLYCHAN` | Invite only channel (+i) |
| 474 | `ERR_BANNEDCHANNEL` | Banned from channel (+b) |
| 475 | `ERR_BADCHANNELKEY` | Bad channel key (+k) |
| 476 | `ERR_BADCHANMASK` | Bad channel mask |
| 481 | `ERR_NOPRIVILEGES` | Permission denied (not operator) |
| 482 | `ERR_CHANOPRIVSNEEDED` | Channel operator privileges needed |
| 483 | `ERR_CANTKILLSERVER` | Cannot kill server |
| 491 | `ERR_NOOPERHOST` | No O-lines for your host |
| 501 | `ERR_UMODEUNKNOWNFLAG` | Unknown MODE flag |
| 502 | `ERR_USERSDONTMATCH` | Cannot change mode for other users |

Error codes are in the 400-599 range.

## PIRC Extension Commands

All pirc extensions use the `PIRC` command prefix with a subcommand keyword. Namespaced subcommands use a two-word format (e.g., `PIRC CLUSTER JOIN`).

### Core

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `VERSION` | `PIRC VERSION <version>` | Protocol version announcement |
| `CAP` | `PIRC CAP <capability> [...]` | Capability announcement |

### Encryption

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `KEYEXCHANGE` | `PIRC KEYEXCHANGE <target> <public-key-data>` | Initiate X3DH key exchange |
| `KEYEXCHANGE-ACK` | `PIRC KEYEXCHANGE-ACK <target> <public-key-data>` | Acknowledge key exchange |
| `KEYEXCHANGE-COMPLETE` | `PIRC KEYEXCHANGE-COMPLETE <target>` | Key exchange completed |
| `FINGERPRINT` | `PIRC FINGERPRINT <target> <fingerprint>` | Share identity fingerprint |
| `ENCRYPTED` | `PIRC ENCRYPTED <target> <encrypted-payload>` | Send E2E encrypted message |

#### Encryption Handshake Sequence

```
Alice                          Server                         Bob
  │                              │                              │
  │ PIRC KEYEXCHANGE bob <keys>  │                              │
  ├─────────────────────────────►│  PIRC KEYEXCHANGE alice <keys│
  │                              ├─────────────────────────────►│
  │                              │                              │
  │                              │ PIRC KEYEXCHANGE-ACK alice <keys>
  │ PIRC KEYEXCHANGE-ACK bob <keys>                             │
  │◄─────────────────────────────┤◄─────────────────────────────┤
  │                              │                              │
  │ PIRC KEYEXCHANGE-COMPLETE bob│                              │
  ├─────────────────────────────►│PIRC KEYEXCHANGE-COMPLETE alice
  │                              ├─────────────────────────────►│
  │                              │                              │
  │ PIRC ENCRYPTED bob <payload> │                              │
  ├─────────────────────────────►│  PIRC ENCRYPTED alice <payload>
  │                              ├─────────────────────────────►│
```

1. **KEYEXCHANGE:** Initiator sends X3DH public key bundle (identity key, ephemeral key, pre-key, ML-KEM public key)
2. **KEYEXCHANGE-ACK:** Responder sends their key bundle and derives the shared secret
3. **KEYEXCHANGE-COMPLETE:** Both sides confirm session establishment
4. **ENCRYPTED:** All subsequent messages use triple ratchet encryption

### Cluster Management (Server-to-Server)

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `CLUSTER JOIN` | `PIRC CLUSTER JOIN <invite-key>` | Request to join the cluster |
| `CLUSTER WELCOME` | `PIRC CLUSTER WELCOME <server-id> <config>` | Accept new server into cluster |
| `CLUSTER SYNC` | `PIRC CLUSTER SYNC <state-data>` | State synchronization |
| `CLUSTER HEARTBEAT` | `PIRC CLUSTER HEARTBEAT <server-id>` | Cluster keepalive |
| `CLUSTER MIGRATE` | `PIRC CLUSTER MIGRATE <user-id> <target>` | User migration notification |
| `CLUSTER RAFT` | `PIRC CLUSTER RAFT <raft-message>` | Raft consensus protocol message |
| `CLUSTER STATUS` | `PIRC CLUSTER STATUS` | Query cluster status |
| `CLUSTER MEMBERS` | `PIRC CLUSTER MEMBERS` | List cluster members |

### Invite Key Management

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `INVITE-KEY GENERATE` | `PIRC INVITE-KEY GENERATE [ttl]` | Generate a cluster invite key |
| `INVITE-KEY LIST` | `PIRC INVITE-KEY LIST` | List active invite keys |
| `INVITE-KEY REVOKE` | `PIRC INVITE-KEY REVOKE <token>` | Revoke an invite key |

### P2P Signaling

P2P signaling messages are relayed through the server to establish direct UDP connections between clients.

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `P2P OFFER` | `PIRC P2P OFFER <target> <signal-data>` | Send connection offer |
| `P2P ANSWER` | `PIRC P2P ANSWER <target> <signal-data>` | Send connection answer |
| `P2P ICE` | `PIRC P2P ICE <target> <candidate-data>` | Send ICE candidate |
| `P2P ESTABLISHED` | `PIRC P2P ESTABLISHED <target>` | Connection established |
| `P2P FAILED` | `PIRC P2P FAILED <target> <reason>` | Connection failed |

#### P2P Connection Sequence

```
Alice                          Server                         Bob
  │                              │                              │
  │  PIRC P2P OFFER bob <sdp>   │                              │
  ├─────────────────────────────►│  PIRC P2P OFFER alice <sdp> │
  │                              ├─────────────────────────────►│
  │                              │                              │
  │                              │ PIRC P2P ANSWER alice <sdp>  │
  │ PIRC P2P ANSWER bob <sdp>   │                              │
  │◄─────────────────────────────┤◄─────────────────────────────┤
  │                              │                              │
  │  PIRC P2P ICE bob <cand>    │                              │
  ├─────────────────────────────►│  PIRC P2P ICE alice <cand>  │
  │                              ├─────────────────────────────►│
  │                              │  PIRC P2P ICE alice <cand>  │
  │ PIRC P2P ICE bob <cand>     │                              │
  │◄─────────────────────────────┤◄─────────────────────────────┤
  │                              │                              │
  │ ◄═══════ Direct UDP (encrypted) ═══════►                   │
  │                              │                              │
  │ PIRC P2P ESTABLISHED bob    │                              │
  ├─────────────────────────────►│                              │
```

### Group Chat

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `GROUP CREATE` | `PIRC GROUP CREATE <group_id> <group_name>` | Create a new group |
| `GROUP INVITE` | `PIRC GROUP INVITE <group_id> <nick>` | Invite user to group |
| `GROUP JOIN` | `PIRC GROUP JOIN <group_id>` | Accept group invitation |
| `GROUP LEAVE` | `PIRC GROUP LEAVE <group_id>` | Leave a group |
| `GROUP MSG` | `PIRC GROUP MSG <group_id> <encrypted_payload>` | Send encrypted group message |
| `GROUP MEMBERS` | `PIRC GROUP MEMBERS <group_id> <nick1> <nick2> ...` | List group members |
| `GROUP KEYEX` | `PIRC GROUP KEYEX <group_id> <target> <data>` | Group key exchange signaling |
| `GROUP P2P-OFFER` | `PIRC GROUP P2P-OFFER <group_id> <target> <signal>` | Group P2P connection offer |
| `GROUP P2P-ANSWER` | `PIRC GROUP P2P-ANSWER <group_id> <target> <signal>` | Group P2P connection answer |
| `GROUP P2P-ICE` | `PIRC GROUP P2P-ICE <group_id> <target> <candidate>` | Group P2P ICE candidate |

### Network

| Subcommand | Syntax | Description |
|------------|--------|-------------|
| `NETWORK INFO` | `PIRC NETWORK INFO` | Query network information |

## Connection Lifecycle

### Client Registration

```
Client                                     Server
  │                                          │
  │  NICK alice                              │
  ├─────────────────────────────────────────►│
  │  USER alice 0 * :Alice Smith             │
  ├─────────────────────────────────────────►│
  │                                          │
  │  :server 001 alice :Welcome to pirc      │
  │◄─────────────────────────────────────────┤
  │  :server 002 alice :Your host is ...     │
  │◄─────────────────────────────────────────┤
  │  :server 003 alice :Created ...          │
  │◄─────────────────────────────────────────┤
  │  :server 004 alice server pirc-0.1 ...   │
  │◄─────────────────────────────────────────┤
  │  :server 375 alice :- MOTD -             │
  │◄─────────────────────────────────────────┤
  │  :server 372 alice :- Welcome text       │
  │◄─────────────────────────────────────────┤
  │  :server 376 alice :End of MOTD          │
  │◄─────────────────────────────────────────┤
```

Before registration completes (RPL_WELCOME), only `NICK` and `USER` commands are accepted. After registration, all commands are available.

### Keepalive

The server sends `PING <token>` periodically. The client must respond with `PONG <token>` to avoid being disconnected.

## Cluster Protocol

### Server Join

A new server joins the cluster using an invite key:

1. New server sends `PIRC CLUSTER JOIN <invite-key>` to an existing cluster member
2. Existing server validates the invite key (timing-safe comparison, expiry check)
3. On success, responds with `PIRC CLUSTER WELCOME <server-id> <cluster-config>`
4. State synchronization begins with `PIRC CLUSTER SYNC`
5. New server participates in Raft consensus via `PIRC CLUSTER RAFT`

### Raft Consensus Messages

All Raft RPCs are tunneled through `PIRC CLUSTER RAFT <json-encoded-message>`:

- **RequestVote:** Candidate requests votes during leader election
- **RequestVoteResponse:** Follower grants or denies vote
- **AppendEntries:** Leader replicates log entries and sends heartbeats
- **AppendEntriesResponse:** Follower acknowledges replication
- **InstallSnapshot:** Leader sends state snapshot to lagging followers (chunked, 64KB per chunk)
- **InstallSnapshotResponse:** Follower acknowledges snapshot chunk

### User Migration

When a server node fails, its connected users are migrated to healthy nodes:

```
PIRC CLUSTER MIGRATE <user-id> <target-server>
```

The cluster state machine tracks all user registrations, channel memberships, and topics for consistent recovery.
