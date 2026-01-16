# moo-transport

A Rust implementation of the MOO protocol, an HTTP-like bidirectional message protocol designed for reliable, ordered communication over TCP or WebSockets.

## Features

- **Bidirectional Communication**: Both parties can send requests and receive responses
- **Multiple Outstanding Requests**: Handle many concurrent requests/responses
- **Streaming Responses**: A single request can receive multiple CONTINUE messages before a final COMPLETE
- **TCP and WebSocket Support**: Connect over raw TCP or WebSocket transports
- **Async/Await**: Built on Tokio for high-performance async I/O
- **Type-Safe Message Handling**: Strong typing with Rust's type system
- **Request Handlers**: Register handlers for incoming requests with async closures

## Protocol Overview

The MOO protocol uses a text-based header section followed by an optional binary body, similar to HTTP:

```
MOO/1 <VERB> <NAME>
Request-Id: <id>
Header1: Value1
Content-Type: application/json
Content-Length: <bytes>

<body content>
```

**Verbs:**
- `REQUEST`: Initial request message
- `CONTINUE`: Streaming response (can be sent multiple times)
- `COMPLETE`: Final response message

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
moo-transport = "0.1"
```

Or add it via cargo:

```bash
cargo add moo-transport
```

## Quick Start

### Basic Request-Response

```rust
use moo_transport::{MooConnection, Result};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to a MOO server via TCP
    let conn = MooConnection::new_tcp("192.168.1.100:9100").await?;

    // Send a request and receive responses
    let mut responses = conn.send_request(
        "com.roon.app/ping",
        Some(serde_json::json!({"message": "hello"}))
    ).await?;

    // Process responses
    while let Some(result) = responses.next().await {
        match result {
            Ok(msg) => println!("Received: {}", msg.name),
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}
```

### WebSocket Connection

```rust
use moo_transport::{MooConnection, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Connect via WebSocket
    let conn = MooConnection::new_websocket("ws://192.168.1.100:9100/api").await?;

    // Use the connection as normal
    let mut responses = conn.send_request("com.example/test", None).await?;

    Ok(())
}
```

### Handling Incoming Requests

```rust
use moo_transport::{MooConnection, MooRequest, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let conn = MooConnection::new_tcp("192.168.1.100:9100").await?;

    // Register a handler for incoming requests
    conn.register_request_handler("com.myservice/ping", |req: MooRequest| async move {
        // Send a COMPLETE response
        req.send_complete("Success", Some(serde_json::json!({"pong": true}))).await
    }).await;

    // Keep connection alive
    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;

    Ok(())
}
```

### Streaming Responses

The MOO protocol supports streaming responses where a single REQUEST can receive multiple CONTINUE messages followed by a final COMPLETE message:

```rust
use moo_transport::{MooConnection, Result};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let conn = MooConnection::new_tcp("192.168.1.100:9100").await?;

    let mut responses = conn.send_request(
        "com.roon.app/subscribe_zones",
        None
    ).await?;

    // Receive streaming updates
    while let Some(result) = responses.next().await {
        match result {
            Ok(msg) => {
                println!("Update: {}", msg.name);
                if let Some(body_result) = msg.body_json() {
                    match body_result {
                        Ok(body) => println!("  Body: {:?}", body),
                        Err(e) => eprintln!("  Error parsing body: {}", e),
                    }
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}
```

## Features

The library supports the following cargo features:

- `websocket` (enabled by default): Enables WebSocket transport support via `tokio-tungstenite`

To disable WebSocket support:

```toml
[dependencies]
moo-transport = { version = "0.1", default-features = false }
```

## Protocol Specification

### Message Format

Every MOO message consists of:

1. **First Line**: `MOO/1 VERB NAME`
   - Protocol version (always `MOO/1`)
   - Verb (`REQUEST`, `CONTINUE`, or `COMPLETE`)
   - service/path (e.g., `com.roon.app/ping`)

2. **Required Headers**:
   - `Request-Id`: Unique identifier for matching responses to requests

3. **Optional Headers**:
   - `Content-Type`: MIME type of body (required if body present)
   - `Content-Length`: Size of body in bytes (required if body present)
   - Custom headers as needed

4. **Blank Line**: Separates headers from body

5. **Body** (optional): JSON or binary data, see `Content-Type` for format

### Message Flow

```
Client                          Server
  |                               |
  |--- REQUEST (Request-Id: 1) -->|
  |                               |
  |<-- CONTINUE (Request-Id: 1) --|  (optional, repeatable)
  |<-- CONTINUE (Request-Id: 1) --|
  |                               |
  |<-- COMPLETE (Request-Id: 1) --|
```

Both client and server can send REQUEST messages and handle incoming requests, making the protocol fully bidirectional.

## API Documentation

Full API documentation is available by running:

```bash
cargo doc --open
```

Key types:
- `MooConnection`: High-level connection manager
- `MooMessage`: Represents a complete MOO message
- `MooRequest`: Incoming request with methods to send responses
- `MooVerb`: Enum for REQUEST, CONTINUE, COMPLETE
- `ResponseStream`: Stream of responses for a request
- `MooParser`: Low-level parser for MOO protocol
- `MooMessageBuilder`: Builder for constructing MOO messages

## Error Handling

The library uses a custom `Result<T>` type alias with `MooError` for error handling:

```rust
use moo_transport::{MooConnection, MooError, Result};

match conn.send_request("test", None).await {
    Ok(mut stream) => { /* handle responses */ }
    Err(MooError::ConnectionClosed) => eprintln!("Connection closed"),
    Err(MooError::InvalidVerb(v)) => eprintln!("Invalid verb: {}", v),
    Err(e) => eprintln!("Other error: {}", e),
}
```

## Testing

Run the test suite:

```bash
cargo test
```

Run tests with logging:

```bash
RUST_LOG=debug cargo test
```

## Performance

The library is designed for high performance:
- Async I/O using Tokio for efficient concurrent operations
- Zero-copy parsing where possible using the `bytes` crate
- Minimal allocations in hot paths
- Lock-free send/receive operations for WebSocket transport

## License

MIT License - see LICENSE file for details

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.
