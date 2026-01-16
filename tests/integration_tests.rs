//! Integration tests for moo-transport

use bytes::Bytes;
use futures::StreamExt;
use moo_transport::protocol::{MooMessageBuilder, MooParser};
use moo_transport::{MooConnection, MooError, MooRequest, MooVerb, Result};
use tokio::net::{TcpListener, TcpStream};

/// Test helper to create a pair of connected TCP sockets
async fn create_tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });

    let (server, _) = listener.accept().await.unwrap();
    let client = client_task.await.unwrap();

    (client, server)
}

#[tokio::test]
async fn test_parser_with_valid_messages() {
    let mut parser = MooParser::new();

    // Simple REQUEST without body
    let data = b"MOO/1 REQUEST com.example/test\nRequest-Id: 123\n\n";
    let msg = parser.parse(data).unwrap().unwrap();
    assert_eq!(msg.verb, MooVerb::Request);
    assert_eq!(msg.name, "com.example/test");
    assert_eq!(msg.request_id, "123");
    assert!(msg.body.is_none());

    // CONTINUE with JSON body
    let mut parser = MooParser::new();
    let data = b"MOO/1 CONTINUE Progress\nRequest-Id: 456\nContent-Type: application/json\nContent-Length: 13\n\n{\"foo\":\"bar\"}";
    let msg = parser.parse(data).unwrap().unwrap();
    assert_eq!(msg.verb, MooVerb::Continue);
    assert_eq!(msg.name, "Progress");
    assert!(msg.body.is_some());

    // COMPLETE with binary body
    let mut parser = MooParser::new();
    let binary_data = b"binary content";
    let mut data = Vec::new();
    data.extend_from_slice(b"MOO/1 COMPLETE Success\nRequest-Id: 789\nContent-Type: application/octet-stream\nContent-Length: 14\n\n");
    data.extend_from_slice(binary_data);
    let msg = parser.parse(&data).unwrap().unwrap();
    assert_eq!(msg.verb, MooVerb::Complete);
    assert!(msg.body.is_some());
}

#[tokio::test]
async fn test_parser_with_invalid_messages() {
    // Missing Request-Id
    let mut parser = MooParser::new();
    let data = b"MOO/1 REQUEST com.example/test\n\n";
    let result = parser.parse(data);
    assert!(result.is_err());

    // Invalid protocol version
    let mut parser = MooParser::new();
    let data = b"MOO/2 REQUEST com.example/test\nRequest-Id: 123\n\n";
    let result = parser.parse(data);
    assert!(matches!(result, Err(MooError::VersionMismatch(_))));

    // Invalid verb
    let mut parser = MooParser::new();
    let data = b"MOO/1 INVALID com.example/test\nRequest-Id: 123\n\n";
    let result = parser.parse(data);
    assert!(matches!(result, Err(MooError::InvalidVerb(_))));

    // Content-Type without Content-Length
    let mut parser = MooParser::new();
    let data = b"MOO/1 REQUEST com.example/test\nRequest-Id: 123\nContent-Type: application/json\n\n";
    let result = parser.parse(data);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_message_builder() {
    // Build REQUEST with JSON body
    let builder = MooMessageBuilder::request("123".to_string(), "com.example/test".to_string())
        .body_json(serde_json::json!({"key": "value"}));
    let bytes = builder.build().unwrap();
    let output = String::from_utf8_lossy(&bytes);

    assert!(output.contains("MOO/1 REQUEST com.example/test"));
    assert!(output.contains("Request-Id: 123"));
    assert!(output.contains("Content-Type: application/json"));
    assert!(output.contains("Content-Length:"));

    // Build COMPLETE with binary body
    let binary_data = Bytes::from_static(b"test data");
    let builder = MooMessageBuilder::complete("456".to_string(), "Success".to_string())
        .body_binary(binary_data);
    let bytes = builder.build().unwrap();
    let output = String::from_utf8_lossy(&bytes[..50]); // Check headers only

    assert!(output.contains("MOO/1 COMPLETE Success"));
    assert!(output.contains("Request-Id: 456"));
}

#[tokio::test]
async fn test_parser_partial_messages() {
    let mut parser = MooParser::new();

    // Send first part
    let result = parser.parse(b"MOO/1 REQUEST com.example/test\n").unwrap();
    assert!(result.is_none()); // Incomplete

    // Send second part
    let result = parser.parse(b"Request-Id: 123\n\n").unwrap();
    assert!(result.is_some()); // Now complete

    let msg = result.unwrap();
    assert_eq!(msg.verb, MooVerb::Request);
    assert_eq!(msg.request_id, "123");
}

#[tokio::test]
async fn test_tcp_connection() -> Result<()> {
    let (client, server) = create_tcp_pair().await;

    // Create connection from client socket
    let conn = MooConnection::from_tcp(client)?;

    // Spawn a simple echo server
    tokio::spawn(async move {
        let mut server_conn = MooConnection::from_tcp(server).unwrap();

        server_conn
            .register_request_handler("test/echo", |req: MooRequest| async move {
                let body = req.message.body.clone();
                req.send_complete("Echo", body.map(|b| b.as_json().unwrap().clone()))
                    .await
            })
            .await;

        // Keep server alive
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Send a request
    let mut responses = conn
        .send_request("test/echo", Some(serde_json::json!({"test": "data"})))
        .await?;

    // Receive response
    if let Some(Ok(msg)) = responses.next().await {
        assert_eq!(msg.verb, MooVerb::Complete);
        assert_eq!(msg.name, "Echo");
    } else {
        panic!("Expected response");
    }

    Ok(())
}

#[tokio::test]
async fn test_streaming_responses() -> Result<()> {
    let (client, server) = create_tcp_pair().await;

    let conn = MooConnection::from_tcp(client)?;
    let server_conn = MooConnection::from_tcp(server)?;

    // Register handler on server
    server_conn
        .register_request_handler("test/stream", |req: MooRequest| async move {
            // Send multiple CONTINUE messages
            for i in 1..=3 {
                req.send_continue(
                    format!("Progress{}", i),  // No whitespace in name
                    Some(serde_json::json!({"step": i})),
                )
                .await?;
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }

            // Send final COMPLETE
            req.send_complete("Done", Some(serde_json::json!({"status": "success"})))
                .await
        })
        .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Send request and collect all responses
    let mut responses = conn.send_request("test/stream", None).await?;

    let mut messages = Vec::new();
    while let Some(result) = responses.next().await {
        match result {
            Ok(msg) => {
                println!("Received message: {:?} - {}", msg.verb, msg.name);
                messages.push(msg);
            }
            Err(e) => {
                println!("Error receiving message: {:?}", e);
                return Err(e);
            }
        }
    }

    // Should have 3 CONTINUE + 1 COMPLETE
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].verb, MooVerb::Continue);
    assert_eq!(messages[1].verb, MooVerb::Continue);
    assert_eq!(messages[2].verb, MooVerb::Continue);
    assert_eq!(messages[3].verb, MooVerb::Complete);

    // Keep server connection alive
    drop(server_conn);

    Ok(())
}

#[tokio::test]
async fn test_bidirectional_communication() -> Result<()> {
    let (client, server) = create_tcp_pair().await;

    let client_conn = MooConnection::from_tcp(client)?;
    let server_conn = MooConnection::from_tcp(server)?;

    // Register handlers on both sides
    client_conn
        .register_request_handler("client/ping", |req: MooRequest| async move {
            req.send_complete("ClientPong", Some(serde_json::json!({"from": "client"})))
                .await
        })
        .await;

    server_conn
        .register_request_handler("server/ping", |req: MooRequest| async move {
            req.send_complete("ServerPong", Some(serde_json::json!({"from": "server"})))
                .await
        })
        .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client pings server
    let mut responses = client_conn.send_request("server/ping", None).await?;
    if let Some(Ok(msg)) = responses.next().await {
        assert_eq!(msg.name, "ServerPong");
    }

    // Server pings client
    let mut responses = server_conn.send_request("client/ping", None).await?;
    if let Some(Ok(msg)) = responses.next().await {
        assert_eq!(msg.name, "ClientPong");
    }

    Ok(())
}

#[tokio::test]
async fn test_connection_lifecycle() -> Result<()> {
    let (client, server) = create_tcp_pair().await;

    let conn = MooConnection::from_tcp(client)?;

    assert!(conn.is_connected());

    // Close connection
    conn.close().await?;

    assert!(!conn.is_connected());

    // Attempting to send after close should fail
    let result = conn.send_request("test", None).await;
    assert!(result.is_err());

    drop(server);
    Ok(())
}
