//! The OCapN conformance suite's fixture objects.
//!
//! The suite reaches these by `fetch`ing fixed swiss numbers from the bootstrap and then delivering
//! to the result; the names and behaviours are the ones in `ocapn-test-suite/tests/op_deliver.py`.
//! They exist to make the protocol testable, not to be useful: three of the five are implemented
//! (the greeter, the echo, and the car factory), and the two that need machinery this crate does
//! not have yet — the promise resolver's `op:listen` path and the sturdyref enlivener's dial-back —
//! are absent, so a `fetch` of those numbers breaks rather than hanging.

use std::sync::Arc;

use async_trait::async_trait;

use crate::bootstrap::Bootstrap;
use crate::captp::Desc;
use crate::conn::{Act, Export, Reply};
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
