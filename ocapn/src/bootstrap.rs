//! The bootstrap object: position 0 of every session.
//!
//! `CapTP Specification.md`: the bootstrap is "always the first export, at position 0", and its
//! methods are `fetch` (a swiss number for an object), `deposit-gift`, and `withdraw-gift`. This
//! implements `fetch` — the path the conformance suite uses to reach every fixture object — and
//! refuses the two handoff methods with a reason, which the peer sees as a `break` rather than a
//! hang.
//!
//! The directory is deliberately a plain map from swiss number to object, with no capability
//! hiding: reachability is what a swiss number *is* here, and the objects it names are the ones the
//! node chose to publish.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use rchain_shared::base16;

use crate::conn::{Act, Export};
use crate::syrup::Value;

/// A directory of swiss numbers to the objects they name, as the bootstrap's `fetch` sees it.
#[derive(Default)]
pub struct Bootstrap {
    directory: BTreeMap<Vec<u8>, Arc<dyn Export>>,
}

impl Bootstrap {
    pub fn new(directory: BTreeMap<Vec<u8>, Arc<dyn Export>>) -> Bootstrap {
        Bootstrap { directory }
    }

    /// Publish an object under a swiss number.
    pub fn publish(&mut self, swiss_num: impl Into<Vec<u8>>, object: Arc<dyn Export>) -> &mut Self {
        self.directory.insert(swiss_num.into(), object);
        self
    }
}

#[async_trait]
impl Export for Bootstrap {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        match args.first() {
            Some(Value::Symbol(method)) if method == "fetch" => {
                let Some(Value::Bytes(swiss)) = args.get(1) else {
                    return Err("fetch expects a byte-array swiss number".to_string());
                };
                match self.directory.get(swiss) {
                    Some(object) => Ok(Act::object(object.clone())),
                    None => Err(format!(
                        "no object at swiss number {}",
                        base16::encode(swiss)
                    )),
                }
            }
            Some(Value::Symbol(m)) if m == "deposit-gift" || m == "withdraw-gift" => {
                Err("third-party handoffs are not implemented yet".to_string())
            }
            other => Err(format!("unknown bootstrap method: {other:?}")),
        }
    }
}
