//! Low-level MOO protocol parsing and message building.

use crate::error::{MooError, Result};
use crate::message::{MooBody, MooMessage, MooVerb};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::HashMap;
use std::str::FromStr;

/// Maximum size for a single MOO message (headers + body).
const MAX_MESSAGE_SIZE: usize = 100 * 1024 * 1024; // 100 MB

/// Stateful MOO protocol parser.
#[derive(Debug)]
pub struct MooParser {
    /// Internal buffer for partial messages.
    buffer: BytesMut,
}

impl MooParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
        }
    }

    /// Add data to the internal buffer and attempt to parse a complete message.
    /// Returns `Ok(Some(message))` if a complete message was parsed,
    /// `Ok(None)` if more data is needed, or `Err` if a protocol violation occurred.
    pub fn parse(&mut self, data: &[u8]) -> Result<Option<MooMessage>> {
        // Add new data to buffer
        self.buffer.put_slice(data);

        // Check if buffer is too large
        if self.buffer.len() > MAX_MESSAGE_SIZE {
            return Err(MooError::MessageTooLarge(self.buffer.len()));
        }

        // Try to parse a complete message
        self.try_parse()
    }

    /// Try to parse a complete message from the buffer.
    fn try_parse(&mut self) -> Result<Option<MooMessage>> {
        // Find the end of headers (blank line: \n\n)
        let header_end = match find_header_end(&self.buffer) {
            Some(pos) => pos,
            None => return Ok(None), // Need more data
        };

        // Parse the header section
        let header_bytes = &self.buffer[..header_end];
        let header_str = std::str::from_utf8(header_bytes)
            .map_err(|e| MooError::protocol_violation(format!("Invalid UTF-8 in headers: {}", e)))?;

        // Parse first line and headers
        let (verb, name, request_id, headers, content_length, content_type) =
            Self::parse_headers(header_str)?;

        // Check if we have the complete body
        let body_start = header_end + 2; // Skip both \n characters (\n\n)
        let total_size = body_start + content_length.unwrap_or(0);

        if self.buffer.len() < total_size {
            return Ok(None); // Need more data for body
        }

        // Parse body if present
        let body = if let Some(len) = content_length {
            if len > 0 {
                let content_type = content_type.ok_or_else(|| {
                    MooError::protocol_violation("Content-Length present but Content-Type missing")
                })?;

                let body_bytes = &self.buffer[body_start..body_start + len];

                if content_type == "application/json" {
                    let json_str = std::str::from_utf8(body_bytes)
                        .map_err(|e| MooError::protocol_violation(format!("Invalid UTF-8 in JSON body: {}", e)))?;
                    let json: serde_json::Value = serde_json::from_str(json_str)?;
                    Some(MooBody::Json(json))
                } else {
                    Some(MooBody::Binary(Bytes::copy_from_slice(body_bytes)))
                }
            } else {
                None
            }
        } else {
            None
        };

        // Construct message
        let message = MooMessage {
            verb,
            request_id,
            name,
            headers,
            body,
        };

        // Remove parsed data from buffer
        self.buffer.advance(total_size);

        Ok(Some(message))
    }

    /// Parse the header section of a MOO message.
    fn parse_headers(
        header_str: &str,
    ) -> Result<(
        MooVerb,
        String,
        String,
        HashMap<String, String>,
        Option<usize>,
        Option<String>,
    )> {
        let mut lines = header_str.lines();

        // Parse first line: MOO/1 VERB NAME
        let first_line = lines.next().ok_or_else(|| {
            MooError::protocol_violation("Empty message")
        })?;

        let (verb, name) = Self::parse_first_line(first_line)?;

        // Parse headers
        let mut request_id = None;
        let mut content_length = None;
        let mut content_type = None;
        let mut headers = HashMap::new();

        for line in lines {
            if line.is_empty() {
                break;
            }

            // Check for carriage return or non-printable characters
            if line.contains('\r') || line.chars().any(|c| c.is_control() && c != '\t') {
                return Err(MooError::protocol_violation(
                    "Header contains carriage return or non-printable character",
                ));
            }

            // Parse header: "Name: Value"
            let (key, value) = line.split_once(": ").ok_or_else(|| {
                MooError::invalid_header(format!("Invalid header format: {}", line))
            })?;

            // Validate header name
            if key.contains(' ') || key.contains(':') {
                return Err(MooError::invalid_header(format!(
                    "Header name contains invalid characters: {}",
                    key
                )));
            }

            // Handle special headers
            match key {
                "Request-Id" => request_id = Some(value.to_string()),
                "Content-Length" => {
                    content_length = Some(value.parse::<usize>().map_err(|_| {
                        MooError::InvalidContentLength(value.to_string())
                    })?);
                }
                "Content-Type" => content_type = Some(value.to_string()),
                _ => {
                    headers.insert(key.to_string(), value.to_string());
                }
            }
        }

        // Validate required headers
        let request_id = request_id.ok_or_else(|| MooError::missing_header("Request-Id"))?;

        // Validate Content-Length/Content-Type consistency
        match (content_length, &content_type) {
            (Some(_), None) => {
                return Err(MooError::protocol_violation(
                    "Content-Length present but Content-Type missing",
                ))
            }
            (None, Some(_)) => {
                return Err(MooError::protocol_violation(
                    "Content-Type present but Content-Length missing",
                ))
            }
            _ => {}
        }

        Ok((verb, name, request_id, headers, content_length, content_type))
    }

    /// Parse the first line of a MOO message.
    fn parse_first_line(line: &str) -> Result<(MooVerb, String)> {
        // Format: MOO/1 VERB NAME
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() != 3 {
            return Err(MooError::protocol_violation(format!(
                "Invalid first line: {}",
                line
            )));
        }

        // Check protocol version
        if parts[0] != "MOO/1" {
            return Err(MooError::VersionMismatch(parts[0].to_string()));
        }

        // Parse verb
        let verb = MooVerb::from_str(parts[1])?;

        // Parse name
        let name = parts[2].to_string();

        // Validate name doesn't contain whitespace
        if name.contains(char::is_whitespace) {
            return Err(MooError::protocol_violation(format!(
                "Name contains whitespace: {}",
                name
            )));
        }

        Ok((verb, name))
    }

    /// Get the current buffer size.
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Clear the internal buffer.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl Default for MooParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the position of the blank line that ends the header section.
/// Returns the position of the first \n in the \n\n sequence.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

/// Builder for constructing MOO protocol messages.
#[derive(Debug)]
pub struct MooMessageBuilder {
    message: MooMessage,
}

impl MooMessageBuilder {
    /// Create a REQUEST message builder.
    pub fn request(request_id: String, name: String) -> Self {
        Self {
            message: MooMessage::request(request_id, name),
        }
    }

    /// Create a CONTINUE message builder.
    pub fn continue_msg(request_id: String, name: String) -> Self {
        Self {
            message: MooMessage::continue_msg(request_id, name),
        }
    }

    /// Create a COMPLETE message builder.
    pub fn complete(request_id: String, name: String) -> Self {
        Self {
            message: MooMessage::complete(request_id, name),
        }
    }

    /// Add a header.
    pub fn header(mut self, key: String, value: String) -> Self {
        self.message.headers.insert(key, value);
        self
    }

    /// Set the body to a JSON value.
    pub fn body_json(mut self, value: serde_json::Value) -> Self {
        self.message.body = Some(MooBody::Json(value));
        self
    }

    /// Set the body to binary data.
    pub fn body_binary(mut self, data: Bytes) -> Self {
        self.message.body = Some(MooBody::Binary(data));
        self
    }

    /// Build the message into a byte vector ready to send.
    pub fn build(self) -> Result<Vec<u8>> {
        let mut output = Vec::new();

        // First line: MOO/1 VERB NAME
        output.extend_from_slice(b"MOO/1 ");
        output.extend_from_slice(self.message.verb.as_str().as_bytes());
        output.push(b' ');
        output.extend_from_slice(self.message.name.as_bytes());
        output.push(b'\n');

        // Request-Id header
        output.extend_from_slice(b"Request-Id: ");
        output.extend_from_slice(self.message.request_id.as_bytes());
        output.push(b'\n');

        // Additional headers
        for (key, value) in &self.message.headers {
            output.extend_from_slice(key.as_bytes());
            output.extend_from_slice(b": ");
            output.extend_from_slice(value.as_bytes());
            output.push(b'\n');
        }

        // Body headers and content
        if let Some(body) = &self.message.body {
            let body_bytes = match body {
                MooBody::Json(v) => serde_json::to_vec(v)?,
                MooBody::Binary(b) => b.to_vec(),
            };

            if !body_bytes.is_empty() {
                // Content-Type
                output.extend_from_slice(b"Content-Type: ");
                output.extend_from_slice(body.content_type().as_bytes());
                output.push(b'\n');

                // Content-Length
                output.extend_from_slice(b"Content-Length: ");
                output.extend_from_slice(body_bytes.len().to_string().as_bytes());
                output.push(b'\n');

                // Blank line
                output.push(b'\n');

                // Body content
                output.extend_from_slice(&body_bytes);

                return Ok(output);
            }
        }

        // Blank line (no body)
        output.push(b'\n');

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_request() {
        let mut parser = MooParser::new();
        let data = b"MOO/1 REQUEST com.example/test\nRequest-Id: 123\n\n";

        let result = parser.parse(data).unwrap();
        assert!(result.is_some());

        let msg = result.unwrap();
        assert_eq!(msg.verb, MooVerb::Request);
        assert_eq!(msg.name, "com.example/test");
        assert_eq!(msg.request_id, "123");
        assert!(msg.body.is_none());
    }

    #[test]
    fn test_parse_with_json_body() {
        let mut parser = MooParser::new();
        let data = b"MOO/1 COMPLETE Success\nRequest-Id: 456\nContent-Type: application/json\nContent-Length: 13\n\n{\"foo\":\"bar\"}";

        let result = parser.parse(data).unwrap();
        assert!(result.is_some());

        let msg = result.unwrap();
        assert_eq!(msg.verb, MooVerb::Complete);
        assert_eq!(msg.name, "Success");
        assert!(msg.body.is_some());
    }

    #[test]
    fn test_build_simple_request() {
        let builder = MooMessageBuilder::request("123".to_string(), "com.example/test".to_string());
        let bytes = builder.build().unwrap();
        let output = String::from_utf8(bytes).unwrap();

        assert!(output.starts_with("MOO/1 REQUEST com.example/test\n"));
        assert!(output.contains("Request-Id: 123\n"));
    }

    #[test]
    fn test_build_with_json_body() {
        let builder = MooMessageBuilder::complete("456".to_string(), "Success".to_string())
            .body_json(serde_json::json!({"foo": "bar"}));

        let bytes = builder.build().unwrap();
        let output = String::from_utf8(bytes).unwrap();

        assert!(output.contains("Content-Type: application/json\n"));
        assert!(output.contains("Content-Length:"));
    }
}
