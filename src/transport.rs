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
    ws: std::sync::Arc<tokio::sync::Mutex<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    >>,
}

#[cfg(feature = "websocket")]
impl WebSocketTransportSend {
    pub fn new(
        ws: std::sync::Arc<
            tokio::sync::Mutex<
                tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
            >,
        >,
    ) -> Self {
        Self { ws }
    }
}

#[cfg(feature = "websocket")]
#[async_trait]
impl TransportSend for WebSocketTransportSend {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let msg = Message::Binary(data.to_vec());
        let mut ws = self.ws.lock().await;
        ws.send(msg).await?;
        Ok(())
    }
}

/// WebSocket receive half.
#[cfg(feature = "websocket")]
pub(crate) struct WebSocketTransportRecv {
    ws: std::sync::Arc<tokio::sync::Mutex<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    >>,
}

#[cfg(feature = "websocket")]
impl WebSocketTransportRecv {
    pub fn new(
        ws: std::sync::Arc<
            tokio::sync::Mutex<
                tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
            >,
        >,
    ) -> Self {
        Self { ws }
    }
}

#[cfg(feature = "websocket")]
#[async_trait]
impl TransportRecv for WebSocketTransportRecv {
    async fn recv(&mut self) -> Result<Option<Vec<u8>>> {
        use futures::StreamExt;
        use tokio_tungstenite::tungstenite::Message;

        let mut ws = self.ws.lock().await;
        match ws.next().await {
            Some(Ok(Message::Binary(data))) => Ok(Some(data)),
            Some(Ok(Message::Close(_))) => Ok(None),
            Some(Ok(_)) => {
                // Ignore non-binary messages (text, ping, pong, etc.)
                drop(ws);
                self.recv().await
            }
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }
}

/// Connect to a WebSocket URL and return split transports.
#[cfg(feature = "websocket")]
pub(crate) async fn connect_websocket(
    url: impl AsRef<str>,
) -> Result<(WebSocketTransportSend, WebSocketTransportRecv)> {
    let (ws, _) = tokio_tungstenite::connect_async(url.as_ref()).await?;
    let ws = std::sync::Arc::new(tokio::sync::Mutex::new(ws));
    Ok((
        WebSocketTransportSend::new(ws.clone()),
        WebSocketTransportRecv::new(ws),
    ))
}
