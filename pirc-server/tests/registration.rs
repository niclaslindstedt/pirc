//! Integration tests for the client registration flow (NICK + USER + welcome).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownSignal};
use pirc_protocol::numeric::{
    ERR_ALREADYREGISTERED, ERR_ERRONEUSNICKNAME, ERR_NEEDMOREPARAMS, ERR_NICKNAMEINUSE, ERR_NOMOTD,
    ERR_NONICKNAMEGIVEN, RPL_CREATED, RPL_WELCOME, RPL_YOURHOST,
};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::config::ServerConfig;
use pirc_server::handler::{self, PreRegistrationState};
use pirc_server::registry::UserRegistry;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Start a test server that uses the real handler with registration support.
async fn start_registration_server() -> (SocketAddr, pirc_network::ShutdownController) {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = Listener::bind(addr).await.unwrap();
    let local_addr = listener.local_addr().unwrap();

    let (shutdown_controller, mut shutdown_signal) = ShutdownSignal::new();

    let registry = Arc::new(UserRegistry::new());
    let channels = Arc::new(ChannelRegistry::new());
    let config = Arc::new(ServerConfig::default());

    tokio::spawn(async move {
        loop {
            match listener.accept_with_shutdown(&mut shutdown_signal).await {
                Ok(Some((connection, peer_addr))) => {
                    let conn_shutdown = shutdown_signal.clone();
                    let conn_registry = Arc::clone(&registry);
                    let conn_channels = Arc::clone(&channels);
                    let conn_config = Arc::clone(&config);
                    tokio::spawn(async move {
                        handle_registration_connection(
                            connection,
                            peer_addr,
                            conn_shutdown,
                            conn_registry,
                            conn_channels,
                            conn_config,
                        )
                        .await;
                    });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });

    (local_addr, shutdown_controller)
}

async fn handle_registration_connection(
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

async fn recv_msg(client: &mut Connection) -> Message {
    tokio::time::timeout(Duration::from_secs(2), client.recv())
        .await
        .expect("timeout waiting for response")
        .expect("recv error")
        .expect("unexpected EOF")
}

#[tokio::test]
async fn nick_then_user_receives_welcome_burst() {
    let (addr, shutdown) = start_registration_server().await;
    let mut client = connect(addr).await;

    client.send(nick_msg("IntegAlice")).await.unwrap();
    client.send(user_msg("alice", "Alice Test")).await.unwrap();

    let welcome = recv_msg(&mut client).await;
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
    assert!(welcome.trailing().unwrap().contains("IntegAlice"));

    let yourhost = recv_msg(&mut client).await;
    assert_eq!(yourhost.numeric_code(), Some(RPL_YOURHOST));

    let created = recv_msg(&mut client).await;
    assert_eq!(created.numeric_code(), Some(RPL_CREATED));

    let nomotd = recv_msg(&mut client).await;
    assert_eq!(nomotd.numeric_code(), Some(ERR_NOMOTD));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn user_then_nick_receives_welcome_burst() {
    let (addr, shutdown) = start_registration_server().await;
    let mut client = connect(addr).await;

    client.send(user_msg("bob", "Bob Test")).await.unwrap();
    client.send(nick_msg("IntegBob")).await.unwrap();

    let welcome = recv_msg(&mut client).await;
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
    assert!(welcome.trailing().unwrap().contains("IntegBob"));

    let yourhost = recv_msg(&mut client).await;
    assert_eq!(yourhost.numeric_code(), Some(RPL_YOURHOST));

    let created = recv_msg(&mut client).await;
    assert_eq!(created.numeric_code(), Some(RPL_CREATED));

    let nomotd = recv_msg(&mut client).await;
    assert_eq!(nomotd.numeric_code(), Some(ERR_NOMOTD));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn invalid_nick_returns_erroneous_nickname() {
    let (addr, shutdown) = start_registration_server().await;
    let mut client = connect(addr).await;

    client.send(nick_msg("123bad")).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_ERRONEUSNICKNAME));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn duplicate_nick_returns_nick_in_use() {
    let (addr, shutdown) = start_registration_server().await;

    // First client registers
    let mut client1 = connect(addr).await;
    client1.send(nick_msg("DupNick")).await.unwrap();
    client1.send(user_msg("user1", "User One")).await.unwrap();
    let welcome = recv_msg(&mut client1).await;
    assert_eq!(welcome.numeric_code(), Some(RPL_WELCOME));
    // Drain remaining welcome burst
    let _ = recv_msg(&mut client1).await; // 002
    let _ = recv_msg(&mut client1).await; // 003
    let _ = recv_msg(&mut client1).await; // 422

    // Second client tries same nick
    let mut client2 = connect(addr).await;
    client2.send(nick_msg("DupNick")).await.unwrap();

    let reply = recv_msg(&mut client2).await;
    assert_eq!(reply.numeric_code(), Some(ERR_NICKNAMEINUSE));

    client1.shutdown().await.ok();
    client2.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn nick_no_param_returns_no_nickname_given() {
    let (addr, shutdown) = start_registration_server().await;
    let mut client = connect(addr).await;

    let msg = Message::new(Command::Nick, vec![]);
    client.send(msg).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_NONICKNAMEGIVEN));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn user_missing_params_returns_need_more_params() {
    let (addr, shutdown) = start_registration_server().await;
    let mut client = connect(addr).await;

    let msg = Message::new(Command::User, vec!["onlyuser".to_owned()]);
    client.send(msg).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_NEEDMOREPARAMS));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn user_after_registration_returns_already_registered() {
    let (addr, shutdown) = start_registration_server().await;
    let mut client = connect(addr).await;

    // Register
    client.send(nick_msg("RegUser")).await.unwrap();
    client.send(user_msg("reguser", "Reg User")).await.unwrap();

    // Drain welcome burst
    let _ = recv_msg(&mut client).await; // 001
    let _ = recv_msg(&mut client).await; // 002
    let _ = recv_msg(&mut client).await; // 003
    let _ = recv_msg(&mut client).await; // 422

    // Try USER again
    client.send(user_msg("reguser2", "Another")).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_ALREADYREGISTERED));

    client.shutdown().await.ok();
    shutdown.shutdown();
}
