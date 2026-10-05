//! The OCapN conformance suite's fixture objects.
//!
//! The suite reaches these by `fetch`ing fixed swiss numbers from the bootstrap and then delivering
//! to the result; the names and behaviours are the ones in `ocapn-test-suite/tests/`. They exist to
//! make the protocol testable, not to be useful: four of the five are implemented (the greeter, the
//! echo, the car factory, and the promise resolver). The fifth — the *sturdyref enlivener*, which
//! must dial a peer back from a sturdyref it is handed — is absent, so a `fetch` of its number
//! breaks rather than hanging.

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;

use crate::bootstrap::Bootstrap;
use crate::captp::Desc;
use crate::conn::{Act, Export, ListenOutcome, Reply};
use crate::syrup::Value;

/// The swiss numbers the suite fetches, in its own spelling.
pub const CAR_FACTORY_BUILDER: &[u8] = b"JadQ0++RzsD4M+40uLxTWVaVqM10DcBJ";
pub const ECHO_GC: &[u8] = b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w";
pub const GREETER: &[u8] = b"VMDDd1voKWarCe2GvgLbxbVFysNzRPzx";
pub const PROMISE_RESOLVER: &[u8] = b"IokCxYmMj04nos2JN1TDoY1bT8dXh6Lr";
pub const STURDYREF_ENLIVENER: &[u8] = b"gi02I1qghIwPiKGKleCQAOhpy3ZtYRpB";

/// A bootstrap carrying every fixture this crate implements.
pub fn conformance_bootstrap() -> Bootstrap {
    let mut bootstrap = Bootstrap::default();
    bootstrap.publish(CAR_FACTORY_BUILDER, Arc::new(CarFactoryBuilder));
    bootstrap.publish(ECHO_GC, Arc::new(Echo));
    bootstrap.publish(GREETER, Arc::new(Greeter));
    bootstrap.publish(PROMISE_RESOLVER, Arc::new(PromiseResolver));
    bootstrap
}

/// Delivers `["Hello"]` to whatever object it is handed.
struct Greeter;

#[async_trait]
impl Export for Greeter {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        let Some(arg) = args.first() else {
            return Err("the greeter needs an object to greet".to_string());
        };
        // The argument is a descriptor for an object of the peer's. Address it back the way the
        // reference does: an `import` becomes the matching `export`.
        let to = match Desc::from_syrup(arg) {
            Ok(Desc::ImportObject(n) | Desc::ImportPromise(n)) => Desc::Export(n),
            Ok(other) => other,
            Err(_) => return Err("the greeter expects an object reference".to_string()),
        };
        Ok(Act {
            out: vec![(to, vec![Value::String("Hello".to_string())])],
            reply: Reply::Nothing,
        })
    }
}

/// Replies with its arguments, unchanged.
struct Echo;

#[async_trait]
impl Export for Echo {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        Ok(Act::value(Value::List(args.to_vec())))
    }
}

/// `fetch`ed first; hands back a [`CarFactory`].
struct CarFactoryBuilder;

#[async_trait]
impl Export for CarFactoryBuilder {
    async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
        Ok(Act::object(Arc::new(CarFactory)))
    }
}

/// Builds a [`Car`] from one `[colour, model]` pair of symbols.
struct CarFactory;

#[async_trait]
impl Export for CarFactory {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        let Some(Value::List(pair)) = args.first() else {
            return Err("a car factory expects one [colour, model] pair".to_string());
        };
        let [Value::Symbol(colour), Value::Symbol(model)] = pair.as_slice() else {
            return Err("a car needs a colour and a model, as symbols".to_string());
        };
        Ok(Act::object(Arc::new(Car {
            colour: colour.clone(),
            model: model.clone(),
        })))
    }
}

/// Says what it is.
struct Car {
    colour: String,
    model: String,
}

#[async_trait]
impl Export for Car {
    async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
        Ok(Act::value(Value::String(format!(
            "Vroom! I am a {} {} car!",
            self.colour, self.model
        ))))
    }
}

/// Lock without letting a poisoned mutex take the session down: a poisoned lock means an earlier
/// delivery panicked, and the honest response is to keep answering rather than to panic again.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where a promise has got to.
#[derive(Default, Clone)]
enum PromiseState {
    #[default]
    Pending,
    Fulfilled(Value),
    Broken(Value),
}

/// One cell shared by a [`Vow`] and its [`Resolver`], so a listener registered before the
/// settlement and one registered after see the same answer.
#[derive(Default)]
struct PromiseCell {
    state: Mutex<PromiseState>,
    /// Descriptors to deliver the settlement to, in registration order.
    listeners: Mutex<Vec<Desc>>,
}

/// `fetch`ed at `PROMISE_RESOLVER`: hands back a fresh `[vow, resolver]` pair.
struct PromiseResolver;

#[async_trait]
impl Export for PromiseResolver {
    async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
        let cell = Arc::new(PromiseCell::default());
        Ok(Act::objects(vec![
            Arc::new(Vow(cell.clone())),
            Arc::new(Resolver(cell)),
        ]))
    }
}

/// The promise half: listenable, and not deliverable until it resolves.
struct Vow(Arc<PromiseCell>);

#[async_trait]
impl Export for Vow {
    async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
        Err("this promise has not resolved into something deliverable".to_string())
    }

    fn listen(&self, listener: Desc) -> Option<ListenOutcome> {
        let settled = lock(&self.0.state).clone();
        match settled {
            PromiseState::Pending => {
                lock(&self.0.listeners).push(listener);
                Some(ListenOutcome::Registered)
            }
            PromiseState::Fulfilled(v) => Some(ListenOutcome::Settled(vec![
                Value::Symbol("fulfill".to_string()),
                v,
            ])),
            PromiseState::Broken(e) => Some(ListenOutcome::Settled(vec![
                Value::Symbol("break".to_string()),
                e,
            ])),
        }
    }
}

/// The resolver half: `[<fulfill> <value>]` or `[<break> <reason>]` settles the pair and notifies
/// everyone who listened.
struct Resolver(Arc<PromiseCell>);

#[async_trait]
impl Export for Resolver {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        let [Value::Symbol(verb), value] = args else {
            return Err("a resolver takes [<fulfill|break> <value>]".to_string());
        };
        let (settled, notification) = match verb.as_str() {
            "fulfill" => (
                PromiseState::Fulfilled(value.clone()),
                vec![Value::Symbol("fulfill".to_string()), value.clone()],
            ),
            "break" => (
                PromiseState::Broken(value.clone()),
                vec![Value::Symbol("break".to_string()), value.clone()],
            ),
            other => return Err(format!("unknown resolver verb {other:?}")),
        };
        *lock(&self.0.state) = settled;
        let listeners: Vec<Desc> = std::mem::take(&mut *lock(&self.0.listeners));
        Ok(Act {
            out: listeners
                .into_iter()
                .map(|to| (to, notification.clone()))
                .collect(),
            reply: Reply::Nothing,
        })
    }
}
