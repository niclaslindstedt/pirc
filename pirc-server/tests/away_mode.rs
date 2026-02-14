//! Integration tests for the AWAY and user MODE command handlers.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownSignal};
use pirc_protocol::numeric::{
    ERR_UMODEUNKNOWNFLAG, ERR_USERSDONTMATCH, RPL_NOWAWAY, RPL_UMODEIS, RPL_UNAWAY, RPL_WELCOME,
};
use pirc_protocol::{Command, Message};
use pirc_server::channel_registry::ChannelRegistry;
use pirc_server::config::ServerConfig;
use pirc_server::handler::{self, PreRegistrationState};
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
                handler::handle_message(
                    &msg, conn_id, &registry, &channels, &tx, &mut state, &config,
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

// ---- AWAY integration tests ----

#[tokio::test]
async fn away_set_and_clear_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    // Set away
    let away = Message::new(Command::Away, vec!["Gone fishing".to_owned()]);
    client.send(away).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_NOWAWAY));
    assert!(reply.trailing().unwrap().contains("marked as being away"));

    // Clear away
    let unaway = Message::new(Command::Away, vec![]);
    client.send(unaway).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_UNAWAY));
    assert!(reply
        .trailing()
        .unwrap()
        .contains("no longer marked as being away"));

    client.shutdown().await.ok();
    shutdown.shutdown();
}

// ---- MODE integration tests ----

#[tokio::test]
async fn mode_query_own_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    let mode = Message::new(Command::Mode, vec!["Alice".to_owned()]);
    client.send(mode).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_UMODEIS));
    assert_eq!(reply.params[1], "+");

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn mode_other_user_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client1 = connect(addr).await;
    register_and_drain(&mut client1, "Alice", "alice").await;

    let mut client2 = connect(addr).await;
    register_and_drain(&mut client2, "Bob", "bob").await;

    let mode = Message::new(Command::Mode, vec!["Bob".to_owned()]);
    client1.send(mode).await.unwrap();

    let reply = recv_msg(&mut client1).await;
    assert_eq!(reply.numeric_code(), Some(ERR_USERSDONTMATCH));

    client1.shutdown().await.ok();
    client2.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn mode_set_and_query_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    // Set +v
    let mode_set = Message::new(Command::Mode, vec!["Alice".to_owned(), "+v".to_owned()]);
    client.send(mode_set).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_UMODEIS));
    assert_eq!(reply.params[1], "+v");

    // Query to confirm
    let mode_query = Message::new(Command::Mode, vec!["Alice".to_owned()]);
    client.send(mode_query).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_UMODEIS));
    assert_eq!(reply.params[1], "+v");

    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn mode_unknown_flag_over_tcp() {
    let (addr, shutdown, _registry) = start_server().await;

    let mut client = connect(addr).await;
    register_and_drain(&mut client, "Alice", "alice").await;

    let mode = Message::new(Command::Mode, vec!["Alice".to_owned(), "+x".to_owned()]);
    client.send(mode).await.unwrap();

    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(ERR_UMODEUNKNOWNFLAG));

    // Should also get RPL_UMODEIS
    let reply = recv_msg(&mut client).await;
    assert_eq!(reply.numeric_code(), Some(RPL_UMODEIS));

    client.shutdown().await.ok();
    shutdown.shutdown();
}
