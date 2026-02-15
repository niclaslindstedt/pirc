//! Integration tests for the WHOIS command handler.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_common::Nickname;
use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownSignal};
use pirc_protocol::numeric::{
    ERR_NONICKNAMEGIVEN, ERR_NOSUCHNICK, RPL_AWAY, RPL_ENDOFWHOIS, RPL_WELCOME, RPL_WHOISIDLE,
    RPL_WHOISOPERATOR, RPL_WHOISSERVER, RPL_WHOISUSER,
};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::config::ServerConfig;
use pirc_server::handler::{self, PreRegistrationState};
use pirc_server::registry::UserRegistry;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Start a test server that uses the real handler with registration support.
async fn start_server() -> (
    SocketAddr,
    pirc_network::ShutdownController,
    Arc<UserRegistry>,
) {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = Listener::bind(addr).await.unwrap();
    let local_addr = listener.local_addr().unwrap();

    let (shutdown_controller, mut shutdown_signal) = ShutdownSignal::new();

    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = Arc::new(ServerConfig::default());

    let conn_registry = Arc::clone(&registry);
    let conn_channels = Arc::clone(&channels);
    tokio::spawn(async move {
        loop {
            match listener.accept_with_shutdown(&mut shutdown_signal).await {
                Ok(Some((connection, peer_addr))) => {
                    let conn_shutdown = shutdown_signal.clone();
                    let registry = Arc::clone(&conn_registry);
                    let channels = Arc::clone(&conn_channels);
                    let config = Arc::clone(&config);
                    tokio::spawn(async move {
                        handle_connection(
                            connection,
                            peer_addr,
                            conn_shutdown,
                            registry,
                            channels,
                            config,
                        )
                        .await;
                    });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });

    (local_addr, shutdown_controller, registry)
}

async fn handle_connection(
    mut connection: Connection,
    peer_addr: SocketAddr,
    mut shutdown: ShutdownSignal,
    registry: Arc<UserRegistry>,
    channels: Arc<ChannelRegistry>,
    config: Arc<ServerConfig>,
) {
    let conn_id = connection.info().id;
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let mut state = PreRegistrationState::new(peer_addr.ip().to_string());

    loop {
        match connection.recv_with_shutdown(&mut shutdown).await {
            Ok(Some(msg)) => {
                handler::handle_message(
                    &msg, conn_id, &registry, &channels, &tx, &mut state, &config, None,
                );
                while let Ok(out_msg) = rx.try_recv() {
                    if connection.send(out_msg).await.is_err() {
                        return;
                    }
                }
            }
            _ => break,
        }
    }

    if state.registered {
        registry.remove_by_connection(conn_id);
    }
}

async fn connect(addr: SocketAddr) -> Connection {
    let stream = TcpStream::connect(addr).await.unwrap();
    Connection::new(stream).unwrap()
}

fn nick_msg(nick: &str) -> Message {
    Message::new(Command::Nick, vec![nick.to_owned()])
}

fn user_msg(username: &str, realname: &str) -> Message {
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

fn whois_msg(nick: &str) -> Message {
    Message::new(Command::Whois, vec![nick.to_owned()])
}

async fn recv_msg(client: &mut Connection) -> Message {
    tokio::time::timeout(Duration::from_secs(2), client.recv())
        .await
        .expect("timeout waiting for response")
        .expect("recv error")
        .expect("unexpected EOF")
}

/// Register a client, drain the welcome burst (001, 002, 003, 422).
async fn register_and_drain(client: &mut Connection, nick: &str, username: &str) {
    client.send(nick_msg(nick)).await.unwrap();
    client
        .send(user_msg(username, &format!("{nick} Test")))
        .await
        .unwrap();

    let welcome = recv_msg(client).await;
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
    let _ = recv_msg(client).await; // 002
    let _ = recv_msg(client).await; // 003
    let _ = recv_msg(client).await; // 422
}

#[tokio::test]
async fn whois_existing_user_returns_full_reply() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client1 = connect(addr).await;
    register_and_drain(&mut client1, "Alice", "alice").await;

    let mut client2 = connect(addr).await;
    register_and_drain(&mut client2, "Bob", "bob").await;

    // Alice does WHOIS Bob
    client1.send(whois_msg("Bob")).await.unwrap();

    // RPL_WHOISUSER (311)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISUSER));
    assert_eq!(reply.params[0], "Alice");
    assert_eq!(reply.params[1], "Bob");
    assert_eq!(reply.params[2], "bob"); // username

    // RPL_WHOISSERVER (312)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISSERVER));

    // RPL_WHOISIDLE (317)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));
    let idle_secs: u64 = reply.params[2].parse().expect("idle is numeric");
    assert!(idle_secs < 5);
    let signon: u64 = reply.params[3].parse().expect("signon is numeric");
    assert!(signon > 0, "signon should be a non-zero Unix timestamp");

    // RPL_ENDOFWHOIS (318)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));

    client1.shutdown().await.ok();
    client2.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn whois_nonexistent_nick() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    client.send(whois_msg("NoSuchUser")).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_NOSUCHNICK));

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn whois_no_parameter() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    let msg = Message::new(Command::Whois, vec![]);
    client.send(msg).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn whois_away_user() {
    let (addr, shutdown, registry) = start_server().await;

    let mut client1 = connect(addr).await;
    register_and_drain(&mut client1, "Alice", "alice").await;

    let mut client2 = connect(addr).await;
    register_and_drain(&mut client2, "Bob", "bob").await;

    // Set Bob as away via the registry directly
    {
        let bob = Nickname::new("Bob").unwrap();
        let session = registry.get_by_nick(&bob).unwrap();
        let mut s = session.write().unwrap();
        s.away_message = Some("On vacation".to_owned());
    }

    client1.send(whois_msg("Bob")).await.unwrap();

    // RPL_WHOISUSER (311)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISUSER));

    // RPL_WHOISSERVER (312)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISSERVER));

    // RPL_AWAY (301)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_AWAY));
    assert!(reply.trailing().unwrap().contains("On vacation"));

    // RPL_WHOISIDLE (317)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));

    // RPL_ENDOFWHOIS (318)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));

    client1.shutdown().await.ok();
    client2.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn whois_operator_user() {
    let (addr, shutdown, registry) = start_server().await;

    let mut client1 = connect(addr).await;
    register_and_drain(&mut client1, "Alice", "alice").await;

    let mut client2 = connect(addr).await;
    register_and_drain(&mut client2, "Bob", "bob").await;

    // Set Bob as operator via the registry directly
    {
        let bob = Nickname::new("Bob").unwrap();
        let session = registry.get_by_nick(&bob).unwrap();
        let mut s = session.write().unwrap();
        s.modes.insert(pirc_common::UserMode::Operator);
    }

    client1.send(whois_msg("Bob")).await.unwrap();

    // RPL_WHOISUSER (311)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISUSER));

    // RPL_WHOISSERVER (312)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISSERVER));

    // RPL_WHOISOPERATOR (313)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISOPERATOR));
    assert!(reply.trailing().unwrap().contains("IRC operator"));

    // RPL_WHOISIDLE (317)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_WHOISIDLE));

    // RPL_ENDOFWHOIS (318)
    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(RPL_ENDOFWHOIS));

    client1.shutdown().await.ok();
    client2.shutdown().await.ok();
    shutdown.shutdown();
}
