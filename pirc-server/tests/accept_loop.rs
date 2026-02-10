//! Integration tests for the pircd accept loop and connection handling.

use std::net::SocketAddr;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Listener, ShutdownSignal};
use pirc_protocol::{Command, Message};
use tokio::net::TcpStream;

/// Start a server accept loop on a random port, returning the local address
/// and a shutdown controller to stop it.
async fn start_test_server() -> (SocketAddr, pirc_network::ShutdownController) {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = Listener::bind(addr).await.unwrap();
    let local_addr = listener.local_addr().unwrap();

    let (shutdown_controller, mut shutdown_signal) = ShutdownSignal::new();

    tokio::spawn(async move {
        loop {
            match listener.accept_with_shutdown(&mut shutdown_signal).await {
                Ok(Some((connection, peer_addr))) => {
                    let conn_shutdown = shutdown_signal.clone();
                    tokio::spawn(async move {
                        handle_connection(connection, peer_addr, conn_shutdown).await;
                    });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });

    (local_addr, shutdown_controller)
}

/// Mirrors the handle_connection from main.rs — echoes messages back.
async fn handle_connection(
    mut connection: Connection,
    _peer_addr: SocketAddr,
    mut shutdown: ShutdownSignal,
) {
    loop {
        match connection.recv_with_shutdown(&mut shutdown).await {
            Ok(Some(msg)) => {
                if connection.send(msg).await.is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
}

#[tokio::test]
async fn server_accepts_connection_and_echoes_ping() {
    let (server_addr, shutdown) = start_test_server().await;

    // Connect a TCP client
    let stream = TcpStream::connect(server_addr).await.unwrap();
    let mut client = Connection::new(stream).unwrap();

    // Send a PING
    let ping = Message::new(Command::Ping, vec!["test-server".to_owned()]);
    client.send(ping.clone()).await.unwrap();

    // Receive the echoed PING back
    let response = tokio::time::timeout(Duration::from_secs(2), client.recv())
        .await
        .expect("timeout waiting for response")
        .expect("recv error")
        .expect("unexpected EOF");

    assert_eq!(response, ping);

    // Clean up
    client.shutdown().await.ok();
    shutdown.shutdown();
}

#[tokio::test]
async fn server_handles_multiple_concurrent_connections() {
    let (server_addr, shutdown) = start_test_server().await;

    let mut handles = Vec::new();

    for i in 0..5 {
        let addr = server_addr;
        handles.push(tokio::spawn(async move {
            let stream = TcpStream::connect(addr).await.unwrap();
            let mut client = Connection::new(stream).unwrap();

            let msg = Message::new(Command::Ping, vec![format!("client-{i}")]);
            client.send(msg.clone()).await.unwrap();

            let response = tokio::time::timeout(Duration::from_secs(2), client.recv())
                .await
                .expect("timeout")
                .expect("recv error")
                .expect("unexpected EOF");

            assert_eq!(response, msg);
            client.shutdown().await.ok();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    shutdown.shutdown();
}

#[tokio::test]
async fn server_shuts_down_gracefully() {
    let (server_addr, shutdown) = start_test_server().await;

    // Connect a client
    let stream = TcpStream::connect(server_addr).await.unwrap();
    let mut client = Connection::new(stream).unwrap();

    // Verify it works
    let ping = Message::new(Command::Ping, vec!["alive".to_owned()]);
    client.send(ping.clone()).await.unwrap();

    let response = tokio::time::timeout(Duration::from_secs(2), client.recv())
        .await
        .expect("timeout")
        .expect("recv error")
        .expect("unexpected EOF");
    assert_eq!(response, ping);

    // Signal shutdown
    shutdown.shutdown();

    // The client should eventually see the connection close (EOF)
    let result = tokio::time::timeout(Duration::from_secs(2), client.recv())
        .await
        .expect("timeout waiting for shutdown");

    // Either EOF (None) or an error is acceptable after shutdown
    match result {
        Ok(None) => {} // Expected EOF
        Err(_) => {}   // Connection error is also acceptable
        Ok(Some(_)) => panic!("did not expect a message after shutdown"),
    }
}

#[tokio::test]
async fn server_echoes_multiple_messages_on_same_connection() {
    let (server_addr, shutdown) = start_test_server().await;

    let stream = TcpStream::connect(server_addr).await.unwrap();
    let mut client = Connection::new(stream).unwrap();

    let messages = vec![
        Message::new(Command::Ping, vec!["first".to_owned()]),
        Message::new(Command::Nick, vec!["testuser".to_owned()]),
        Message::new(Command::Pong, vec!["check".to_owned()]),
    ];

    for msg in &messages {
        client.send(msg.clone()).await.unwrap();

        let response = tokio::time::timeout(Duration::from_secs(2), client.recv())
            .await
            .expect("timeout")
            .expect("recv error")
            .expect("unexpected EOF");

        assert_eq!(&response, msg);
    }

    client.shutdown().await.ok();
    shutdown.shutdown();
}
