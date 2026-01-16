//! Message types for the MOO protocol.

use crate::error::{MooError, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// MOO protocol verbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MooVerb {
    /// Initial request message.
    #[serde(rename = "REQUEST")]
    Request,
    /// Continuation message (streaming response).
    #[serde(rename = "CONTINUE")]
    Continue,
    /// Final response message.
    #[serde(rename = "COMPLETE")]
    Complete,
}

impl MooVerb {
    /// Convert verb to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            MooVerb::Request => "REQUEST",
            MooVerb::Continue => "CONTINUE",
            MooVerb::Complete => "COMPLETE",
        }
    }
}

impl FromStr for MooVerb {
    type Err = MooError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "REQUEST" => Ok(MooVerb::Request),
            "CONTINUE" => Ok(MooVerb::Continue),
            "COMPLETE" => Ok(MooVerb::Complete),
            _ => Err(MooError::InvalidVerb(s.to_string())),
        }
    }
}

impl fmt::Display for MooVerb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Body content of a MOO message.
#[derive(Debug, Clone)]
pub enum MooBody {
    /// JSON body.
    Json(serde_json::Value),
    /// Binary body.
    Binary(Bytes),
}

impl MooBody {
    /// Get the body as JSON, returning an error if it's binary.
    pub fn as_json(&self) -> Result<&serde_json::Value> {
        match self {
            MooBody::Json(v) => Ok(v),
            MooBody::Binary(_) => Err(MooError::wrong_body_type("expected JSON, got binary")),
        }
    }

    /// Get the body as binary, returning an error if it's JSON.
    pub fn as_binary(&self) -> Result<&Bytes> {
        match self {
            MooBody::Binary(b) => Ok(b),
            MooBody::Json(_) => Err(MooError::wrong_body_type("expected binary, got JSON")),
        }
    }

    /// Convert the body to a typed value, deserializing JSON if needed.
    pub fn to_typed<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        match self {
            MooBody::Json(v) => serde_json::from_value(v.clone()).map_err(Into::into),
            MooBody::Binary(_) => Err(MooError::wrong_body_type("cannot deserialize binary body to typed value")),
        }
    }

    /// Get the Content-Type for this body.
    pub fn content_type(&self) -> &'static str {
        match self {
            MooBody::Json(_) => "application/json",
            MooBody::Binary(_) => "application/octet-stream",
        }
    }

    /// Get the length of the body in bytes.
    pub fn len(&self) -> usize {
        match self {
            MooBody::Json(v) => serde_json::to_string(v).unwrap_or_default().len(),
            MooBody::Binary(b) => b.len(),
        }
    }

    /// Check if the body is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A MOO protocol message.
#[derive(Debug, Clone)]
pub struct MooMessage {
    /// The message verb (REQUEST, CONTINUE, or COMPLETE).
    pub verb: MooVerb,
    /// The request ID for matching responses to requests.
    pub request_id: String,
    /// The name/path of the request (e.g., "com.roon.app/ping").
    pub name: String,
    /// Additional headers.
    pub headers: HashMap<String, String>,
    /// Optional message body.
    pub body: Option<MooBody>,
}

impl MooMessage {
    /// Create a new REQUEST message.
    pub fn request(request_id: String, name: String) -> Self {
        Self {
            verb: MooVerb::Request,
            request_id,
            name,
            headers: HashMap::new(),
            body: None,
        }
    }

    /// Create a new CONTINUE message.
    pub fn continue_msg(request_id: String, name: String) -> Self {
        Self {
            verb: MooVerb::Continue,
            request_id,
            name,
            headers: HashMap::new(),
            body: None,
        }
    }

    /// Create a new COMPLETE message.
    pub fn complete(request_id: String, name: String) -> Self {
        Self {
            verb: MooVerb::Complete,
            request_id,
            name,
            headers: HashMap::new(),
            body: None,
        }
    }

    /// Get the body as JSON if present.
    pub fn body_json(&self) -> Option<Result<&serde_json::Value>> {
        self.body.as_ref().map(|b| b.as_json())
    }

    /// Get the body as binary if present.
    pub fn body_binary(&self) -> Option<Result<&Bytes>> {
        self.body.as_ref().map(|b| b.as_binary())
    }

    /// Parse the name into service and method components.
    /// For example, "com.roon.app/ping" returns ("com.roon.app", "ping").
    pub fn service_method(&self) -> Option<(&str, &str)> {
        self.name.split_once('/')
    }

    /// Get the service portion of the name.
    pub fn service(&self) -> Option<&str> {
        self.service_method().map(|(s, _)| s)
    }

    /// Get the method portion of the name.
    pub fn method(&self) -> Option<&str> {
        self.service_method().map(|(_, m)| m)
    }

    /// Add a header to the message.
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Set the body to a JSON value.
    pub fn with_json_body(mut self, value: serde_json::Value) -> Self {
        self.body = Some(MooBody::Json(value));
        self
    }

    /// Set the body to a serializable value.
    pub fn with_body<T: Serialize>(mut self, value: &T) -> Result<Self> {
        let json = serde_json::to_value(value)?;
        self.body = Some(MooBody::Json(json));
        Ok(self)
    }

    /// Set the body to binary data.
    pub fn with_binary_body(mut self, data: Bytes) -> Self {
        self.body = Some(MooBody::Binary(data));
        self
    }

    /// Get a header value.
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(|s| s.as_str())
    }
}

/// A MOO request that has been received, with methods to send responses.
#[derive(Debug)]
pub struct MooRequest {
    /// The underlying message.
    pub message: MooMessage,
    /// Internal channel for sending responses.
    response_tx: tokio::sync::mpsc::UnboundedSender<MooMessage>,
}

impl MooRequest {
    /// Create a new MooRequest.
    pub(crate) fn new(
        message: MooMessage,
        response_tx: tokio::sync::mpsc::UnboundedSender<MooMessage>,
    ) -> Self {
        Self {
            message,
            response_tx,
        }
    }

    /// Send a CONTINUE response.
    pub async fn send_continue(
        &self,
        name: impl Into<String>,
        body: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut msg = MooMessage::continue_msg(self.message.request_id.clone(), name.into());
        if let Some(b) = body {
            msg = msg.with_json_body(b);
        }
        self.response_tx
            .send(msg)
            .map_err(|_| MooError::ConnectionClosed)?;
        Ok(())
    }

    /// Send a COMPLETE response.
    pub async fn send_complete(
        &self,
        name: impl Into<String>,
        body: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut msg = MooMessage::complete(self.message.request_id.clone(), name.into());
        if let Some(b) = body {
            msg = msg.with_json_body(b);
        }
        self.response_tx
            .send(msg)
            .map_err(|_| MooError::ConnectionClosed)?;
        Ok(())
    }

    /// Send a CONTINUE response with binary data.
    pub async fn send_continue_binary(
        &self,
        name: impl Into<String>,
        data: Bytes,
    ) -> Result<()> {
        let msg = MooMessage::continue_msg(self.message.request_id.clone(), name.into())
            .with_binary_body(data);
        self.response_tx
            .send(msg)
            .map_err(|_| MooError::ConnectionClosed)?;
        Ok(())
    }

    /// Send a COMPLETE response with binary data.
    pub async fn send_complete_binary(
        &self,
        name: impl Into<String>,
        data: Bytes,
    ) -> Result<()> {
        let msg = MooMessage::complete(self.message.request_id.clone(), name.into())
            .with_binary_body(data);
        self.response_tx
            .send(msg)
            .map_err(|_| MooError::ConnectionClosed)?;
        Ok(())
    }

    /// Get the service and method from the request name.
    pub fn service_method(&self) -> Option<(&str, &str)> {
        self.message.service_method()
    }
}
