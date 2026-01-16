//! Transport layer abstraction for MOO protocol.

use crate::error::Result;
use async_trait::async_trait;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Internal trait for sending data.
#[async_trait]
pub(crate) trait TransportSend: Send + Sync {
    /// Send a complete MOO message.
    async fn send(&mut self, data: &[u8]) -> Result<()>;
}

/// Internal trait for receiving data.
#[async_trait]
pub(crate) trait TransportRecv: Send + Sync {
    /// Receive data from the transport.
    /// Returns `None` if the connection is closed gracefully.
    async fn recv(&mut self) -> Result<Option<Vec<u8>>>;
}

/// TCP send half.
pub(crate) struct TcpTransportSend {
    write: tokio::net::tcp::OwnedWriteHalf,
}

impl TcpTransportSend {
    pub fn new(write: tokio::net::tcp::OwnedWriteHalf) -> Self {
        Self { write }
    }
}

#[async_trait]
impl TransportSend for TcpTransportSend {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        self.write.write_all(data).await?;
        self.write.flush().await?;
        Ok(())
    }
}

/// TCP receive half.
pub(crate) struct TcpTransportRecv {
    read: tokio::net::tcp::OwnedReadHalf,
}

impl TcpTransportRecv {
    pub fn new(read: tokio::net::tcp::OwnedReadHalf) -> Self {
        Self { read }
    }
}

#[async_trait]
impl TransportRecv for TcpTransportRecv {
    async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; 8192];

        match self.read.read(&mut buf).await {
            Ok(0) => Ok(None), // Connection closed
            Ok(n) => {
                buf.truncate(n);
                Ok(Some(buf))
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Split a TcpStream into send and receive halves.
pub(crate) fn split_tcp(stream: TcpStream) -> (TcpTransportSend, TcpTransportRecv) {
    let (read, write) = stream.into_split();
    (TcpTransportSend::new(write), TcpTransportRecv::new(read))
}


/// WebSocket send half.
#[cfg(feature = "websocket")]
pub(crate) struct WebSocketTransportSend {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

#[cfg(feature = "websocket")]
impl WebSocketTransportSend {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self { tx }
    }
}

#[cfg(feature = "websocket")]
#[async_trait]
impl TransportSend for WebSocketTransportSend {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        self.tx
            .send(data.to_vec())
            .map_err(|_| crate::error::MooError::ConnectionClosed)?;
        Ok(())
    }
}

/// WebSocket receive half.
#[cfg(feature = "websocket")]
pub(crate) struct WebSocketTransportRecv {
    rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

#[cfg(feature = "websocket")]
impl WebSocketTransportRecv {
    pub fn new(rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) -> Self {
        Self { rx }
    }
}

#[cfg(feature = "websocket")]
#[async_trait]
impl TransportRecv for WebSocketTransportRecv {
    async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        match self.rx.recv().await {
            Some(data) => Ok(Some(data)),
            None => Ok(None),
        }
    }
}

/// Connect to a WebSocket URL and return split transports.
#[cfg(feature = "websocket")]
pub(crate) async fn connect_websocket(
    url: impl AsRef<str>,
) -> Result<(WebSocketTransportSend, WebSocketTransportRecv)> {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let (ws, _) = tokio_tungstenite::connect_async(url.as_ref()).await?;
    let (mut ws_sink, mut ws_stream) = ws.split();

    // Create channels for send and receive
    let (send_tx, mut send_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (recv_tx, recv_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    // Spawn task to forward outgoing messages to WebSocket
    tokio::spawn(async move {
        while let Some(data) = send_rx.recv().await {
            let msg = Message::Binary(data);
            if let Err(e) = ws_sink.send(msg).await {
                tracing::error!("WebSocket send error: {}", e);
                break;
            }
        }
    });

    // Spawn task to forward incoming messages from WebSocket
    tokio::spawn(async move {
        while let Some(msg_result) = ws_stream.next().await {
            match msg_result {
                Ok(Message::Binary(data)) => {
                    if recv_tx.send(data).is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {
                    // Ignore other message types (text, ping, pong, etc.)
                }
                Err(e) => {
                    tracing::error!("WebSocket receive error: {}", e);
                    break;
                }
            }
        }
    });

    Ok((
        WebSocketTransportSend::new(send_tx),
        WebSocketTransportRecv::new(recv_rx),
    ))
}
