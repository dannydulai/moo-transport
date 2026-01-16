//! # moo-transport
//!
//! A Rust implementation of the MOO protocol, an HTTP-like bidirectional message protocol
//! designed for reliable, ordered communication over TCP or WebSockets.
//!
//! ## Overview
//!
//! The MOO protocol provides symmetrical message-based communication between two parties with
//! support for:
//! - Bidirectional communication (both parties can send requests)
//! - Multiple outstanding requests
//! - Streaming responses (a single request can receive multiple CONTINUE responses)
//! - Publish/subscribe patterns
//!
//! ## Quick Start
//!
//! ### Basic Request-Response
//!
//! ```rust,no_run
//! use moo_transport::{MooConnection, Result};
//! use futures::StreamExt;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Connect to a MOO server via TCP
//!     let conn = MooConnection::new_tcp("192.168.1.100:9100").await?;
//!
//!     // Send a request and receive responses
//!     let mut responses = conn.send_request(
//!         "com.roon.app/ping",
//!         Some(serde_json::json!({"message": "hello"}))
//!     ).await?;
//!
//!     // Process responses
//!     while let Some(result) = responses.next().await {
//!         match result {
//!             Ok(msg) => println!("Received: {}", msg.name),
//!             Err(e) => eprintln!("Error: {}", e),
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Bidirectional Communication
//!
//! ```rust,no_run
//! use moo_transport::{MooConnection, MooRequest, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let conn = MooConnection::new_tcp("192.168.1.100:9100").await?;
//!
//!     // Register a handler for incoming requests
//!     conn.register_request_handler("com.myservice/ping", |req: MooRequest| async move {
//!         // Send a COMPLETE response
//!         req.send_complete("Success", Some(serde_json::json!({"pong": true}))).await
//!     }).await;
//!
//!     // Keep connection alive
//!     tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Streaming Responses
//!
//! The MOO protocol supports streaming responses where a single REQUEST can receive
//! multiple CONTINUE messages followed by a final COMPLETE message:
//!
//! ```rust,no_run
//! use moo_transport::{MooConnection, Result};
//! use futures::StreamExt;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let conn = MooConnection::new_tcp("192.168.1.100:9100").await?;
//!
//!     let mut responses = conn.send_request(
//!         "com.roon.app/subscribe_zones",
//!         None
//!     ).await?;
//!
//!     // Receive streaming updates
//!     while let Some(result) = responses.next().await {
//!         match result {
//!             Ok(msg) => {
//!                 println!("Update: {}", msg.name);
//!                 if let Some(body) = msg.body_json() {
//!                     println!("  Body: {:?}", body);
//!                 }
//!             }
//!             Err(e) => eprintln!("Error: {}", e),
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## WebSocket Support
//!
//! WebSocket support is enabled by default via the `websocket` feature:
//!
//! ```rust,no_run
//! use moo_transport::{MooConnection, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let conn = MooConnection::new_websocket("ws://192.168.1.100:9100").await?;
//!     // Use the connection as normal
//!     Ok(())
//! }
//! ```
//!
//! ## Low-Level API
//!
//! For advanced use cases, you can use the low-level parser and message builder directly:
//!
//! ```rust
//! use moo_transport::protocol::{MooParser, MooMessageBuilder};
//!
//! let mut parser = MooParser::new();
//! let data = b"MOO/1 REQUEST com.example/test\nRequest-Id: 123\n\n";
//!
//! match parser.parse(data) {
//!     Ok(Some(msg)) => println!("Parsed message: {:?}", msg),
//!     Ok(None) => println!("Need more data"),
//!     Err(e) => eprintln!("Parse error: {}", e),
//! }
//!
//! // Build a message
//! let builder = MooMessageBuilder::request("456".to_string(), "com.example/test".to_string())
//!     .body_json(serde_json::json!({"foo": "bar"}));
//! let bytes = builder.build().unwrap();
//! ```
//!
//! ## Features
//!
//! - `websocket` (default): Enables WebSocket transport support
//!
//! ## Protocol Specification
//!
//! The MOO protocol uses a text-based header section followed by an optional binary body:
//!
//! ```text
//! MOO/1 <VERB> <NAME>
//! Request-Id: <id>
//! Header1: Value1
//! Content-Type: application/json
//! Content-Length: <bytes>
//!
//! <body content>
//! ```
//!
//! **Verbs:**
//! - `REQUEST`: Initial request
//! - `CONTINUE`: Streaming response (can be sent multiple times)
//! - `COMPLETE`: Final response
//!
//! **Required Headers:**
//! - `Request-Id`: Unique identifier for matching responses to requests
//!
//! **Optional Headers:**
//! - `Content-Type`: MIME type of the body (required if body is present)
//! - `Content-Length`: Size of the body in bytes (required if body is present)

pub mod connection;
pub mod error;
pub mod message;
pub mod protocol;
pub mod response_stream;
pub(crate) mod transport;

// Re-export main types for convenience
pub use connection::MooConnection;
pub use error::{MooError, Result};
pub use message::{MooBody, MooMessage, MooRequest, MooVerb};
pub use protocol::{MooMessageBuilder, MooParser};
pub use response_stream::ResponseStream;
