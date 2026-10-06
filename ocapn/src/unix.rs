//! The `unix` netlayer — a Unix domain socket, and the operating system's own authentication.
//!
//! `tcp-testing-only` has no authentication and no encryption at all, which is why its own
//! upstream README says not to use it in production and why this crate's listener is off unless
//! configured. A Unix domain socket is the smallest transport that is *not* that: a peer is a process
//! whose uid and gid the socket's filesystem permissions admitted, so **the permission is the
//! authentication** and no key exchange has to be written to get one.
//!
//! That is the whole claim, and it is why [`UnixNetlayer::bind`] sets the mode rather than leaving it
//! to the operator's umask: a socket anyone on the machine can connect to is `tcp-testing-only` with
//! a longer path.
//!
//! **It is also the inner hop of a composition.** A gateway speaking this transport to a handful of
//! local agents, each of which speaks something wider outward, keeps the wide-area transport and its
//! credentials out of the chain-facing process — and gives each agent an identity the operator
//! started deliberately.
//!
//! Framing is the shared netstring [`crate::framed`]; nothing above the [`Netlayer`] trait knows
//! which transport it is on.

use std::io;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::net::{UnixListener, UnixStream};

use crate::framed::{Framed, CONNECT_TIMEOUT};
use crate::locator::PeerLocator;
use crate::netlayer::{NetConn, Netlayer};

/// The mode a bound socket gets: owner read/write only.
const SOCKET_MODE: u32 = 0o600;

/// A `unix` endpoint. The same value dials (as a client) and accepts (as a server); dialling needs no
/// listener of its own, so a purely outgoing peer still binds one.
pub struct UnixNetlayer {
    listener: UnixListener,
    path: PathBuf,
}

impl UnixNetlayer {
    /// Bind a listening socket at `path`, owner-only.
    ///
    /// A socket file left behind by a previous run would make `bind` fail, and a transport an
    /// operator cannot restart is not one they can run — so a stale **socket** is removed. Anything
    /// else at that path (a regular file, a symlink, a directory) is refused instead: this removes
    /// only what it is about to replace.
    pub async fn bind(path: impl AsRef<Path>) -> io::Result<UnixNetlayer> {
        let path = path.as_ref().to_path_buf();
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_socket() => {
                std::fs::remove_file(&path)?;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "{} exists and is not a socket — refusing to remove it",
                        path.display()
                    ),
                ));
            }
            Err(_) => {}
        }
        let listener = UnixListener::bind(&path)?;
        // **The mode is the security** (see the module doc). If it cannot be set, the socket is not
        // the transport this crate claims, so the bind fails rather than serving something weaker.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(SOCKET_MODE))?;
        Ok(UnixNetlayer { listener, path })
    }

    /// The bound path — what a peer puts in its `path` hint.
    pub fn local_path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl Netlayer for UnixNetlayer {
    async fn new_outgoing_connection(&self, locator: &PeerLocator) -> io::Result<Box<dyn NetConn>> {
        let path = locator.hints.get("path").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "unix needs a `path` hint, the socket file to connect to",
            )
        })?;
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(path))
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("connecting to {path} took longer than {CONNECT_TIMEOUT:?}"),
                )
            })??;
        Ok(Box::new(UnixConn {
            framed: Framed::new(stream),
        }))
    }

    async fn accept_incoming_connection(&self) -> io::Result<Box<dyn NetConn>> {
        let (stream, _peer) = self.listener.accept().await?;
        Ok(Box::new(UnixConn {
            framed: Framed::new(stream),
        }))
    }
}

/// A Unix-socket connection: the shared framing, over a socket that has no address.
struct UnixConn {
    framed: Framed<UnixStream>,
}

#[async_trait]
impl NetConn for UnixConn {
    async fn send(&mut self, message: &[u8]) -> io::Result<()> {
        self.framed.send_message(message).await
    }

    async fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.framed.next_message().await
    }

    // `peer_address` keeps its default `None`, and that is the right answer rather than a gap: a
    // Unix socket has no `SocketAddr`, the dial policy reads a `None` origin as "cannot be judged",
    // and "cannot be judged" is correct for a peer that is local by construction and was admitted by
    // the socket's permission. See `dial_policy`'s note.
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::syrup::Value;

    /// A unique socket path short enough for `sun_path` (108 bytes on Linux), cleaned up on drop.
    struct TempSocket(PathBuf);

    impl TempSocket {
        fn new(tag: &str) -> TempSocket {
            let path = std::env::temp_dir().join(format!(
                "rchain-ocapn-{}-{}.sock",
                tag,
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            TempSocket(path)
        }
    }

    impl Drop for TempSocket {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn locator(path: &Path) -> PeerLocator {
        PeerLocator {
            designator: "peer".into(),
            transport: "unix".into(),
            hints: [("path".to_string(), path.display().to_string())].into(),
        }
    }

    /// **The transport's whole claim**: the socket is owner-only. A socket any local process can
    /// connect to is `tcp-testing-only` under another name, and this is the assertion that says so.
    #[tokio::test]
    async fn a_bound_socket_is_owner_only() {
        let sock = TempSocket::new("mode");
        let layer = UnixNetlayer::bind(&sock.0).await.unwrap();
        assert_eq!(layer.local_path(), sock.0.as_path());

        let mode = std::fs::metadata(&sock.0).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            SOCKET_MODE,
            "the socket's permissions are the authentication; got {:o}",
            mode & 0o777
        );
    }

    /// Messages cross the socket and are framed the same way the TCP transport frames them — two
    /// writes coalescing into one read is the case the framing exists for.
    #[tokio::test]
    async fn messages_round_trip_and_are_framed() {
        let sock = TempSocket::new("round");
        let server = UnixNetlayer::bind(&sock.0).await.unwrap();
        let peer = locator(&sock.0);

        // A dialer needs a listener of its own only because the trait bears one, exactly as the TCP
        // transport's tests do.
        let dialer_sock = TempSocket::new("round-dialer");
        let dialer = UnixNetlayer::bind(&dialer_sock.0).await.unwrap();
        let sender = tokio::spawn(async move {
            let mut conn = dialer.new_outgoing_connection(&peer).await.unwrap();
            conn.send(&Value::Symbol("one".into()).to_bytes())
                .await
                .unwrap();
            conn.send(&Value::Symbol("two".into()).to_bytes())
                .await
                .unwrap();
        });

        let mut accepted = server.accept_incoming_connection().await.unwrap();
        let first = accepted.recv().await.unwrap().unwrap();
        let second = accepted.recv().await.unwrap().unwrap();
        assert_eq!(
            Value::decode_prefix(&first).unwrap().0,
            Value::Symbol("one".into())
        );
        assert_eq!(
            Value::decode_prefix(&second).unwrap().0,
            Value::Symbol("two".into())
        );
        sender.await.unwrap();
    }

    /// A peer that closes cleanly is a clean end of stream; one that closes inside a message is not.
    #[tokio::test]
    async fn a_closed_socket_is_a_clean_end_of_stream() {
        let sock = TempSocket::new("eof");
        let server = UnixNetlayer::bind(&sock.0).await.unwrap();
        let peer = locator(&sock.0);
        let dialer_sock = TempSocket::new("eof-dialer");
        let dialer = UnixNetlayer::bind(&dialer_sock.0).await.unwrap();
        let dialing = tokio::spawn(async move {
            let _conn = dialer.new_outgoing_connection(&peer).await.unwrap();
            // Dropped immediately: the accept side sees an empty stream.
        });
        let mut accepted = server.accept_incoming_connection().await.unwrap();
        assert_eq!(accepted.recv().await.unwrap(), None);
        dialing.await.unwrap();
    }

    /// **Only a socket is replaced.** A regular file at the path is refused rather than unlinked,
    /// because a transport that deletes whatever it finds where it wants to bind is a foot-gun.
    #[tokio::test]
    async fn a_path_that_is_not_a_socket_is_refused() {
        let path = std::env::temp_dir().join(format!(
            "rchain-ocapn-{}-notasocket.txt",
            std::process::id()
        ));
        std::fs::write(&path, b"do not delete me").unwrap();
        let err = match UnixNetlayer::bind(&path).await {
            Err(e) => e,
            Ok(_) => panic!("a regular file must not be replaced"),
        };
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err:?}");
        assert_eq!(std::fs::read(&path).unwrap(), b"do not delete me");
        let _ = std::fs::remove_file(&path);
    }
}
