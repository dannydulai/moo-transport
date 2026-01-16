//! High-level MOO connection handler.

use crate::error::{MooError, Result};
use crate::message::{MooMessage, MooRequest};
use crate::protocol::{MooMessageBuilder, MooParser};
use crate::response_stream::ResponseStream;
use crate::transport::{split_tcp, TransportRecv, TransportSend};
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};

#[cfg(feature = "websocket")]
use crate::transport::connect_websocket;

/// Type for request handlers.
type RequestHandler = Arc<dyn Fn(MooRequest) -> futures::future::BoxFuture<'static, Result<()>> + Send + Sync>;

/// Shared state for a MOO connection.
struct ConnectionState {
    /// Request tracking: maps request_id to response channel.
    pending_requests: RwLock<HashMap<String, mpsc::UnboundedSender<Result<MooMessage>>>>,
    /// Request ID counter.
    next_request_id: AtomicU64,
    /// Request handlers: maps service/method to handler function.
    request_handlers: RwLock<HashMap<String, RequestHandler>>,
    /// Default handler for unmatched requests.
    default_handler: RwLock<Option<RequestHandler>>,
    /// Connection status.
    connected: AtomicBool,
    /// Send channel for outgoing messages.
    send_tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl ConnectionState {
    fn new(send_tx: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self {
            pending_requests: RwLock::new(HashMap::new()),
            next_request_id: AtomicU64::new(1),
            request_handlers: RwLock::new(HashMap::new()),
            default_handler: RwLock::new(None),
            connected: AtomicBool::new(true),
            send_tx,
        }
    }

    fn next_request_id(&self) -> String {
        self.next_request_id.fetch_add(1, Ordering::SeqCst).to_string()
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
    }
}

/// High-level MOO connection API.
pub struct MooConnection {
    state: Arc<ConnectionState>,
    send_tx: mpsc::UnboundedSender<Vec<u8>>,
    _receive_task: tokio::task::JoinHandle<()>,
}

impl MooConnection {
    /// Create a new MOO connection over TCP.
    pub async fn new_tcp(addr: impl tokio::net::ToSocketAddrs) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Self::from_tcp(stream)
    }

    /// Create a MOO connection from an existing TcpStream.
    pub fn from_tcp(stream: TcpStream) -> Result<Self> {
        let (send_transport, recv_transport) = split_tcp(stream);
        Self::from_transports(send_transport, recv_transport)
    }

    /// Create a new MOO connection over WebSocket.
    #[cfg(feature = "websocket")]
    pub async fn new_websocket(url: impl AsRef<str>) -> Result<Self> {
        let (send_transport, recv_transport) = connect_websocket(url).await?;
        Self::from_transports(send_transport, recv_transport)
    }

    /// Internal constructor from split transports.
    fn from_transports<S, R>(mut send_transport: S, recv_transport: R) -> Result<Self>
    where
        S: TransportSend + 'static,
        R: TransportRecv + 'static,
    {
        // Create channels for sending
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        // Create state with send channel
        let state = Arc::new(ConnectionState::new(send_tx.clone()));

        // Spawn send task (no mutex needed!)
        let state_send = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(data) = send_rx.recv().await {
                if let Err(e) = send_transport.send(&data).await {
                    tracing::error!("Send error: {}", e);
                    state_send.set_connected(false);
                    break;
                }
            }
        });

        // Spawn receive task (no mutex needed!)
        let state_recv = Arc::clone(&state);
        let receive_task = tokio::spawn(async move {
            Self::receive_loop(recv_transport, state_recv).await;
        });

        Ok(Self {
            state,
            send_tx,
            _receive_task: receive_task,
        })
    }

    /// Background receive loop.
    async fn receive_loop<R>(mut recv_transport: R, state: Arc<ConnectionState>)
    where
        R: TransportRecv,
    {
        let mut parser = MooParser::new();

        loop {
            // Receive data from transport (no lock needed!)
            match recv_transport.recv().await {
                Ok(Some(data)) => {
                    // Parse messages
                    match parser.parse(&data) {
                        Ok(Some(msg)) => {
                            if let Err(e) = Self::handle_message(msg, &state).await {
                                tracing::error!("Error handling message: {}", e);
                            }

                            // Try to parse more messages from buffer
                            while let Ok(Some(msg)) = parser.parse(&[]) {
                                if let Err(e) = Self::handle_message(msg, &state).await {
                                    tracing::error!("Error handling message: {}", e);
                                }
                            }
                        }
                        Ok(None) => {
                            // Need more data
                        }
                        Err(e) => {
                            tracing::error!("Parse error: {}", e);
                            state.set_connected(false);
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // Connection closed
                    tracing::debug!("Connection closed");
                    state.set_connected(false);
                    break;
                }
                Err(e) => {
                    tracing::error!("Receive error: {}", e);
                    state.set_connected(false);
                    break;
                }
            }
        }

        // Clean up pending requests
        let mut pending = state.pending_requests.write().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(MooError::ConnectionClosed));
        }
    }

    /// Handle a received message.
    async fn handle_message(msg: MooMessage, state: &Arc<ConnectionState>) -> Result<()> {
        match msg.verb {
            crate::message::MooVerb::Request => {
                // Incoming request - dispatch to handler
                Self::dispatch_request(msg, state).await
            }
            crate::message::MooVerb::Continue | crate::message::MooVerb::Complete => {
                // Response to our request
                Self::route_response(msg, state).await
            }
        }
    }

    /// Route a response to the appropriate pending request.
    async fn route_response(msg: MooMessage, state: &Arc<ConnectionState>) -> Result<()> {
        let pending = state.pending_requests.read().await;
        if let Some(tx) = pending.get(&msg.request_id) {
            let _ = tx.send(Ok(msg));
        } else {
            tracing::warn!("Received response for unknown request ID: {}", msg.request_id);
        }
        Ok(())
    }

    /// Dispatch an incoming request to a handler.
    async fn dispatch_request(msg: MooMessage, state: &Arc<ConnectionState>) -> Result<()> {
        let (response_tx, mut response_rx) = mpsc::unbounded_channel();
        let req = MooRequest::new(msg.clone(), response_tx);

        // Find handler
        let handlers = state.request_handlers.read().await;
        let handler = handlers.get(&msg.name).cloned();
        drop(handlers);

        let handler = if handler.is_some() {
            handler
        } else {
            let default = state.default_handler.read().await;
            default.clone()
        };

        if let Some(handler) = handler {
            // Spawn a task to handle the request and send responses
            let send_tx = state.send_tx.clone();
            tokio::spawn(async move {
                // Spawn handler task
                let handler_task = tokio::spawn(async move {
                    if let Err(e) = handler(req).await {
                        tracing::error!("Handler error: {}", e);
                    }
                });

                // Spawn response forwarding task
                let forward_task = tokio::spawn(async move {
                    while let Some(msg) = response_rx.recv().await {
                        // Build the response message with the correct verb
                        let builder = match msg.verb {
                            crate::message::MooVerb::Request => MooMessageBuilder::request(msg.request_id.clone(), msg.name.clone()),
                            crate::message::MooVerb::Continue => MooMessageBuilder::continue_msg(msg.request_id.clone(), msg.name.clone()),
                            crate::message::MooVerb::Complete => MooMessageBuilder::complete(msg.request_id.clone(), msg.name.clone()),
                        };

                        let builder = if let Some(body) = msg.body {
                            match body {
                                crate::message::MooBody::Json(v) => builder.body_json(v),
                                crate::message::MooBody::Binary(b) => builder.body_binary(b),
                            }
                        } else {
                            builder
                        };

                        let bytes = match builder.build() {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                tracing::error!("Failed to build response message: {}", e);
                                continue;
                            }
                        };

                        if let Err(e) = send_tx.send(bytes) {
                            tracing::error!("Failed to send response: {}", e);
                            break;
                        }
                    }
                });

                // Wait for both tasks to complete
                let _ = tokio::join!(handler_task, forward_task);
            });
        } else {
            tracing::warn!("No handler for request: {}", msg.name);
        }

        Ok(())
    }

    /// Send a REQUEST and return a stream of responses.
    pub async fn send_request(
        &self,
        name: impl Into<String>,
        body: Option<serde_json::Value>,
    ) -> Result<ResponseStream> {
        if !self.state.is_connected() {
            return Err(MooError::ConnectionClosed);
        }

        let request_id = self.state.next_request_id();
        let name = name.into();

        // Build message
        let mut builder = MooMessageBuilder::request(request_id.clone(), name);
        if let Some(b) = body {
            builder = builder.body_json(b);
        }
        let data = builder.build()?;

        // Create response channel
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        {
            let mut pending = self.state.pending_requests.write().await;
            pending.insert(request_id.clone(), response_tx);
        }

        // Send request
        self.send_tx.send(data).map_err(|_| MooError::ConnectionClosed)?;

        // Create response stream
        let stream = ResponseStream::new(response_rx);

        // Set up cleanup on stream drop
        let state = Arc::clone(&self.state);
        let request_id_cleanup = request_id.clone();
        tokio::spawn(async move {
            // Wait a bit for the stream to be consumed
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let mut pending = state.pending_requests.write().await;
            pending.remove(&request_id_cleanup);
        });

        Ok(stream)
    }

    /// Send a REQUEST with binary body and return a stream of responses.
    pub async fn send_request_raw(
        &self,
        name: impl Into<String>,
        body: Bytes,
        content_type: impl Into<String>,
    ) -> Result<ResponseStream> {
        if !self.state.is_connected() {
            return Err(MooError::ConnectionClosed);
        }

        let request_id = self.state.next_request_id();
        let name = name.into();

        // Build message
        let builder = MooMessageBuilder::request(request_id.clone(), name)
            .header("Content-Type".to_string(), content_type.into())
            .body_binary(body);
        let data = builder.build()?;

        // Create response channel
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        {
            let mut pending = self.state.pending_requests.write().await;
            pending.insert(request_id.clone(), response_tx);
        }

        // Send request
        self.send_tx.send(data).map_err(|_| MooError::ConnectionClosed)?;

        // Create response stream
        Ok(ResponseStream::new(response_rx))
    }

    /// Register a handler for a specific service/method.
    pub async fn register_request_handler<F, Fut>(
        &self,
        service_method: impl Into<String>,
        handler: F,
    ) where
        F: Fn(MooRequest) -> Fut + Send + Sync + 'static,
        Fut: futures::future::Future<Output = Result<()>> + Send + 'static,
    {
        let handler = Arc::new(move |req: MooRequest| {
            let fut = handler(req);
            Box::pin(fut) as futures::future::BoxFuture<'static, Result<()>>
        });

        let mut handlers = self.state.request_handlers.write().await;
        handlers.insert(service_method.into(), handler);
    }

    /// Register a default handler for unmatched requests.
    pub async fn register_default_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(MooRequest) -> Fut + Send + Sync + 'static,
        Fut: futures::future::Future<Output = Result<()>> + Send + 'static,
    {
        let handler = Arc::new(move |req: MooRequest| {
            let fut = handler(req);
            Box::pin(fut) as futures::future::BoxFuture<'static, Result<()>>
        });

        let mut default = self.state.default_handler.write().await;
        *default = Some(handler);
    }

    /// Close the connection.
    pub async fn close(&self) -> Result<()> {
        self.state.set_connected(false);
        Ok(())
    }

    /// Check if the connection is still active.
    pub fn is_connected(&self) -> bool {
        self.state.is_connected()
    }
}

