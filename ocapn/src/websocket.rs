//! The `websocket` netlayer — the transport `@endo/ocapn` speaks, and the one a *published* peer can
//! be pointed at.
//!
//! **Why this exists when [`crate::noise`] is stronger.** Noise encrypts and authenticates *both*
//! ends. This one, as the reference implementation writes it, does neither: Endo's netlayer is `ws://`
//! with **no TLS**, and its in-band handshake has the **server** prove its identity while the client
//! proves nothing. What it has instead is a peer that exists today — `@endo/ocapn` 1.1.1 is on npm and
//! already interoperates with this repository (`spec/audit/evidence/endo-spike/`) — so it is the
//! transport to test *against*, and the one to reach a deployment that speaks nothing else. `wss://`
//! is supported for an operator who wants the channel protected; that half's reference is Goblins
//! rather than Endo, since Endo has no TLS at all.
//!
//! **The framing is one WebSocket frame per CapTP message, with nothing inside it.** Endo's
//! `socketOps.write(bytes)` goes straight to `ws.send(bytes, { binary: true })` and its reader takes a
//! whole frame as a whole message: no length prefix and no netstring, because WebSocket already frames.
//! So [`crate::framed`] is *not* reused — its own doc says a transport with different framing writes
//! its own module. **The reference sets no size limit and no timeout anywhere**; the bound and the
//! deadline below are this port's additions, and are marked as such rather than presented as parity.
//!
//! **The in-band handshake, before any CapTP byte.** The client sends one `init:peer-auth` record
//! carrying 32 random bytes; the server signs **the bytes it received, verbatim**, with its designator
//! key, and replies with a `desc:sig-envelope` carrying that record and the signature; the client
//! checks the signature against the challenge it sent. Only then does CapTP start. The record wrapper
//! is what keeps the server from being a signing oracle — it signs a typed payload, not arbitrary
//! bytes — and this port mirrors it for that reason rather than for convenience.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use rchain_crypto::signatures::ed25519::Ed25519;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{accept_async, connect_async, WebSocketStream};

use crate::framed::CONNECT_TIMEOUT;
use crate::locator::PeerLocator;
use crate::netlayer::{NetConn, Netlayer};
use crate::noise::NoiseIdentity;
use crate::syrup::Value;

/// The one hint this transport reads: the whole URL, exactly as `@endo/ocapn` reads it
/// (`remoteLocation.hints.url`, used verbatim). Endo appends no path and no query, so neither do we.
const URL_HINT: &str = "url";

/// The largest WebSocket message this transport will take, matching [`crate::framed`]'s bound.
/// **The reference has no such limit** — `ws` defaults are permissive and its `pendingWrites` array
/// grows unbounded until authentication — so this is a thing the port adds.
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// The challenge length: `DESIGNATOR_CHALLENGE_PAYLOAD_BYTES = 32` in the reference.
const CHALLENGE_LEN: usize = 32;

/// The `init:peer-auth` record's label, and the envelope's, as the reference spells them.
const INIT_PEER_AUTH: &str = "init:peer-auth";
const SIG_ENVELOPE: &str = "desc:sig-envelope";

/// How long the in-band handshake may take. **The reference sets no handshake deadline**, and without
/// one a peer that opens a socket and never sends its challenge holds the connection for ever — which
/// on the node's side is a session slot.
const AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The byte stream under the WebSocket layer, as a trait object — the one place the two ways in
/// differ, since a plain listener yields `TcpStream` and a TLS one yields `TlsStream<TcpStream>`.
///
/// **The supertraits are named in a local trait because Rust allows only auto traits to be added to a
/// trait object**: `dyn AsyncRead + AsyncWrite` is not a type, but `dyn ByteStream` is, and `dyn
/// ByteStream: AsyncRead + AsyncWrite` follows from the bound.
trait ByteStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ByteStream for T {}
type BoxedIo = Box<dyn ByteStream>;

/// A `websocket` endpoint. The same value dials and accepts.
pub struct WebsocketNetlayer {
    listener: TcpListener,
    /// The node's identity. Only the Ed25519 half is used here — what a responder signs the challenge
    /// with, and what a dialler checks it against. See [`NoiseIdentity::sign`].
    identity: NoiseIdentity,
    /// `Some` accepts `wss://`; `None` accepts `ws://`, which is what Endo speaks and what the interop
    /// harness needs.
    tls: Option<Arc<tokio_rustls::rustls::ServerConfig>>,
}

impl WebsocketNetlayer {
    /// Bind a plain `ws://` listener. `addr` is `host:port`; `127.0.0.1:0` asks the OS for a free port.
    pub async fn bind(addr: &str, identity: NoiseIdentity) -> io::Result<WebsocketNetlayer> {
        Ok(WebsocketNetlayer {
            listener: TcpListener::bind(addr).await?,
            identity,
            tls: None,
        })
    }

    /// Bind a `wss://` listener. Inbound connections complete a TLS handshake before the WebSocket
    /// one; outbound ones still choose by the URL's scheme.
    pub async fn bind_tls(
        addr: &str,
        identity: NoiseIdentity,
        tls: Arc<tokio_rustls::rustls::ServerConfig>,
    ) -> io::Result<WebsocketNetlayer> {
        Ok(WebsocketNetlayer {
            listener: TcpListener::bind(addr).await?,
            identity,
            tls: Some(tls),
        })
    }

    /// The bound address, including the port chosen for `:0`.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// The location to advertise: the `url` hint Endo reads, in the scheme this listener speaks, plus
    /// this node's verifying key so a dialler can check the challenge response.
    pub fn location(&self, designator: &str) -> io::Result<PeerLocator> {
        let local = self.local_addr()?;
        let scheme = if self.tls.is_some() { "wss" } else { "ws" };
        Ok(PeerLocator {
            designator: designator.to_string(),
            transport: "websocket".to_string(),
            hints: std::collections::BTreeMap::from([
                (
                    URL_HINT.to_string(),
                    format!("{scheme}://{}:{}", local.ip(), local.port()),
                ),
                (
                    "verify".to_string(),
                    rchain_shared::base16::encode(&self.identity.verifying_key()),
                ),
            ]),
        })
    }
}

#[async_trait]
impl Netlayer for WebsocketNetlayer {
    async fn new_outgoing_connection(&self, locator: &PeerLocator) -> io::Result<Box<dyn NetConn>> {
        let url = locator.hints.get(URL_HINT).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "websocket needs a `url` hint, the whole ws:// or wss:// address",
            )
        })?;
        // **The peer's Ed25519 key, which the challenge/response checks its signature against.** A dial
        // that cannot name the peer cannot tell an impostor from it, so this is refused, not skipped.
        let peer = peer_key(locator)?;

        let (stream, _response) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(url))
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("connecting to {url} took longer than {CONNECT_TIMEOUT:?}"),
                )
            })?
            .map_err(io::Error::other)?;
        let peer_addr = match stream.get_ref() {
            tokio_tungstenite::MaybeTlsStream::Plain(tcp) => tcp.peer_addr().ok(),
            // A TLS stream's address is not exposed here; the dial policy reads an unknown origin as
            // "cannot be judged", which is the behaviour it had before this transport existed.
            _ => None,
        };
        let mut conn = WsConn::new(stream, peer_addr);
        authenticate_as_client(&mut conn, &peer).await?;
        Ok(Box::new(conn))
    }

    async fn accept_incoming_connection(&self) -> io::Result<Box<dyn NetConn>> {
        let (tcp, peer_addr) = self.listener.accept().await?;
        // The transport first, then the WebSocket layer, then the in-band handshake. The two arms
        // produce different stream types, so the byte stream is boxed *before* the WebSocket layer is
        // built — after which everything downstream is one type.
        let io: BoxedIo = match &self.tls {
            Some(config) => {
                let acceptor = tokio_rustls::TlsAcceptor::from(config.clone());
                Box::new(acceptor.accept(tcp).await?)
            }
            None => Box::new(tcp),
        };
        let stream = accept_async(io).await.map_err(io::Error::other)?;
        let mut conn = WsConn::new(stream, Some(peer_addr));
        authenticate_as_server(&mut conn, &self.identity).await?;
        Ok(Box::new(conn))
    }
}

/// The peer's Ed25519 verifying key, from the locator's `verify` hint (base16).
///
/// **The locator convention is unsettled**, exactly as it is for `noise`: Endo names a peer by a base32
/// public key in the location's `designator`, and this port reads a hint instead because the convention
/// is pinned by nothing reachable. Stated rather than implied.
fn peer_key(locator: &PeerLocator) -> io::Result<[u8; 32]> {
    let encoded = locator.hints.get("verify").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "websocket needs a `verify` hint: the peer's Ed25519 verifying key, base16",
        )
    })?;
    rchain_shared::base16::decode(encoded)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "websocket `verify` is not 32 bytes of base16",
            )
        })
}

/// The client's half: send the challenge, read the envelope, check the signature over our own bytes.
async fn authenticate_as_client<S>(conn: &mut WsConn<S>, peer: &[u8; 32]) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    use rand::Rng;
    let mut challenge = [0u8; CHALLENGE_LEN];
    rand::rng().fill_bytes(&mut challenge);
    let bytes = peer_auth(&challenge).to_bytes();

    conn.send_raw(Message::Binary(bytes.clone())).await?;
    let reply = conn.next_raw(AUTH_TIMEOUT).await?;
    let envelope = Value::from_bytes(&reply)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let signature = signature_of_envelope(&envelope)?;
    // **The signature covers the bytes the server received**, which are the bytes we sent — so this
    // checks our own challenge, not a re-encoding of it.
    if !Ed25519::verify_bytes(&bytes, &signature, peer) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the websocket peer did not prove it holds the key this dial named",
        ));
    }
    conn.authenticated = true;
    Ok(())
}

/// The server's half: read the challenge, sign **the bytes received**, answer with the envelope.
async fn authenticate_as_server<S>(conn: &mut WsConn<S>, identity: &NoiseIdentity) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let received = conn.next_raw(AUTH_TIMEOUT).await?;
    // Decoded as well as kept: the record wrapper is what keeps this from being a signing oracle, so a
    // frame that is not an `init:peer-auth` is refused rather than signed.
    let record = Value::from_bytes(&received)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    if !is_peer_auth(&record) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the first websocket message was not an `init:peer-auth`",
        ));
    }
    let signature = identity
        .sign(&received)
        .map_err(|e| io::Error::other(format!("signing the challenge: {e}")))?;
    let envelope = Value::Record(vec![
        Value::Symbol(SIG_ENVELOPE.to_string()),
        record,
        Value::Bytes(signature),
    ]);
    conn.send_raw(Message::Binary(envelope.to_bytes())).await?;
    conn.authenticated = true;
    Ok(())
}

/// `<init:peer-auth payload:bytes>` — the record the client opens with.
fn peer_auth(challenge: &[u8]) -> Value {
    Value::Record(vec![
        Value::Symbol(INIT_PEER_AUTH.to_string()),
        Value::Bytes(challenge.to_vec()),
    ])
}

/// Whether a value is an `init:peer-auth` record with one bytestring field.
fn is_peer_auth(value: &Value) -> bool {
    matches!(value,
        Value::Record(fields)
            if fields.first() == Some(&Value::Symbol(INIT_PEER_AUTH.to_string()))
                && matches!(fields.get(1), Some(Value::Bytes(_)))
    )
}

/// The signature out of a `desc:sig-envelope` record.
fn signature_of_envelope(value: &Value) -> io::Result<Vec<u8>> {
    match value {
        Value::Record(fields)
            if fields.first() == Some(&Value::Symbol(SIG_ENVELOPE.to_string())) =>
        {
            match fields.get(2) {
                Some(Value::Bytes(signature)) => Ok(signature.clone()),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the `desc:sig-envelope` carried no signature",
                )),
            }
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the websocket peer's answer was not a `desc:sig-envelope`",
        )),
    }
}

/// One WebSocket connection. Messages are whole frames: one CapTP message each, with nothing added.
struct WsConn<S> {
    stream: WebSocketStream<S>,
    /// Captured where the socket was made, so the dial policy can judge this peer (Law 62) — unlike
    /// `unix`'s deliberate `None`.
    peer: Option<SocketAddr>,
    /// Set once the in-band handshake has completed. An unauthenticated connection carries no CapTP
    /// traffic — the reference's rule, enforced here by **refusing** rather than by buffering (the
    /// reference's `pendingWrites` grows without bound until auth succeeds).
    authenticated: bool,
}

impl<S> WsConn<S> {
    fn new(stream: WebSocketStream<S>, peer: Option<SocketAddr>) -> WsConn<S> {
        WsConn {
            stream,
            peer,
            authenticated: false,
        }
    }
}

impl<S> WsConn<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    async fn send_raw(&mut self, message: Message) -> io::Result<()> {
        self.stream.send(message).await.map_err(io::Error::other)
    }

    /// The next whole message, bounded by `limit`. `next_raw` is the handshake's read, where a close
    /// before an answer is an error rather than an end of stream.
    async fn next_raw(&mut self, limit: std::time::Duration) -> io::Result<Vec<u8>> {
        let next = tokio::time::timeout(limit, self.stream.next())
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the websocket peer did not answer in time",
                )
            })?;
        match next {
            None => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the websocket peer closed before it answered",
            )),
            Some(Err(e)) => Err(io::Error::other(e)),
            // In this tungstenite `into_data` hands the payload back whole; there is no fallible form.
            Some(Ok(message)) => Ok(message.into_data()),
        }
    }
}

#[async_trait]
impl<S> NetConn for WsConn<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    async fn send(&mut self, message: &[u8]) -> io::Result<()> {
        if !self.authenticated {
            return Err(io::Error::other(
                "the websocket peer-authentication handshake has not completed",
            ));
        }
        if message.len() > MAX_MESSAGE_BYTES {
            // Refused before the frame is built, the rule `framed` pins for netstrings.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a websocket message exceeds the transport's bound",
            ));
        }
        self.send_raw(Message::Binary(message.to_vec())).await
    }

    fn peer_address(&self) -> Option<SocketAddr> {
        self.peer
    }

    async fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        // A clean close at a message boundary is end of stream; anything else is an error.
        let next = self.stream.next().await;
        match next {
            None => Ok(None),
            Some(Err(e)) => Err(io::Error::other(e)),
            Some(Ok(message)) => {
                if message.is_close() {
                    return Ok(None);
                }
                let bytes = message.into_data();
                if bytes.len() > MAX_MESSAGE_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a websocket message exceeds the transport's bound",
                    ));
                }
                Ok(Some(bytes))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> NoiseIdentity {
        NoiseIdentity::generate().expect("a fresh identity")
    }

    /// A listener and the locator a client dials it by — which names it by the key its
    /// challenge/response is checked against.
    async fn listening() -> (WebsocketNetlayer, PeerLocator) {
        let server_identity = identity();
        let server = WebsocketNetlayer::bind("127.0.0.1:0", server_identity.clone())
            .await
            .expect("bind");
        let mut location = server.location("server").expect("location");
        location.hints.insert(
            "verify".to_string(),
            rchain_shared::base16::encode(&server_identity.verifying_key()),
        );
        (server, location)
    }

    /// **The in-band handshake, then messages both ways.** The server proves it holds the key the dial
    /// named, and only then does either side send CapTP traffic.
    #[tokio::test]
    async fn a_websocket_session_authenticates_and_carries_messages_both_ways() {
        let (server, locator) = listening().await;
        let inbound = tokio::spawn(async move {
            let mut conn = server.accept_incoming_connection().await.expect("accept");
            let message = conn.recv().await.expect("recv").expect("a message");
            conn.send(b"pong").await.expect("send");
            message
        });

        let client = WebsocketNetlayer::bind("127.0.0.1:0", identity())
            .await
            .expect("bind the dialing side");
        let mut conn = client
            .new_outgoing_connection(&locator)
            .await
            .expect("the server should prove its identity");
        conn.send(b"ping").await.expect("send");
        assert_eq!(conn.recv().await.expect("recv").expect("a reply"), b"pong");
        assert_eq!(inbound.await.expect("join"), b"ping");
    }

    /// **A peer that cannot prove the key the dial named is refused** — the whole point of the
    /// challenge/response, without which `ws://` is an unauthenticated channel to whoever answers.
    #[tokio::test]
    async fn a_peer_that_cannot_prove_the_key_it_was_dialled_by_is_refused() {
        let (server, mut locator) = listening().await;
        // Name the server by *somebody else's* key: it will sign with its own, and the check fails.
        locator.hints.insert(
            "verify".to_string(),
            rchain_shared::base16::encode(&identity().verifying_key()),
        );

        let accepting =
            tokio::spawn(async move { server.accept_incoming_connection().await.is_ok() });
        let client = WebsocketNetlayer::bind("127.0.0.1:0", identity())
            .await
            .expect("bind the dialing side");
        let err = match client.new_outgoing_connection(&locator).await {
            Ok(_) => panic!("a peer that cannot prove the key it was dialled by must be refused"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("did not prove"),
            "the refusal names what failed: {err}"
        );
        let _ = accepting.await;
    }

    /// **Only a `init:peer-auth` is signed.** The record wrapper is what keeps the responder from
    /// being an oracle over bytes a peer chooses, so the gate is exercised directly rather than only
    /// through a socket.
    #[test]
    fn only_a_peer_auth_record_is_accepted_as_a_challenge() {
        assert!(is_peer_auth(&peer_auth(&[0u8; CHALLENGE_LEN])));
        assert!(!is_peer_auth(&Value::Symbol(INIT_PEER_AUTH.to_string())));
        // The right label with no payload is not a challenge either.
        assert!(!is_peer_auth(&Value::Record(vec![Value::Symbol(
            INIT_PEER_AUTH.to_string()
        )])));
        assert!(!is_peer_auth(&Value::Record(vec![
            Value::Symbol("desc:export".to_string()),
            Value::Bytes(vec![0u8; 4]),
        ])));
    }

    /// The envelope's signature is read from the field it belongs in, and a value that is not an
    /// envelope is refused rather than guessed at.
    #[test]
    fn the_envelope_yields_its_signature_and_nothing_else_does() {
        let good = Value::Record(vec![
            Value::Symbol(SIG_ENVELOPE.to_string()),
            peer_auth(&[0u8; CHALLENGE_LEN]),
            Value::Bytes(vec![7u8; 64]),
        ]);
        assert_eq!(
            signature_of_envelope(&good).expect("a signature"),
            vec![7u8; 64]
        );
        assert!(signature_of_envelope(&Value::Symbol(SIG_ENVELOPE.to_string())).is_err());
        assert!(signature_of_envelope(&Value::Int(0.into())).is_err());
    }
}
