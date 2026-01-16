//! Response stream for handling MOO responses.

use crate::error::{MooError, Result};
use crate::message::{MooMessage, MooVerb};
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

/// A stream of responses to a MOO request.
/// Yields CONTINUE messages followed by a final COMPLETE message.
pub struct ResponseStream {
    receiver: mpsc::UnboundedReceiver<Result<MooMessage>>,
    completed: bool,
}

impl ResponseStream {
    /// Create a new response stream.
    pub(crate) fn new(receiver: mpsc::UnboundedReceiver<Result<MooMessage>>) -> Self {
        Self {
            receiver,
            completed: false,
        }
    }
}

impl Stream for ResponseStream {
    type Item = Result<MooMessage>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // If already completed, return None
        if self.completed {
            return Poll::Ready(None);
        }

        // Poll the receiver
        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some(result)) => {
                // Check if this is a COMPLETE message
                match &result {
                    Ok(msg) if msg.verb == MooVerb::Complete => {
                        self.completed = true;
                    }
                    Err(_) => {
                        // On error, mark as completed to stop the stream
                        self.completed = true;
                    }
                    _ => {}
                }
                Poll::Ready(Some(result))
            }
            Poll::Ready(None) => {
                // Channel closed without COMPLETE
                self.completed = true;
                if !self.completed {
                    Poll::Ready(Some(Err(MooError::ConnectionClosed)))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_response_stream_with_continue_and_complete() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut stream = ResponseStream::new(rx);

        // Send CONTINUE messages
        tx.send(Ok(MooMessage::continue_msg("1".to_string(), "Progress".to_string()))).unwrap();
        tx.send(Ok(MooMessage::continue_msg("1".to_string(), "More Progress".to_string()))).unwrap();

        // Send COMPLETE message
        tx.send(Ok(MooMessage::complete("1".to_string(), "Success".to_string()))).unwrap();

        // Collect all messages
        let mut messages = Vec::new();
        while let Some(result) = stream.next().await {
            messages.push(result.unwrap());
        }

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].verb, MooVerb::Continue);
        assert_eq!(messages[1].verb, MooVerb::Continue);
        assert_eq!(messages[2].verb, MooVerb::Complete);
    }

    #[tokio::test]
    async fn test_response_stream_completes_after_complete() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut stream = ResponseStream::new(rx);

        // Send COMPLETE message
        tx.send(Ok(MooMessage::complete("1".to_string(), "Success".to_string()))).unwrap();

        // Try to send another message (shouldn't be received)
        tx.send(Ok(MooMessage::continue_msg("1".to_string(), "After Complete".to_string()))).unwrap();

        // Collect messages
        let mut messages = Vec::new();
        while let Some(result) = stream.next().await {
            messages.push(result.unwrap());
        }

        // Should only receive the COMPLETE message
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].verb, MooVerb::Complete);
    }

    #[tokio::test]
    async fn test_response_stream_handles_errors() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut stream = ResponseStream::new(rx);

        // Send an error
        tx.send(Err(MooError::ConnectionClosed)).unwrap();

        // Try to send another message (shouldn't be received)
        tx.send(Ok(MooMessage::continue_msg("1".to_string(), "After Error".to_string()))).unwrap();

        // Should receive the error and then complete
        let result = stream.next().await;
        assert!(result.is_some());
        assert!(result.unwrap().is_err());

        // Stream should be completed
        let result = stream.next().await;
        assert!(result.is_none());
    }
}
