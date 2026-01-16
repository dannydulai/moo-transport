//! Error types for the MOO protocol implementation.

use std::fmt;

/// Result type alias for MOO operations.
pub type Result<T> = std::result::Result<T, MooError>;

/// Errors that can occur when using the MOO protocol.
#[derive(Debug, thiserror::Error)]
pub enum MooError {
    /// IO error from the underlying transport.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Protocol violation detected.
    #[error("Protocol violation: {0}")]
    ProtocolViolation(String),

    /// Invalid or missing header.
    #[error("Invalid or missing header: {0}")]
    InvalidHeader(String),

    /// Missing required header.
    #[error("Missing required header: {0}")]
    MissingHeader(String),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Connection closed unexpectedly.
    #[error("Connection closed")]
    ConnectionClosed,

    /// Operation timed out.
    #[error("Operation timed out")]
    Timeout,

    /// Wrong body type (expected JSON but got binary, or vice versa).
    #[error("Wrong body type: {0}")]
    WrongBodyType(String),

    /// Protocol version mismatch.
    #[error("Protocol version mismatch: expected MOO/1, got {0}")]
    VersionMismatch(String),

    /// No response received for request.
    #[error("No response received for request {0}")]
    NoResponse(String),

    /// WebSocket error.
    #[cfg(feature = "websocket")]
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// Error from a request handler.
    #[error("Handler error: {0}")]
    HandlerError(String),

    /// Invalid message format.
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    /// Invalid verb in message.
    #[error("Invalid verb: {0}")]
    InvalidVerb(String),

    /// Invalid request ID.
    #[error("Invalid request ID: {0}")]
    InvalidRequestId(String),

    /// Content-Length header does not match actual content length.
    #[error("Content-Length mismatch: expected {expected}, got {actual}")]
    ContentLengthMismatch { expected: usize, actual: usize },

    /// Missing Content-Type header when body is present.
    #[error("Missing Content-Type header")]
    MissingContentType,

    /// Missing Content-Length header when body is present.
    #[error("Missing Content-Length header")]
    MissingContentLength,

    /// Invalid Content-Length value.
    #[error("Invalid Content-Length: {0}")]
    InvalidContentLength(String),

    /// UTF-8 encoding error in headers.
    #[error("UTF-8 encoding error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    /// Message too large.
    #[error("Message too large: {0} bytes")]
    MessageTooLarge(usize),
}

impl MooError {
    /// Create a protocol violation error.
    pub fn protocol_violation(msg: impl fmt::Display) -> Self {
        MooError::ProtocolViolation(msg.to_string())
    }

    /// Create an invalid header error.
    pub fn invalid_header(msg: impl fmt::Display) -> Self {
        MooError::InvalidHeader(msg.to_string())
    }

    /// Create a missing header error.
    pub fn missing_header(name: impl fmt::Display) -> Self {
        MooError::MissingHeader(name.to_string())
    }

    /// Create a wrong body type error.
    pub fn wrong_body_type(msg: impl fmt::Display) -> Self {
        MooError::WrongBodyType(msg.to_string())
    }

    /// Create a handler error.
    pub fn handler_error(msg: impl fmt::Display) -> Self {
        MooError::HandlerError(msg.to_string())
    }

    /// Create an invalid message error.
    pub fn invalid_message(msg: impl fmt::Display) -> Self {
        MooError::InvalidMessage(msg.to_string())
    }
}

#[cfg(feature = "websocket")]
impl From<tokio_tungstenite::tungstenite::Error> for MooError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        MooError::WebSocket(err.to_string())
    }
}
