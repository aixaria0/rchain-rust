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
                // **The two implementations disagree about the swiss number's type.** The Locators
                // draft calls it a string and Endo sends one; the Python conformance suite sends a
                // byte array. A peer that speaks to both must accept both, so this does — the
                // directory is keyed by bytes either way, and a string is its UTF-8.
                let key: Vec<u8> = match args.get(1) {
                    Some(Value::Bytes(b)) => b.clone(),
                    Some(Value::String(s)) => s.as_bytes().to_vec(),
                    _ => {
                        return Err(
                            "fetch expects a swiss number, as bytes or as a string".to_string()
                        )
                    }
                };
                match self.directory.get(&key) {
                    Some(object) => Ok(Act::object(object.clone())),
                    None => Err(format!(
                        "no object at swiss number {}",
                        base16::encode(&key)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conn::Reply;

    const SWISS: &[u8] = b"IO58l1laTyhcrgDKbEzFOO32MDd6zE5w";

    struct Marker;

    #[async_trait]
    impl Export for Marker {
        async fn deliver(&self, _args: &[Value]) -> Result<Act, String> {
            Ok(Act::value(Value::String("found".to_string())))
        }
    }

    fn catalogue() -> Bootstrap {
        let mut directory: BTreeMap<Vec<u8>, Arc<dyn Export>> = BTreeMap::new();
        directory.insert(SWISS.to_vec(), Arc::new(Marker));
        Bootstrap::new(directory)
    }

    /// **The two reference implementations disagree on the swiss number's type** (AUDIT C217): the
    /// Locators draft and Endo send a *string*; the Python conformance suite sends a *byte array*.
    /// A peer that speaks to both must accept both, so this one does.
    #[tokio::test]
    async fn bootstrap_deliver_accepts_a_swiss_number_as_bytes_or_as_a_string() {
        let bootstrap = catalogue();
        for swiss in [
            Value::Bytes(SWISS.to_vec()),
            Value::String(String::from_utf8(SWISS.to_vec()).unwrap()),
        ] {
            let act = bootstrap
                .deliver(&[Value::Symbol("fetch".into()), swiss.clone()])
                .await
                .expect("both spellings resolve");
            assert!(
                matches!(act.reply, Reply::Object(_)),
                "{swiss:?} should have found the object"
            );
        }
    }

    #[tokio::test]
    async fn bootstrap_refuses_a_swiss_number_of_any_other_type() {
        let bootstrap = catalogue();
        // `Act` is not `Debug` (it holds a `dyn Export`), so read the error arm rather than
        // `unwrap_err`.
        let reason = match bootstrap
            .deliver(&[Value::Symbol("fetch".into()), Value::Int(7.into())])
            .await
        {
            Err(reason) => reason,
            Ok(_) => "an int was accepted as a swiss number".to_string(),
        };
        assert_eq!(
            reason,
            "fetch expects a swiss number, as bytes or as a string"
        );
    }

    #[tokio::test]
    async fn bootstrap_breaks_on_an_unknown_swiss_number_rather_than_hanging() {
        let bootstrap = catalogue();
        let reason = match bootstrap
            .deliver(&[Value::Symbol("fetch".into()), Value::Bytes(vec![1, 2, 3])])
            .await
        {
            Err(reason) => reason,
            Ok(_) => "an unknown swiss number was accepted".to_string(),
        };
        assert!(reason.starts_with("no object at swiss number"), "{reason}");
    }
}
