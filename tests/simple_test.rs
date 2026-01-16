//! Simple test to debug message flow

use futures::StreamExt;
use moo_transport::{MooConnection, MooRequest};
use tokio::net::{TcpListener, TcpStream};

async fn create_tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });

    let (server, _) = listener.accept().await.unwrap();
    let client = client_task.await.unwrap();

    (client, server)
}

#[tokio::test]
async fn test_simple_echo() {
    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    println!("Creating TCP pair...");
    let (client, server) = create_tcp_pair().await;

    println!("Creating client connection...");
    let client_conn = MooConnection::from_tcp(client).unwrap();

    println!("Creating server connection...");
    let server_conn = MooConnection::from_tcp(server).unwrap();

    println!("Registering handler...");
    server_conn
        .register_request_handler("test/echo", |req: MooRequest| async move {
            println!("Handler received request!");
            req.send_complete("Echo", Some(serde_json::json!({"status": "ok"})))
                .await
        })
        .await;

    println!("Waiting for server to be ready...");
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    println!("Sending request...");
    let mut responses = client_conn
        .send_request("test/echo", Some(serde_json::json!({"test": "data"})))
        .await
        .unwrap();

    println!("Request sent, stream created");
    println!("Client connected: {}", client_conn.is_connected());
    println!("Server connected: {}", server_conn.is_connected());
    println!("Waiting for response...");
    match tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        responses.next()
    ).await {
        Ok(Some(Ok(msg))) => {
            println!("Received response: {:?}", msg.name);
            assert_eq!(msg.name, "Echo");
        }
        Ok(Some(Err(e))) => {
            panic!("Error receiving response: {}", e);
        }
        Ok(None) => {
            panic!("Stream ended without response");
        }
        Err(_) => {
            panic!("Timeout waiting for response");
        }
    }

    println!("Test completed successfully!");
}
