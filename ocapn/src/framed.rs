//! The netstring framing two netlayers share.
//!
//! `tcp-testing-only` and `unix` differ only in the socket underneath them; what a message *is* — one
//! netstring, read in chunks, capped — is the same on both. It lives here rather than twice, because
//! two copies of a framing rule are two chances for it to drift, and a framing disagreement is
//! invisible until a peer sends something one of them mishandles.
//!
//! **A third transport with different framing writes its own.** A Noise transport message caps at
//! 65535 bytes, well under the `CapTP` messages this codebase produces, so a Noise netlayer has to
//! chunk — that is its own module's business, not a parameter of this one.
//!
//! The two bounds are the same two the TCP transport carried: a message-size cap, so a peer that
//! opens a value and never closes it cannot grow our memory (`crate::syrup`'s nesting bound is the
//! other), and a fixed read size.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::netstring;

/// A single message larger than this is refused rather than buffered. The node's other ingress paths
/// keep the same kind of bound; a netlayer that buffers without one is a remote DoS.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// How much is read at a time while waiting for a message to complete.
pub const READ_CHUNK: usize = 8 * 1024;

/// How long a dial may take before it is abandoned.
///
/// **The peer chooses the address.** A sturdyref's locator and a handoff give's `exporter-location`
/// both arrive over the wire (`enliven.rs`, `fixtures.rs`), so the connect here is an attempt to
/// wherever a stranger said — a port probe into the node's network position, and without a bound one
/// that hangs holds a session's loop open for the kernel's own (~130 s) or for ever. Bounded, it is
/// still a probe; the bound is what keeps it from also being a stall.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A stream, plus the bytes read but not yet consumed by a completed message.
///
/// `S` is any byte stream — a `TcpStream`, a `UnixStream`, or whatever a later transport carries —
/// so the framing is written once and a netlayer supplies only the socket.
pub struct Framed<S> {
    stream: S,
    buf: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Framed<S> {
    pub fn new(stream: S) -> Framed<S> {
        Framed {
            stream,
            buf: Vec::new(),
        }
    }

    /// Write one message, framed as a netstring (`CapTPSocket.send_message`'s boundary).
    pub async fn send_message(&mut self, message: &[u8]) -> io::Result<()> {
        self.stream.write_all(&netstring::encode(message)).await?;
        self.stream.flush().await
    }

    /// Read until the buffer holds one complete netstring and return its payload — one Syrup message.
    /// `Ok(None)` at a clean end of stream; `UnexpectedEof` if the stream ends inside a message,
    /// which is never a silently dropped message.
    pub async fn next_message(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            match netstring::decode_prefix(&self.buf) {
                Ok(Some((payload, used))) => {
                    self.buf.drain(..used);
                    return Ok(Some(payload));
                }
                // Not enough bytes yet; read more.
                Ok(None) => {}
                Err(other) => return Err(io::Error::new(io::ErrorKind::InvalidData, other)),
            }
            if self.buf.len() > MAX_MESSAGE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "netstring message exceeds the netlayer's bound",
                ));
            }
            let mut chunk = [0u8; READ_CHUNK];
            let n = self.stream.read(&mut chunk).await?;
            if n == 0 {
                return if self.buf.is_empty() {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "stream ended inside a netstring message",
                    ))
                };
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// The stream itself, for a netlayer that has to ask the socket something — the TCP transport
    /// reads its peer's address from here, which is what lets the dial policy tell a remote peer from
    /// a local one (Law 62).
    pub fn get_ref(&self) -> &S {
        &self.stream
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The framing is exercised end to end by the transports that use it; what is worth pinning here
    /// is the **bound**, because it is the one thing a peer controls and no transport's own test
    /// sends a 4 MiB message to find.
    ///
    /// **The bound is on what is buffered, not on what is declared.** A peer that declares an
    /// enormous length and then sends nothing gets a clean error at end of stream (below), and one
    /// that declares it and *keeps sending* is refused once the buffer passes the cap — which is the
    /// case that matters, because it is the one that grows memory.
    #[tokio::test]
    async fn a_message_past_the_bound_is_refused_rather_than_buffered() {
        // Declared *twice* the cap and sent one-and-a-quarter caps' worth: the message is still
        // incomplete when the buffer passes the bound, which is the case that grows memory.
        let declared = 2 * MAX_MESSAGE_BYTES;
        let sent = MAX_MESSAGE_BYTES + MAX_MESSAGE_BYTES / 4;
        let mut bytes = format!("{declared}:").into_bytes();
        bytes.resize(bytes.len() + sent, b'x');
        let mut framed = Framed::new(std::io::Cursor::new(bytes));
        let err = framed
            .next_message()
            .await
            .expect_err("a message past the bound must be refused, not buffered");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{err:?}");
    }

    /// And the peer that declares more than it sends does **not** grow anything: the stream ends and
    /// the reader says so, rather than waiting for a payload that will never arrive.
    #[tokio::test]
    async fn a_message_that_promises_more_than_it_sends_ends_cleanly() {
        let bytes = format!("{}:", MAX_MESSAGE_BYTES + 1).into_bytes();
        let mut framed = Framed::new(std::io::Cursor::new(bytes));
        let err = framed
            .next_message()
            .await
            .expect_err("a truncated message is not a clean end of stream");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof, "{err:?}");
    }
}
