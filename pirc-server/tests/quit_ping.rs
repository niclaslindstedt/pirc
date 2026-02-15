//! Integration tests for QUIT, PING/PONG, and connection drop handling.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownSignal};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::config::ServerConfig;
use pirc_server::handler::{self, HandleResult, PreRegistrationState};
use pirc_server::prekey_store::PreKeyBundleStore;
use pirc_server::registry::UserRegistry;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

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
                let prekey_store = Arc::new(PreKeyBundleStore::new());
                let result = handler::handle_message(
                    &msg, conn_id, &registry, &channels, &tx, &mut state, &config, None,
                    &prekey_store,
                );
                while let Ok(out_msg) = rx.try_recv() {
                    if connection.send(out_msg).await.is_err() {
                        return;
                    }
                }
                if matches!(result, HandleResult::Quit) {
                    return;
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

async fn register_and_drain(client: &mut Connection, nick: &str, username: &str) {
    client.send(nick_msg(nick)).await.unwrap();
    client
        .send(user_msg(username, &format!("{nick} Test")))
        .await
        .unwrap();

    let welcome = recv_msg(client).await;
    assert_eq!(
        welcome.numeric_code(),
        Some(pirc_protocol::numeric::RPL_WELCOME)
    );
    let _ = recv_msg(client).await; // 002
    let _ = recv_msg(client).await; // 003
    let _ = recv_msg(client).await; // 422
}

// ---- QUIT integration tests ----

#[tokio::test]
async fn quit_with_message_over_tcp() {
    let (addr, shutdown, registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    assert_eq!(registry.connection_count(), 1);

    // Send QUIT with message
    let quit = Message::new(Command::Quit, vec!["Goodbye!".to_owned()]);
    client.send(quit).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.command, Command::Error);
    let trailing = reply.trailing().unwrap();
    assert!(trailing.contains("Closing Link"));
    assert!(trailing.contains("Goodbye!"));

    // Give server time to clean up
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(registry.connection_count(), 0);

    shutdown.shutdown();
}

#[tokio::test]
async fn quit_without_message_over_tcp() {
    let (addr, shutdown, registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Bob", "bob").await;

    let quit = Message::new(Command::Quit, vec![]);
    client.send(quit).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.command, Command::Error);
    let trailing = reply.trailing().unwrap();
    assert!(trailing.contains("Client Quit"));

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(registry.connection_count(), 0);

    shutdown.shutdown();
}

// ---- Connection drop tests ----

#[tokio::test]
async fn connection_drop_removes_user_from_registry() {
    let (addr, shutdown, registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Carol", "carol").await;

    assert_eq!(registry.connection_count(), 1);

    // Drop connection abruptly (without QUIT)
    client.shutdown().await.ok();
    drop(client);

    // Give server time to detect disconnect and clean up
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(registry.connection_count(), 0);

    shutdown.shutdown();
}

// ---- PING/PONG integration tests ----

#[tokio::test]
async fn client_ping_receives_pong_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Dave", "dave").await;

    // Send PING with token
    let ping = Message::new(Command::Ping, vec!["mytoken".to_owned()]);
    client.send(ping).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "mytoken");

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn client_ping_pre_registration_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;

    // Send PING before registering
    let ping = Message::new(Command::Ping, vec!["earlyping".to_owned()]);
    client.send(ping).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "earlyping");

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn pong_from_client_is_accepted_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Eve", "eve").await;

    // Send a PONG (simulating response to server PING)
    let pong = Message::new(Command::Pong, vec!["pircd".to_owned()]);
    client.send(pong).await.unwrap();

    // No error or disconnect should occur; send another command to verify connection alive.
    let ping = Message::new(Command::Ping, vec!["alive".to_owned()]);
    client.send(ping).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.command, Command::Pong);
    assert_eq!(reply.params[0], "alive");

    client.shutdown().await.ok();
    shutdown.shutdown();
}
