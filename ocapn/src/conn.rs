//! The CapTP connection: a session over a netlayer, with its tables and its loop.
//!
//! This is stages 1 and 2 of the implementation guide, in the shapes the reference implementation
//! actually uses (`utils/captp.py`, `utils/captp_types.py`):
//!
//! * **The export table** — position 0 is the bootstrap object, and every object we hand a peer
//!   gets the next position. A peer addresses those with `<desc:export N>`.
//! * **The answer table** — when a peer delivers with an `answer-position`, it is asking us to make
//!   that position usable as a *recipient* for later pipelined deliveries; we remember what the
//!   delivery resolved to so a later `<desc:answer N>` reaches it.
//! * **The reply path** — a result is sent as `[<fulfill> <value>]` (or `[<break> <reason>]`) to
//!   the peer's `resolve-me-desc`, addressed as `<desc:export N>`.
//!
//! Objects do not send directly. A delivery returns an [`Act`] — outgoing messages and a reply —
//! which the loop performs. That keeps the connection single-owner (no lock around the socket, and
//! no deadlock when an object wants to talk back) at the cost of an object not being able to await
//! a reply to something it sends, which nothing here needs.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::captp::{Deliver, Desc, OpListen, DELIVER_LABEL, LISTEN_LABEL};
use crate::locator::PeerLocator;
use crate::netlayer::NetConn;
use crate::session::{my_location_payload, Abort, SessionError, StartSession, ABORT_LABEL};
use crate::session_id::{public_identifier_of_key, session_id, Octets32};
use crate::syrup::Value;

/// What an object replies with.
pub enum Reply {
    /// Nothing is owed to the peer.
    Nothing,
    /// A plain value.
    Value(Value),
    /// A new object, which the session exports and describes to the peer.
    Object(Arc<dyn Export>),
    /// Several new objects; the reply is a list of their descriptors, in order.
    Objects(Vec<Arc<dyn Export>>),
}

/// What a promise does when `op:listen` arrives.
pub enum ListenOutcome {
    /// The promise is already settled; tell the listener now, with these delivery args.
    Settled(Vec<Value>),
    /// Registered; the promise will deliver when its resolver settles it.
    Registered,
}

/// The result of a delivery: messages to send first, then what to reply.
///
/// `out` exists for the objects that must *address a peer's object* — the greeter sends `Hello` to
/// the object it was handed — without giving every object the socket.
pub struct Act {
    pub out: Vec<(Desc, Vec<Value>)>,
    pub reply: Reply,
}

impl Act {
    /// No outgoing messages, no reply.
    pub fn nothing() -> Act {
        Act {
            out: Vec::new(),
            reply: Reply::Nothing,
        }
    }
    /// No outgoing messages, a value reply.
    pub fn value(v: Value) -> Act {
        Act {
            out: Vec::new(),
            reply: Reply::Value(v),
        }
    }
    /// No outgoing messages, a new object to export.
    pub fn object(o: Arc<dyn Export>) -> Act {
        Act {
            out: Vec::new(),
            reply: Reply::Object(o),
        }
    }
    /// No outgoing messages; reply with descriptors for several new objects.
    pub fn objects(os: Vec<Arc<dyn Export>>) -> Act {
        Act {
            out: Vec::new(),
            reply: Reply::Objects(os),
        }
    }
}

/// A local object a peer may deliver to.
#[async_trait]
pub trait Export: Send + Sync {
    /// Handle one delivery. `Err(reason)` becomes a `break` for a peer that asked for a reply.
    async fn deliver(&self, args: &[Value]) -> Result<Act, String>;

    /// Register a listener for `op:listen` — a descriptor to deliver `[<fulfill …>]` or
    /// `[<break …>]` to once this promise settles. `None` means this object is not a promise, and
    /// `op:listen` on it is refused rather than silently ignored.
    fn listen(&self, _listener: Desc) -> Option<ListenOutcome> {
        None
    }
}

/// This peer's session identity: an Ed25519 key and where it accepts connections.
pub struct Identity {
    secret: [u8; 32],
    public: Vec<u8>,
    location: PeerLocator,
}

impl Identity {
    /// Derive the keypair from a 32-byte seed. The seed is the session key; OCapN generates a fresh
    /// one per session and never reuses it.
    pub fn from_seed(secret: [u8; 32], location: PeerLocator) -> Result<Identity, ConnectionError> {
        let public = rchain_crypto::signatures::ed25519::Ed25519::to_public_bytes(&secret)
            .map_err(|e| ConnectionError::Key(e.to_string()))?;
        Ok(Identity {
            secret,
            public,
            location,
        })
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public
    }

    pub fn location(&self) -> &PeerLocator {
        &self.location
    }

    /// Our side's public identifier — two SHA-256 rounds over the Syrup-encoded public-key record.
    pub fn public_identifier(&self) -> Octets32 {
        public_identifier_of_key(&public_key_record(&self.public))
    }

    /// Our `op:start-session`, signed over `<my-location <location>>`.
    pub fn start_session(&self) -> Result<StartSession, ConnectionError> {
        let payload = my_location_payload(&self.location).to_bytes();
        let sig = rchain_crypto::signatures::ed25519::Ed25519::sign_bytes(&payload, &self.secret)
            .map_err(|e| ConnectionError::Key(e.to_string()))?;
        Ok(StartSession {
            captp_version: crate::session::CAPTP_VERSION.to_string(),
            session_pubkey: self.public.clone(),
            acceptable_location: self.location.clone(),
            acceptable_location_sig: sig,
        })
    }
}

/// The public-key record a `StartSession` carries, as a `Value` (so the public identifier can be
/// taken over exactly what goes on the wire).
pub fn public_key_record(raw: &[u8]) -> Value {
    crate::session::public_key_syrup(raw)
}

/// An established CapTP session.
pub struct Session {
    conn: Box<dyn NetConn>,
    peer: StartSession,
    /// The shared session id, derived from both public identifiers.
    pub id: Octets32,
    exports: BTreeMap<u64, Arc<dyn Export>>,
    /// Answer positions the peer asked us to keep usable, and the export each resolved to.
    answers: BTreeMap<u64, Option<u64>>,
    next_export: u64,
}

impl Session {
    /// Accept a connection: the peer speaks first, then we reply — which is the order the suite's
    /// `setup_session` expects ("the peer replies with its own start-session").
    pub async fn accept(
        conn: Box<dyn NetConn>,
        identity: &Identity,
        bootstrap: Arc<dyn Export>,
    ) -> Result<Session, ConnectionError> {
        let mut conn = conn;
        let peer = match read_start_session(&mut conn).await? {
            Ok(ss) => ss,
            Err(reason) => {
                send_abort(&mut conn, &reason).await;
                return Err(ConnectionError::Handshake(reason));
            }
        };
        if !peer.location_signature_is_valid() {
            let reason = "invalid location signature".to_string();
            send_abort(&mut conn, &reason).await;
            return Err(ConnectionError::Handshake(reason));
        }
        let ours = identity.start_session()?;
        send(&mut conn, &ours.to_syrup()?).await?;
        Ok(Session::new(conn, identity, peer, bootstrap))
    }

    /// Dial a peer: we speak first, then read their reply.
    pub async fn dial(
        conn: Box<dyn NetConn>,
        identity: &Identity,
        bootstrap: Arc<dyn Export>,
    ) -> Result<Session, ConnectionError> {
        let mut conn = conn;
        let ours = identity.start_session()?;
        send(&mut conn, &ours.to_syrup()?).await?;
        let peer = match read_start_session(&mut conn).await? {
            Ok(ss) => ss,
            Err(reason) => {
                send_abort(&mut conn, &reason).await;
                return Err(ConnectionError::Handshake(reason));
            }
        };
        if !peer.location_signature_is_valid() {
            let reason = "invalid location signature".to_string();
            send_abort(&mut conn, &reason).await;
            return Err(ConnectionError::Handshake(reason));
        }
        Ok(Session::new(conn, identity, peer, bootstrap))
    }

    fn new(
        conn: Box<dyn NetConn>,
        identity: &Identity,
        peer: StartSession,
        bootstrap: Arc<dyn Export>,
    ) -> Session {
        let id = session_id_from(identity, &peer);
        let mut exports: BTreeMap<u64, Arc<dyn Export>> = BTreeMap::new();
        exports.insert(0, bootstrap);
        Session {
            conn,
            peer,
            id,
            exports,
            answers: BTreeMap::new(),
            next_export: 1,
        }
    }

    /// The peer's public key record, as received.
    pub fn peer(&self) -> &StartSession {
        &self.peer
    }

    /// The loop: read a message, act on it. Returns when the peer aborts or closes cleanly.
    pub async fn run(&mut self) -> Result<(), ConnectionError> {
        loop {
            let Some(bytes) = self.conn.recv().await? else {
                return Ok(());
            };
            let message =
                Value::from_bytes(&bytes).map_err(|e| ConnectionError::Protocol(e.to_string()))?;
            let Some(label) = record_label(&message) else {
                return Err(ConnectionError::Protocol("message is not a record".into()));
            };
            match label {
                DELIVER_LABEL => self.handle_deliver(&message).await?,
                LISTEN_LABEL => self.handle_listen(&message).await?,
                ABORT_LABEL => return Ok(()),
                // GC only ever releases resources; ignoring it is safe, not partial.
                "op:gc-exports" | "op:gc-answers" => {}
                other => {
                    // Anything else we do not implement; say so rather than silently stall.
                    let reason = format!("unsupported operation {other:?}");
                    send_abort(&mut self.conn, &reason).await;
                    return Err(ConnectionError::Protocol(reason));
                }
            }
        }
    }

    /// Send a raw CapTP message. The loop uses the typed paths; this is for drivers and tests that
    /// need to assert on the exact wire shape.
    pub async fn send_message(&mut self, message: &Value) -> Result<(), ConnectionError> {
        send(&mut self.conn, message).await
    }

    /// Read one message without acting on it.
    pub async fn recv_message(&mut self) -> Result<Option<Value>, ConnectionError> {
        match self.conn.recv().await? {
            Some(bytes) => Ok(Some(
                Value::from_bytes(&bytes).map_err(|e| ConnectionError::Protocol(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    async fn handle_deliver(&mut self, message: &Value) -> Result<(), ConnectionError> {
        let deliver =
            Deliver::from_syrup(message).map_err(|e| ConnectionError::Protocol(e.to_string()))?;

        let target = match resolve_to(&self.exports, &self.answers, &deliver.to)? {
            Resolution::Object(o) => o,
            Resolution::Broken(reason) => {
                // A delivery pipelined onto an answer that broke must itself break.
                self.fulfil(
                    deliver.resolve_me_desc.as_ref(),
                    vec![Value::Symbol("break".to_string()), Value::String(reason)],
                )
                .await?;
                return Ok(());
            }
            Resolution::Missing => {
                let reason = "no such export or answer".to_string();
                send_abort(&mut self.conn, &reason).await;
                return Err(ConnectionError::Protocol(reason));
            }
        };

        let act = match target.deliver(&deliver.args).await {
            Ok(act) => act,
            Err(reason) => {
                // The answer this delivery would have filled is now broken, so a later pipelined
                // delivery onto it breaks too.
                if let Some(n) = &deliver.answer_pos {
                    self.answers.insert(position(n)?, None);
                }
                // A break is `[<break> <reason>]`, sent to the peer's resolver if it left one.
                self.fulfil(
                    deliver.resolve_me_desc.as_ref(),
                    vec![Value::Symbol("break".to_string()), Value::String(reason)],
                )
                .await?;
                return Ok(());
            }
        };

        for (to, args) in act.out {
            self.send_deliver(to, args).await?;
        }

        // What we owe the peer, and what a later pipelined delivery to this answer must reach.
        let (reply, answer_target) = match act.reply {
            Reply::Nothing => (None, None),
            Reply::Value(v) => (Some(v), None),
            Reply::Object(o) => {
                let pos = self.insert_export(o);
                (Some(Desc::ImportObject(pos.into()).to_syrup()), Some(pos))
            }
            Reply::Objects(os) => {
                let mut parts = Vec::with_capacity(os.len());
                for o in os {
                    let pos = self.insert_export(o);
                    parts.push(Desc::ImportObject(pos.into()).to_syrup());
                }
                (Some(Value::List(parts)), None)
            }
        };
        if let Some(n) = &deliver.answer_pos {
            self.answers.insert(position(n)?, answer_target);
        }
        if let Some(value) = reply {
            self.fulfil(
                deliver.resolve_me_desc.as_ref(),
                vec![Value::Symbol("fulfill".to_string()), value],
            )
            .await?;
        }
        Ok(())
    }

    async fn handle_listen(&mut self, message: &Value) -> Result<(), ConnectionError> {
        let listen =
            OpListen::from_syrup(message).map_err(|e| ConnectionError::Protocol(e.to_string()))?;
        let target = match resolve_to(&self.exports, &self.answers, &listen.to)? {
            Resolution::Object(o) => o,
            Resolution::Broken(_) | Resolution::Missing => {
                let reason = "op:listen names no object".to_string();
                send_abort(&mut self.conn, &reason).await;
                return Err(ConnectionError::Protocol(reason));
            }
        };
        let listener = match &listen.resolve_me_desc {
            Desc::ImportObject(n) | Desc::ImportPromise(n) => Desc::Export(n.clone()),
            _ => {
                let reason = "op:listen needs an import descriptor to notify".to_string();
                send_abort(&mut self.conn, &reason).await;
                return Err(ConnectionError::Protocol(reason));
            }
        };
        match target.listen(listener.clone()) {
            // Already settled: the notification is owed now.
            Some(ListenOutcome::Settled(args)) => self.send_deliver(listener, args).await?,
            // Registered; the promise will speak up when its resolver settles it.
            Some(ListenOutcome::Registered) => {}
            None => {
                let reason = "op:listen on something that is not a promise".to_string();
                send_abort(&mut self.conn, &reason).await;
                return Err(ConnectionError::Protocol(reason));
            }
        }
        Ok(())
    }

    /// Send `[<fulfill> …]` — or nothing, when the peer asked for no reply.
    async fn fulfil(
        &mut self,
        resolve_me_desc: Option<&Desc>,
        args: Vec<Value>,
    ) -> Result<(), ConnectionError> {
        if let Some(Desc::ImportObject(p) | Desc::ImportPromise(p)) = resolve_me_desc {
            self.send_deliver(Desc::Export(p.clone()), args).await?;
        }
        Ok(())
    }

    fn insert_export(&mut self, object: Arc<dyn Export>) -> u64 {
        let pos = self.next_export;
        self.next_export += 1;
        self.exports.insert(pos, object);
        pos
    }

    async fn send_deliver(&mut self, to: Desc, args: Vec<Value>) -> Result<(), ConnectionError> {
        let deliver = Deliver {
            to,
            args,
            answer_pos: None,
            resolve_me_desc: None,
        };
        send(&mut self.conn, &deliver.to_syrup()).await
    }
}

/// What a `to` descriptor names.
enum Resolution {
    Object(Arc<dyn Export>),
    /// An answer whose delivery broke; a pipelined delivery onto it must break too.
    Broken(String),
    /// No such export or answer.
    Missing,
}

fn resolve_to(
    exports: &BTreeMap<u64, Arc<dyn Export>>,
    answers: &BTreeMap<u64, Option<u64>>,
    to: &Desc,
) -> Result<Resolution, ConnectionError> {
    match to {
        Desc::Export(n) => Ok(match exports.get(&position(n)?) {
            Some(o) => Resolution::Object(o.clone()),
            None => Resolution::Missing,
        }),
        Desc::Answer(n) => Ok(match answers.get(&position(n)?) {
            Some(Some(pos)) => match exports.get(pos) {
                Some(o) => Resolution::Object(o.clone()),
                None => Resolution::Missing,
            },
            Some(None) => Resolution::Broken("the delivery it pipelined onto broke".to_string()),
            None => Resolution::Missing,
        }),
        _ => Ok(Resolution::Missing),
    }
}

/// The shared session id, from our identifier and the peer's record on the wire.
fn session_id_from(identity: &Identity, peer: &StartSession) -> Octets32 {
    // Both sides hash the Syrup-encoded public-key record, so feed `session_id` exactly that.
    session_id(
        &public_key_record(identity.public_key()).to_bytes(),
        &public_key_record(&peer.session_pubkey).to_bytes(),
    )
}

fn position(n: &num_bigint::BigUint) -> Result<u64, ConnectionError> {
    u64::try_from(n.clone()).map_err(|_| ConnectionError::Protocol("position too large".into()))
}

fn record_label(v: &Value) -> Option<&str> {
    match v {
        Value::Record(fields) => match fields.first() {
            Some(Value::Symbol(s)) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

async fn read_start_session(
    conn: &mut Box<dyn NetConn>,
) -> Result<Result<StartSession, String>, ConnectionError> {
    let Some(bytes) = conn.recv().await? else {
        return Err(ConnectionError::Protocol(
            "connection closed during handshake".into(),
        ));
    };
    let message =
        Value::from_bytes(&bytes).map_err(|e| ConnectionError::Protocol(e.to_string()))?;
    match StartSession::from_syrup(&message) {
        Ok(ss) => Ok(Ok(ss)),
        Err(SessionError::UnsupportedVersion(v)) => {
            Ok(Err(format!("unsupported captp-version {v:?}")))
        }
        Err(e) => Err(ConnectionError::Protocol(e.to_string())),
    }
}

async fn send(conn: &mut Box<dyn NetConn>, value: &Value) -> Result<(), ConnectionError> {
    conn.send(&value.to_bytes()).await.map_err(Into::into)
}

async fn send_abort(conn: &mut Box<dyn NetConn>, reason: &str) {
    let abort = Abort {
        reason: reason.to_string(),
    };
    // Best effort: the peer may already be gone.
    let _ = conn.send(&abort.to_syrup().to_bytes()).await;
}

/// A connection-level failure.
#[derive(Debug)]
pub enum ConnectionError {
    /// The socket failed.
    Transport(std::io::Error),
    /// A handshake was refused; the reason is what the peer was told.
    Handshake(String),
    /// A message was not what CapTP says it should be.
    Protocol(String),
    /// The session key could not be derived or used.
    Key(String),
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionError::Transport(e) => write!(f, "ocapn transport: {e}"),
            ConnectionError::Handshake(r) => write!(f, "ocapn handshake refused: {r}"),
            ConnectionError::Protocol(r) => write!(f, "ocapn protocol error: {r}"),
            ConnectionError::Key(r) => write!(f, "ocapn session key: {r}"),
        }
    }
}

impl std::error::Error for ConnectionError {}

impl From<std::io::Error> for ConnectionError {
    fn from(e: std::io::Error) -> Self {
        ConnectionError::Transport(e)
    }
}

impl From<SessionError> for ConnectionError {
    fn from(e: SessionError) -> Self {
        ConnectionError::Protocol(e.to_string())
    }
}
