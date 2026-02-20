//! Shared integration test harness and helpers.
//!
//! Provides [`TestServer`] for starting a real pirc server on a random port,
//! [`TestClient`] for connecting and exchanging IRC messages, message builder
//! functions, and assertion helpers.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownController, ShutdownSignal};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::config::ServerConfig;
use pirc_server::group_registry::GroupRegistry;
use pirc_server::handler::{self, HandleResult, PreRegistrationState};
use pirc_server::offline_store::OfflineMessageStore;
use pirc_server::prekey_store::PreKeyBundleStore;
use pirc_server::registry::UserRegistry;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Shared constants
// ---------------------------------------------------------------------------

/// Number of messages in a JOIN burst: JOIN echo, RPL_NOTOPIC, RPL_NAMREPLY,
/// RPL_ENDOFNAMES.
pub const JOIN_BURST_LEN: usize = 4;

// ---------------------------------------------------------------------------
// Network helpers
// ---------------------------------------------------------------------------

/// Bind a listener on a random loopback port.
pub async fn loopback_listener() -> Listener {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    Listener::bind(addr).await.unwrap()
}

/// Create a connected pair of [`Connection`] endpoints via TCP loopback.
pub async fn connection_pair() -> (Connection, Connection) {
    let listener = loopback_listener().await;
    let addr = listener.local_addr().unwrap();
    let (client_result, server_result) =
        tokio::join!(TcpStream::connect(addr), listener.accept());
    let client = Connection::new(client_result.unwrap()).unwrap();
    let server = server_result.unwrap().0;
    (client, server)
}

// ---------------------------------------------------------------------------
// TestServer
// ---------------------------------------------------------------------------

/// A running test server bound to a random port on localhost.
pub struct TestServer {
    pub addr: SocketAddr,
    pub shutdown: ShutdownController,
    pub users: Arc<UserRegistry>,
    pub channels: Arc<ChannelRegistry>,
}

impl TestServer {
    /// Start a test server with default configuration.
    pub async fn start() -> Self {
        Self::with_config(ServerConfig::default()).await
    }

    /// Start a test server with the given configuration.
    ///
    /// The server binds to `127.0.0.1:0` regardless of the config's bind
    /// address so that each test gets a unique ephemeral port.
    pub async fn with_config(config: ServerConfig) -> Self {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = Listener::bind(addr).await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let (shutdown_controller, mut shutdown_signal) = ShutdownSignal::new();

        let users = Arc::new(UserRegistry::new());
        let channels = Arc::new(ChannelRegistry::new());
        let config = Arc::new(config);
        let prekey_store = Arc::new(PreKeyBundleStore::new());
        let offline_store = Arc::new(OfflineMessageStore::default());
        let group_registry = Arc::new(GroupRegistry::new());

        let accept_users = Arc::clone(&users);
        let accept_channels = Arc::clone(&channels);

        tokio::spawn(async move {
            loop {
                match listener.accept_with_shutdown(&mut shutdown_signal).await {
                    Ok(Some((connection, peer_addr))) => {
                        let sig = shutdown_signal.clone();
                        let reg = Arc::clone(&accept_users);
                        let ch = Arc::clone(&accept_channels);
                        let cfg = Arc::clone(&config);
                        let pks = Arc::clone(&prekey_store);
                        let ofs = Arc::clone(&offline_store);
                        let grp = Arc::clone(&group_registry);
                        tokio::spawn(async move {
                            handle_connection(
                                connection, peer_addr, sig, reg, ch, cfg, pks, ofs, grp,
                            )
                            .await;
                        });
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        });

        Self {
            addr: local_addr,
            shutdown: shutdown_controller,
            users,
            channels,
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.shutdown();
    }
}

/// Per-connection handler that mirrors the real server's accept loop.
///
/// Uses `tokio::select!` to concurrently drain outbound messages (e.g.
/// PRIVMSGs routed from other connections) while waiting for inbound data.
async fn handle_connection(
    mut connection: Connection,
    peer_addr: SocketAddr,
    mut shutdown: ShutdownSignal,
    registry: Arc<UserRegistry>,
    channels: Arc<ChannelRegistry>,
    config: Arc<ServerConfig>,
    prekey_store: Arc<PreKeyBundleStore>,
    offline_store: Arc<OfflineMessageStore>,
    group_registry: Arc<GroupRegistry>,
) {
    let conn_id = connection.info().id;
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let mut state = PreRegistrationState::new(peer_addr.ip().to_string());

    loop {
        tokio::select! {
            recv_result = connection.recv_with_shutdown(&mut shutdown) => {
                match recv_result {
                    Ok(Some(msg)) => {
                        let result = handler::handle_message(
                            &msg,
                            conn_id,
                            &registry,
                            &channels,
                            &tx,
                            &mut state,
                            &config,
                            None,
                            &prekey_store,
                            &offline_store,
                            &group_registry,
                        );

                        while let Ok(out_msg) = rx.try_recv() {
                            if connection.send(out_msg).await.is_err() {
                                return;
                            }
                        }

                        match result {
                            HandleResult::Quit | HandleResult::Shutdown => break,
                            HandleResult::Continue => {}
                        }
                    }
                    _ => break,
                }
            }
            Some(out_msg) = rx.recv() => {
                if connection.send(out_msg).await.is_err() {
                    return;
                }
            }
        }
    }

    if state.registered {
        registry.remove_by_connection(conn_id);
    }
}

// ---------------------------------------------------------------------------
// TestClient
// ---------------------------------------------------------------------------

/// Default timeout for receiving a single message.
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_secs(2);

/// The number of messages in a standard welcome burst
/// (RPL_WELCOME, RPL_YOURHOST, RPL_CREATED, ERR_NOMOTD).
const WELCOME_BURST_LEN: usize = 4;

/// A convenience wrapper around [`Connection`] for integration tests.
pub struct TestClient {
    conn: Connection,
}

impl TestClient {
    /// Connect to a test server at `addr`.
    pub async fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        let conn = Connection::new(stream).unwrap();
        Self { conn }
    }

    /// Register with the server by sending NICK + USER and draining the
    /// welcome burst (4 messages: 001, 002, 003, 422).
    pub async fn register(&mut self, nick: &str, username: &str) {
        self.send(nick_msg(nick)).await;
        self.send(user_msg(username, &format!("{nick} Test User"))).await;
        self.drain(WELCOME_BURST_LEN).await;
    }

    /// Send a message and return the first response (with timeout).
    pub async fn send_and_recv(&mut self, msg: Message) -> Message {
        self.send(msg).await;
        self.recv_msg().await
    }

    /// Receive one message with a 2-second timeout.
    pub async fn recv_msg(&mut self) -> Message {
        tokio::time::timeout(DEFAULT_RECV_TIMEOUT, self.conn.recv())
            .await
            .expect("timeout waiting for message")
            .expect("recv error")
            .expect("unexpected EOF")
    }

    /// Try to receive a message, returning `None` on timeout instead of
    /// panicking.
    pub async fn try_recv_msg(&mut self) -> Option<Message> {
        match tokio::time::timeout(DEFAULT_RECV_TIMEOUT, self.conn.recv()).await {
            Ok(Ok(Some(msg))) => Some(msg),
            _ => None,
        }
    }

    /// Try to receive a message with a short timeout (200ms), returning `None`
    /// if nothing arrives. Useful for asserting no message was sent.
    pub async fn try_recv_short(&mut self) -> Option<Message> {
        match tokio::time::timeout(Duration::from_millis(200), self.conn.recv()).await {
            Ok(Ok(Some(msg))) => Some(msg),
            _ => None,
        }
    }

    /// Receive and discard `n` messages.
    pub async fn drain(&mut self, n: usize) {
        for _ in 0..n {
            self.recv_msg().await;
        }
    }

    /// Send a raw message.
    pub async fn send(&mut self, msg: Message) {
        self.conn.send(msg).await.unwrap();
    }

    /// Shut down the underlying connection.
    pub async fn shutdown(&mut self) {
        self.conn.shutdown().await.ok();
    }
}

// ---------------------------------------------------------------------------
// Message builders
// ---------------------------------------------------------------------------

/// `NICK <nick>`
pub fn nick_msg(nick: &str) -> Message {
    Message::new(Command::Nick, vec![nick.to_owned()])
}

/// `USER <username> 0 * :<realname>`
pub fn user_msg(username: &str, realname: &str) -> Message {
    Message::new(
        Command::User,
        vec![
            username.to_owned(),
            "0".to_owned(),
            "*".to_owned(),
            realname.to_owned(),
        ],
    )
}

/// `JOIN <channel>`
pub fn join_msg(channel: &str) -> Message {
    Message::new(Command::Join, vec![channel.to_owned()])
}

/// `PART <channel>`
pub fn part_msg(channel: &str) -> Message {
    Message::new(Command::Part, vec![channel.to_owned()])
}

/// `PRIVMSG <target> :<text>`
pub fn privmsg(target: &str, text: &str) -> Message {
    Message::new(
        Command::Privmsg,
        vec![target.to_owned(), text.to_owned()],
    )
}

/// `QUIT :<reason>`
pub fn quit_msg(reason: &str) -> Message {
    Message::new(Command::Quit, vec![reason.to_owned()])
}

/// `PING <token>`
pub fn ping_msg(token: &str) -> Message {
    Message::new(Command::Ping, vec![token.to_owned()])
}

/// `PONG <token>`
pub fn pong_msg(token: &str) -> Message {
    Message::new(Command::Pong, vec![token.to_owned()])
}

/// `MODE <target> <mode>`
pub fn mode_msg(target: &str, mode: &str) -> Message {
    Message::new(
        Command::Mode,
        vec![target.to_owned(), mode.to_owned()],
    )
}

/// `KICK <channel> <nick>`
pub fn kick_msg(channel: &str, nick: &str) -> Message {
    Message::new(
        Command::Kick,
        vec![channel.to_owned(), nick.to_owned()],
    )
}

/// `KICK <channel> <nick> :<reason>`
pub fn kick_msg_with_reason(channel: &str, nick: &str, reason: &str) -> Message {
    Message::new(
        Command::Kick,
        vec![channel.to_owned(), nick.to_owned(), reason.to_owned()],
    )
}

/// `TOPIC <channel>`
pub fn topic_query(channel: &str) -> Message {
    Message::new(Command::Topic, vec![channel.to_owned()])
}

/// `TOPIC <channel> :<text>`
pub fn topic_msg(channel: &str, text: &str) -> Message {
    Message::new(
        Command::Topic,
        vec![channel.to_owned(), text.to_owned()],
    )
}

/// `INVITE <nick> <channel>`
pub fn invite_msg(nick: &str, channel: &str) -> Message {
    Message::new(
        Command::Invite,
        vec![nick.to_owned(), channel.to_owned()],
    )
}

/// `NOTICE <target> :<text>`
pub fn notice_msg(target: &str, text: &str) -> Message {
    Message::new(
        Command::Notice,
        vec![target.to_owned(), text.to_owned()],
    )
}

/// `MODE <target> <modestring> [params...]`
pub fn mode_msg_with_params(target: &str, mode: &str, params: &[&str]) -> Message {
    let mut p = vec![target.to_owned(), mode.to_owned()];
    for param in params {
        p.push((*param).to_owned());
    }
    Message::new(Command::Mode, p)
}

/// `WHOIS <nick>`
pub fn whois_msg(nick: &str) -> Message {
    Message::new(Command::Whois, vec![nick.to_owned()])
}

/// `AWAY :<message>` — set away status.
pub fn away_msg(message: &str) -> Message {
    Message::new(Command::Away, vec![message.to_owned()])
}

/// `AWAY` — clear away status.
pub fn away_clear() -> Message {
    Message::new(Command::Away, vec![])
}

/// `JOIN <channel> <key>`
pub fn join_msg_with_key(channel: &str, key: &str) -> Message {
    Message::new(
        Command::Join,
        vec![channel.to_owned(), key.to_owned()],
    )
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

/// Assert that `msg` is a numeric reply with the given code.
///
/// # Panics
///
/// Panics if the message is not a numeric or does not match `expected`.
pub fn assert_numeric(msg: &Message, expected: u16) {
    assert_eq!(
        msg.numeric_code(),
        Some(expected),
        "expected numeric {expected}, got {:?}",
        msg.command
    );
}

/// Assert that `msg` has the given command.
///
/// # Panics
///
/// Panics if the command does not match.
pub fn assert_command(msg: &Message, expected: Command) {
    assert_eq!(
        msg.command, expected,
        "expected command {expected:?}, got {:?}",
        msg.command
    );
}

/// Assert that the parameter at `index` contains `substring`.
///
/// # Panics
///
/// Panics if the parameter is missing or does not contain the substring.
pub fn assert_param_contains(msg: &Message, index: usize, substring: &str) {
    let param = msg
        .params
        .get(index)
        .unwrap_or_else(|| panic!("message has no param at index {index}: {msg:?}"));
    assert!(
        param.contains(substring),
        "param[{index}] = {param:?} does not contain {substring:?}"
    );
}
