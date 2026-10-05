//! Sending on one session from inside a delivery on another: the catcher, and the forwarder.
//!
//! A sturdyref enlivener is the reason this exists: it is delivered a sturdyref on session A, dials
//! the peer that sturdyref names to make session B, fetches an object from B, and has to hand that
//! object *back to A's peer*. The fetched thing is a descriptor on **B** — "an object B's peer
//! exports at position N" — and a descriptor is meaningless on A, so the value cannot simply be
//! passed through. [`Forward`] is what crosses instead: an object that lives on the connection that
//! owns it, and that a delivery on any session can be routed through.
//!
//! **The wait.** `Export::deliver` is async, so a forward can *await* its answer instead of returning
//! a promise. That blocks the session it was delivered on — acceptable here, and only here, because
//! the peer that asked is the peer that is waiting, and everything else that peer sends would have to
//! queue behind the answer anyway. It is bounded by a timeout, so a peer that never answers costs
//! that session a bounded wait rather than a hung task (which is what a suite that only checks the
//! *dial* would otherwise leave behind).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::oneshot;

use crate::captp::{Desc, IMPORT_OBJECT_LABEL, IMPORT_PROMISE_LABEL};
use crate::conn::{Act, ConnectionError, Export, Reply};
use crate::owner::{HandOff, SessionHandle};
use crate::syrup::Value;

/// How long a forwarded delivery waits for its answer before breaking.
pub const FORWARD_TIMEOUT: Duration = Duration::from_secs(30);

/// Catches the `[<fulfill> …]` (or `[<break> …]`) a peer sends to a `resolve-me-desc` we registered
/// on another session.
pub struct Catcher {
    tx: Mutex<Option<oneshot::Sender<Result<Value, String>>>>,
}

/// The two halves: the object to hand a delivery, and the wait to run afterwards.
///
/// Split rather than returned as one value because the catcher must be *in* the delivery before the
/// answer can arrive — so a caller pairs, sends, then awaits.
pub fn catcher() -> (Arc<Catcher>, oneshot::Receiver<Result<Value, String>>) {
    let (tx, rx) = oneshot::channel();
    (
        Arc::new(Catcher {
            tx: Mutex::new(Some(tx)),
        }),
        rx,
    )
}

#[async_trait]
impl Export for Catcher {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        let outcome = match args {
            [Value::Symbol(verb), rest @ ..] => match verb.as_str() {
                "fulfill" => Ok(rest.first().cloned().unwrap_or(Value::Bool(false))),
                "break" => Err(match rest.first() {
                    Some(Value::String(reason)) => reason.clone(),
                    other => format!("the peer broke the promise: {other:?}"),
                }),
                other => Err(format!(
                    "a resolver was delivered `{other}`, not a fulfilment"
                )),
            },
            _ => Err("a resolver was delivered a message that is not a fulfilment".to_string()),
        };
        if let Some(tx) = self.tx.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(outcome);
        }
        Ok(Act::nothing())
    }
}

/// An object that lives on another session: a delivery here becomes a delivery there, and its answer
/// is the answer here.
pub struct Forward {
    session: SessionHandle,
    /// Where the object is on that session, **as we address it**: an export of the peer's, so
    /// `Desc::Export(N)` for the import it handed us.
    to: Desc,
    timeout: Duration,
}

impl Forward {
    pub fn new(session: SessionHandle, to: Desc) -> Forward {
        Forward {
            session,
            to,
            timeout: FORWARD_TIMEOUT,
        }
    }

    /// How to address a value the peer handed us as a reference: `<desc:import-object N>` names the
    /// *peer's* export N, and we address it as `Desc::Export(N)` on that session — the same inversion
    /// `Session::fulfil` makes on the reply path, and the one the conformance suite's own client
    /// makes when it turns a fetched `DescImportObject` into a `DescExport` to address it.
    pub fn address_of(value: &Value) -> Option<Desc> {
        let Value::Record(fields) = value else {
            return None;
        };
        let [Value::Symbol(label), Value::Int(n)] = fields.as_slice() else {
            return None;
        };
        if label != IMPORT_OBJECT_LABEL && label != IMPORT_PROMISE_LABEL {
            return None;
        }
        if n.sign() == num_bigint::Sign::Minus {
            return None;
        }
        Some(Desc::Export(n.magnitude().clone()))
    }
}

#[async_trait]
impl Export for Forward {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        let (catcher, rx) = catcher();
        self.session
            .deliver(HandOff {
                to: self.to.clone(),
                args: args.to_vec(),
                resolve_me: Some(catcher),
            })
            .await
            .map_err(|e: ConnectionError| e.to_string())?;
        match tokio::time::timeout(self.timeout, rx).await {
            Err(_) => Err("the forwarded delivery was not answered in time".to_string()),
            Ok(Err(_)) => Err("the session this object lives on ended".to_string()),
            Ok(Ok(Err(reason))) => Err(reason),
            Ok(Ok(Ok(value))) => match Self::address_of(&value) {
                // The answer is itself a reference: keep forwarding, on the same session.
                Some(to) => Ok(Act::object(Arc::new(Forward {
                    session: self.session.clone(),
                    to,
                    timeout: self.timeout,
                }))),
                None => Ok(Act {
                    out: Vec::new(),
                    reply: Reply::Value(value),
                }),
            },
        }
    }
}
