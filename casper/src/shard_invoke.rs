//! Cross-shard invoke as a remote signed deploy (issue #33, Layer 1).
//!
//! A cross-shard invoke is **not** a relay or a bespoke envelope protocol: it is an
//! ordinary, caller-signed deploy submitted to the target shard. Identity is already
//! the same account everywhere — the far node binds `rho:rchain:deployerId` from the
//! deploy signature, which is the caller's own key — so a `deployerId`-gated
//! capability on the far shard sees exactly the caller it would locally.
//!
//! This module is the client-side primitive. Because only the keyholder can sign,
//! it is a client helper (the node performs no server-side invoke):
//!
//! * [`invoke_term`] builds the term that runs on the target shard. The reply is
//!   written to `` `rho:rchain:deployId` `` — the deploy's own id, which is the
//!   reply *channel* the caller listens on.
//! * [`signed_invoke`] signs that term with the caller's key.
//! * [`await_reply`] awaits that reply channel with the node's listen
//!   (`listenForDataAtName`), mapping the channel's value to the caller-visible
//!   result. It does **not** poll `deployStatus`: a reply is data on a channel, so
//!   the client listens for it.
//!
//! See `docs/src/node/shard-invoke.md` for the design and its consequences
//! (client-orchestrated, non-atomic bilateral exchange; Layer 2 is out of scope).

use std::time::Duration;

use rchain_crypto::private_key::PrivateKey;
use rchain_crypto::signatures::signed::Signed;
use rchain_models::ast::Par;
use rchain_models::casper::protocol::casper_message::DeployData;
use rchain_models::casper::protocol::deploy_service::{DataAtNameQuery, DataWithBlockInfo};
use rchain_models::rholang::RhoType::{RhoDeployId, RhoString, RhoTupleN};
use rchain_rholang::pretty_printer::PrettyPrinter;
use tokio::time::{sleep, Instant};

use crate::construct_deploy;
use crate::protocol::client::DeployService;

/// The unforgeable reply channel a remote deploy writes its result to. The far node
/// binds it from the deploy signature, so the caller can listen on it once it knows
/// the deploy id.
pub const REMOTE_REPLY_CHANNEL: &str = "rho:rchain:deployId";

/// The native registry-lookup system process used to resolve the target capability.
pub const REGISTRY_LOOKUP: &str = "rho:registry:lookup";

/// The tag of the failure-as-value tuple: `("shard-error", reason)`.
pub const SHARD_ERROR_TAG: &str = "shard-error";

/// Build the term run on the target shard for `$at(shard, targetUri)!(method, args…)`.
///
/// `args` are already-normalized rholang `Par`s; they are rendered with the pretty
/// printer so names/data keep their rholang literal form. The invoked capability's
/// reply is sent to the deploy's own id — the reply channel the caller listens on
/// (see [`reply_channel`] / [`await_reply`]).
///
/// **The reply channel must be *bound*, not written as a URI** (AUDIT C218). A backticked
/// `` `rho:rchain:deployId` `` is a `GUri` *ground* — an ordinary, guessable name — and the
/// normalizer leaves it alone; the unforgeable per-deploy channel is what
/// `deployId(`rho:rchain:deployId`)` **binds**, so the name has to be introduced and passed as
/// `*deployId`, exactly as [`crate::txn_coordinator::txn_term`] does. Sending to the URI instead
/// reaches a real channel that nobody reads, and the deploy still reports success.
///
/// A registry miss yields `Nil`; the `for` then does not fire and the deploy produces
/// nothing on the reply channel, which [`await_reply`] reports as a `shard-error`.
pub fn invoke_term(target_uri: &str, method: &str, args: &[Arg]) -> Result<String, String> {
    let pp = PrettyPrinter::new();
    let uri_lit = pp.build_string(&RhoString::apply(target_uri.to_string()));
    let method_lit = pp.build_string(&RhoString::apply(method.to_string()));
    // The method is printed into the term too, so it is checked by the same rule as a value (C220).
    rchain_rholang::pretty_printer::check_renderable(&RhoString::apply(method.to_string()))
        .map_err(|e| format!("the method cannot be rendered into a term: {e}"))?;
    let (payload, binders) = payload_and_binders(&method_lit, args, &pp)?;
    let (call, extra_names) = wrap_binders(binders, format!("@target!({payload}, *deployId)"), &pp);
    let extra = extra_bindings(&extra_names);

    Ok(format!(
        "new lookup(`{REGISTRY_LOOKUP}`), deployId(`{REMOTE_REPLY_CHANNEL}`), cap{extra} in {{ \
           lookup!({uri_lit}, *cap) | \
           for (@(_, target) <- cap) {{ \
             {call} \
           }} \
         }}"
    ))
}

/// One argument of a built term.
///
/// **A capability cannot be written as data** (C224 item 4). A peer holds one as a descriptor — a
/// position in *our* export table — and the object behind that position is a chain object the node
/// registered, so it has a `rho:id:` location and a pattern that binds it inside the registered value.
/// Such an argument becomes a **name the term binds**: the term looks the location up and matches the
/// pattern, then passes the name. Everything else is a value, printed as a literal.
///
/// The distinction is here rather than in the node because the *term* is what has to express it: a
/// name has no source literal, so it can only reach a contract by being bound in the deploying term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    /// A value, rendered as a rholang literal.
    Value(Par),
    /// A capability the node can name on chain: the registry URI it lives at, and the pattern that
    /// binds it inside the value registered there (`None` when the registered value *is* the object).
    Named {
        location: String,
        pattern: Option<String>,
    },
}

impl Arg {
    /// A capability argument that is the registered value itself.
    pub fn named(location: impl Into<String>) -> Arg {
        Arg::Named {
            location: location.into(),
            pattern: None,
        }
    }
}

/// The payload a call passes, and the binders its named arguments still need.
///
/// A capability is passed **bare** — `@purse!("deposit", payment, *ret)` — because the name is what
/// the contract wants; a value is passed as its literal.
type Binders = Vec<(usize, String, Option<String>)>;

fn payload_and_binders(
    method_lit: &str,
    args: &[Arg],
    pp: &PrettyPrinter,
) -> Result<(String, Binders), String> {
    let mut payload: Vec<String> = Vec::with_capacity(args.len() + 1);
    payload.push(method_lit.to_string());
    let mut binders: Binders = Vec::new();

    for (index, arg) in args.iter().enumerate() {
        match arg {
            Arg::Value(value) => {
                // **Every value printed into this term is parsed back as code**, so it is checked
                // first (AUDIT C220): one that cannot be written as a literal would end the literal
                // early and be read as a process — in a deploy the *caller's* key signs.
                rchain_rholang::pretty_printer::check_renderable(value)
                    .map_err(|e| format!("argument {index} cannot be rendered into a term: {e}"))?;
                payload.push(pp.build_string(value));
            }
            Arg::Named { location, pattern } => {
                rchain_rholang::pretty_printer::check_renderable(&RhoString::apply(
                    location.clone(),
                ))
                .map_err(|e| {
                    format!("argument {index}'s location cannot be rendered into a term: {e}")
                })?;
                payload.push(format!("arg{index}"));
                binders.push((index, location.clone(), pattern.clone()));
            }
        }
    }
    Ok((payload.join(", "), binders))
}

/// Nest every binder's `lookup!`/`match` around `inner`, and list the names the term must bind.
///
/// Built inside-out, so the innermost call sees every bound name in scope and each level's `lookup!`
/// is a sibling of the level below it.
fn wrap_binders(binders: Binders, inner: String, pp: &PrettyPrinter) -> (String, Vec<String>) {
    let extra: Vec<String> = binders
        .iter()
        .flat_map(|(index, _, _)| {
            [
                format!("arg{index}"),
                format!("arg{index}Ch"),
                format!("arg{index}Root"),
            ]
        })
        .collect();
    let mut body = inner;
    for (index, location, pattern) in binders.into_iter().rev() {
        let name = format!("arg{index}");
        let channel = format!("arg{index}Ch");
        let root = format!("arg{index}Root");
        let uri_lit = pp.build_string(&RhoString::apply(location));
        // **The leaf is renamed by token, not by position.** `capability_patterns_at` emits the token
        // `member` exactly once, as the leaf, so the rename is total; if that ever changed the term
        // would hold a free variable and the normalizer would refuse it — a named break, never a call
        // that reaches the wrong object.
        let bound = match pattern {
            None => format!("for (@(_, {name}) <- {channel}) {{ {body} }}"),
            Some(pattern) => {
                let pattern = pattern.replacen("member", &name, 1);
                format!(
                    "for (@{root} <- {channel}) {{ match {root} {{ {pattern} => {{ {body} }} \
                     _ => {{ deployId!(false) }} }} }}"
                )
            }
        };
        body = format!("lookup!({uri_lit}, *{channel}) | {bound}");
    }
    (body, extra)
}

/// The extra names a term must bind, as a `new` suffix.
fn extra_bindings(extra: &[String]) -> String {
    if extra.is_empty() {
        String::new()
    } else {
        format!(", {}", extra.join(", "))
    }
}

/// Build the term that calls a **member of a registered value** and registers what that call returns.
///
/// This is the OCapN bridge's second shape, and it exists because of a fact about the calculus: a
/// capability a contract *returns* — an ERTP purse, an issuer, a brand — is an unforgeable name, and
/// a `GPrivate` has **no rholang source literal**, so a later deploy cannot address it by writing it
/// down. The one thing that survives between deploys is chain state, so the answer is to put the
/// object *into* the registry in the same deploy that produced it: the term calls the method, feeds
/// the whole reply to `insertArbitrary`, and hands **both** the fresh URI and the value to the
/// deploy's reply channel — the URI is how a later call reaches a member of it, and the value is how
/// the caller (the node, which knows the signing key) finds each capability inside it.
///
/// **Why `insertArbitrary` and not `insertSigned`.** `insertSigned` derives the URI from the
/// *deployer's* public key, which would put every returned object at one URI — and worse, it forces
/// the object to be registered by a deploy signed with that key. A fresh key per object has no REV,
/// so its deploy cannot pay for itself: measured, `preCharge: insufficient funds (0 < 1000000)`, and
/// the call breaks. `insertArbitrary` needs no key at all and mints a fresh URI per call, so the
/// deploy can be signed by the node's own funded key, and each returned object still lands somewhere
/// of its own.
///
/// `pattern` says how to bind the object inside the registered value, and is **written against the
/// value's actual shape** rather than derived from a position path — see [`crate::api`]'s caller for
/// why. `None` is the symbolic case: the registered value *is* the object (a chain-registered urn
/// like `rho:rchain:ertp`, whose entry the registry stores as `(space, value)`). A pattern that does
/// not fit answers `false` on the reply channel rather than stalling.
pub fn invoke_member_term(
    target_uri: &str,
    pattern: Option<&str>,
    method: &str,
    args: &[Arg],
) -> Result<String, String> {
    let pp = PrettyPrinter::new();
    let uri_lit = pp.build_string(&RhoString::apply(target_uri.to_string()));
    let method_lit = pp.build_string(&RhoString::apply(method.to_string()));
    // Here the arguments are a *peer's*, delivered over CapTP (C220), and so is a capability's
    // location. `payload_and_binders` applies that check to both.
    let (payload, binders) = payload_and_binders(&method_lit, args, &pp)?;

    // The reply carries `(uri, value)`: the URI the reply was just registered under, then the value
    // itself — so a caller that asks for a capability gets the handle to it *and* the shape it sits
    // in, in one round trip. The named arguments' binders wrap this whole call, so a capability passed
    // as an argument is in scope for the send that uses it.
    let (call, extra_names) = wrap_binders(
        binders,
        format!(
            "@member!({payload}, *replyCh) | \
             for (@reply <- replyCh) {{ \
               register!(reply, *uriOut) | \
               for (@uri <- uriOut) {{ deployId!((uri, reply)) }} \
             }}"
        ),
        &pp,
    );
    let extra = extra_bindings(&extra_names);
    // **A tuple pattern is exact, not "rest-able".** Rholang patterns have no `…` for tuples: a
    // `(_, member)` pattern matches a 2-tuple *and nothing else*, so a 3-tuple reply like an ERTP
    // kit `(brand, mint, issuer)` falls straight through it and the deploy answers nothing at all —
    // measured, which is why the pattern is built from the value the caller already saw rather than
    // from a path assumed to have one element per level.
    let resolve = match pattern {
        None => format!("for (@(_, member) <- cap) {{ {call} }}"),
        Some(pattern) => format!(
            "for (@root <- cap) {{ \
               match root {{ \
                 {pattern} => {{ {call} }} \
                 _ => {{ deployId!(false) }} \
               }} \
             }}"
        ),
    };

    Ok(format!(
        "new lookup(`{REGISTRY_LOOKUP}`), deployId(`{REMOTE_REPLY_CHANNEL}`), \
             register(`rho:registry:insertArbitrary`), cap, uriOut, replyCh{extra} in {{ \
           lookup!({uri_lit}, *cap) | {resolve} \
         }}"
    ))
}

/// Sign the far-shard term with the caller's key (port of `deployFileProgram`'s
/// signing), producing the deploy to submit to the target shard's deploy service.
///
/// `shard_id` must be the target shard's id (the far node rejects a mismatched
/// `DeployData.shardId`). `deployerId` on the far shard is this key's public key.
pub fn signed_invoke(
    term: &str,
    caller_key: &PrivateKey,
    timestamp: i64,
    phlo_limit: i64,
    phlo_price: i64,
    valid_after_block_number: i64,
    shard_id: &str,
) -> Result<Signed<DeployData>, String> {
    construct_deploy::source_deploy(
        term,
        timestamp,
        phlo_limit,
        phlo_price,
        caller_key,
        valid_after_block_number,
        shard_id,
    )
}

/// The reply channel of a remote deploy: its own id (signature) as the unforgeable
/// name `` `rho:rchain:deployId` `` resolves to on the far shard.
pub fn reply_channel(deploy_id: &[u8]) -> Par {
    RhoDeployId::apply(deploy_id.to_vec())
}

/// The caller-visible outcome of a remote invoke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShardOutcome {
    /// The reply channel produced a value.
    Value(Par),
    /// The invoke failed or the reply channel stayed empty; rendered as
    /// `("shard-error", reason)` by [`ShardOutcome::into_value`].
    Error(String),
}

impl ShardOutcome {
    /// The rholang value the caller's `for` observes: the reply, or
    /// `("shard-error", reason)`.
    pub fn into_value(self) -> Par {
        match self {
            ShardOutcome::Value(v) => v,
            ShardOutcome::Error(reason) => shard_error(&reason),
        }
    }
}

/// `("shard-error", reason)` — an ordinary rholang tuple value, so failure composes
/// with local rholang instead of blocking.
pub fn shard_error(reason: &str) -> Par {
    RhoTupleN::apply(vec![
        RhoString::apply(SHARD_ERROR_TAG.to_string()),
        RhoString::apply(reason.to_string()),
    ])
}

/// The first value produced on the reply channel, from a listen result.
///
/// Data at the deploy's id channel *is* the reply; an empty result means the deploy
/// has not produced one.
pub fn reply_outcome(data: &[DataWithBlockInfo]) -> ShardOutcome {
    match data.iter().find_map(|d| d.post_block_data.first()) {
        Some(v) => ShardOutcome::Value(v.clone()),
        None => ShardOutcome::Error("no reply on the deploy reply channel".to_string()),
    }
}

/// Listen on a remote deploy's reply channel until the reply appears (or `timeout`
/// elapses), returning the value or a `shard-error`.
///
/// This is the channel-wait the primitive is built on — **no `deployStatus` polling**.
/// The node's `listenForDataAtName` is a one-shot query (the same shape as the Scala
/// oracle, whose client waits with `listenAtNameUntilChanges`), so the await
/// re-listens on the *channel* at `listen_interval` until the reply commits. Callers
/// that want the wait in the transport should use a streaming listen instead.
pub async fn await_reply(
    service: &dyn DeployService,
    deploy_id: &[u8],
    listen_interval: Duration,
    timeout: Duration,
) -> ShardOutcome {
    let query = DataAtNameQuery {
        depth: i32::MAX,
        name: reply_channel(deploy_id),
    };
    let interval = if listen_interval.is_zero() {
        Duration::from_millis(250)
    } else {
        listen_interval
    };
    let deadline = Instant::now() + timeout;
    loop {
        match service.listen_for_data_at_name(&query).await {
            Ok(data) => {
                if data.iter().any(|d| !d.post_block_data.is_empty()) {
                    return reply_outcome(&data);
                }
                if Instant::now() >= deadline {
                    return ShardOutcome::Error(format!(
                        "timed out listening on the reply channel of deploy {}",
                        rchain_shared::base16::encode(deploy_id)
                    ));
                }
                sleep(interval).await;
            }
            Err(errors) => return ShardOutcome::Error(errors.join("; ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rchain_crypto::signatures::secp256k1::Secp256k1;
    use rchain_crypto::signatures::signatures_alg::SignaturesAlg;
    use rchain_crypto::signatures::signed::signature_hash;
    use rchain_models::casper::protocol::deploy_service::LightBlockInfo;
    use rchain_models::rholang::RhoType::RhoByteArray;
    use rchain_shared::serialize::Serialize;

    fn light_block() -> LightBlockInfo {
        LightBlockInfo {
            version: 1,
            shard_id: "root".to_string(),
            block_hash: String::new(),
            block_number: 0,
            sender: String::new(),
            seq_num: 0,
            pre_state_hash: String::new(),
            post_state_hash: String::new(),
            justifications: Vec::new(),
            bonds: Vec::new(),
            sig_algorithm: String::new(),
            sig: String::new(),
            block_size: "0".to_string(),
            deploy_count: 0,
            rejected_deploys: Vec::new(),
            timestamp: 0,
        }
    }

    fn data_with(value: Option<Par>) -> DataWithBlockInfo {
        DataWithBlockInfo {
            post_block_data: value.into_iter().collect(),
            block: light_block(),
        }
    }

    /// **A capability argument becomes a bound name** (C224 item 4). It cannot be a literal — a name
    /// has no source form — so the term looks the registered location up, matches the pattern that
    /// binds it inside the registered value, and passes the name bare, the way the ERTP contracts take
    /// a purse: `@purse!("deposit", payment, *ret)`.
    #[test]
    fn a_named_argument_is_bound_by_a_lookup_and_the_term_parses() {
        let term = invoke_term(
            "rho:rchain:ertp",
            "take",
            &[
                Arg::Named {
                    location: "rho:id:theKit".to_string(),
                    pattern: Some("(_, _, member)".to_string()),
                },
                Arg::Value(RhoString::apply("plain".to_string())),
            ],
        )
        .expect("a name and a value both render");

        // It parses — the assertion that matters, because the binding is a *pattern* the normalizer
        // would refuse if the leaf rename or the nesting were wrong.
        rchain_rholang::normalizer::source_to_adt(&term)
            .unwrap_or_else(|e| panic!("the term must parse: {e}\n{term}"));

        assert!(term.contains("lookup!(\"rho:id:theKit\""), "{term}");
        assert!(
            term.contains("(_, _, arg0)"),
            "the pattern's leaf is renamed to the bound name: {term}"
        );
        // The capability is passed bare and the value as its literal, in argument order.
        assert!(term.contains("!(\"take\", arg0, \"plain\""), "{term}");
        // And the names it binds are declared, or the term is free-variable and unparseable.
        assert!(term.contains("arg0Ch"), "{term}");
    }

    #[test]
    fn invoke_term_parses() {
        let term = invoke_term(
            "rho:id:theOracle",
            "getPrice",
            &[Arg::Value(RhoString::apply("ETH".to_string()))],
        )
        .expect("a renderable argument");
        // Must parse + normalize as real rholang (catches syntax drift in the template).
        rchain_rholang::normalizer::source_to_adt(&term).expect("generated invoke term must parse");
    }

    #[test]
    fn invoke_term_targets_uri_method_and_deploy_id() {
        let term = invoke_term("rho:id:locker", "open", &[]).expect("no arguments to refuse");
        assert!(term.contains("rho:registry:lookup"), "{term}");
        assert!(term.contains("rho:id:locker"), "{term}");
        assert!(term.contains("\"open\""), "{term}");
        // The reply channel is the deploy id, never a caller-local name.
        assert!(term.contains(REMOTE_REPLY_CHANNEL), "{term}");
        // **And it must be *bound*, not written as a URI ground** (AUDIT C218): a backticked
        // `rho:rchain:deployId` is an ordinary guessable name that nobody reads, so the send goes
        // nowhere while the deploy reports success. Pinned here so it cannot come back silently.
        assert!(
            term.contains(&format!("deployId(`{REMOTE_REPLY_CHANNEL}`)")),
            "the reply channel must be introduced as a binding: {term}"
        );
        assert!(term.contains("*deployId"), "{term}");
    }

    /// **An argument that cannot be written as a literal is refused, not printed** (AUDIT C220).
    ///
    /// The bug this pins: `PrettyPrinter` writes a string as `"…"` with no escape, and Rholang's
    /// lexer reads to the next `"` — so **any** argument containing a double quote ends the literal
    /// early and whatever follows is read as part of the term this caller then signs. The whole
    /// trigger is the quote, so a plain sentence is a faithful and minimal vector: what matters is
    /// that the value is refused *before* it reaches the printer, whatever it contains.
    #[test]
    fn an_argument_that_would_close_its_literal_is_refused() {
        for argument in [
            "he said \"hello\"", // the minimal trigger: a quote, and nothing else
            "a\"b",              // and the same with no surrounding text
            "trailing quote\"",  // and at the end, where the literal would simply not close
        ] {
            let refused = invoke_term(
                "rho:rchain:revVault",
                "getBalance",
                &[Arg::Value(RhoString::apply(argument.to_string()))],
            );
            assert!(
                refused.is_err(),
                "an argument containing a quote must be refused, not printed into a signed term: \
                 {refused:?}"
            );
            let reason = refused.expect_err("checked");
            assert!(
                reason.contains("double quote") && reason.contains("argument"),
                "the refusal must name the argument and the reason: {reason}"
            );
        }

        // The method rides the same printer, so it is checked the same way.
        assert!(
            invoke_term("rho:rchain:revVault", "get\"Balance", &[]).is_err(),
            "a method name is printed into the term too"
        );

        // A value that *can* be written as a literal is still rendered, unchanged.
        let ok = invoke_term(
            "rho:rchain:revVault",
            "getBalance",
            &[Arg::Value(RhoString::apply(
                "11112VYAt8rUGNRRZX3eJdgagaAhtWTK8Js7F7X5iqddMVqyDTtYau".to_string(),
            ))],
        )
        .expect("an ordinary address renders");
        assert!(ok.contains("getBalance"), "{ok}");
    }

    /// The two source-literal shapes the printer must get right for a *parsed* term: a byte array
    /// (which a bare hex token is not) and a string.
    #[test]
    fn a_byte_array_argument_renders_as_its_source_literal() {
        let term = invoke_term(
            "rho:rchain:pos",
            "getDelegations",
            &[Arg::Value(RhoByteArray::apply(vec![0xAB, 0xCD]))],
        )
        .expect("a byte array is a value");
        assert!(term.contains("\"abcd\".hexToBytes()"), "{term}");
    }

    #[test]
    fn signed_invoke_is_signed_by_the_caller() {
        let caller = construct_deploy::default_sec();
        let caller_pub = Secp256k1.to_public(&caller).unwrap();

        let term = invoke_term("rho:id:x", "m", &[]).expect("no arguments to refuse");
        let deploy = signed_invoke(&term, &caller, 0, 90_000, 1, 0, "root").unwrap();

        // The far shard will bind this public key as `deployerId`.
        assert_eq!(deploy.pk, caller_pub);
        assert_eq!(deploy.data.term, term);
        assert_eq!(deploy.data.shard_id, "root");

        let serialized = <DeployData as Serialize<DeployData>>::encode(&deploy.data);
        let hash = signature_hash("secp256k1", &serialized);
        assert!(Secp256k1.verify(&hash, &deploy.sig, deploy.pk.bytes()));
    }

    #[test]
    fn reply_channel_is_the_deploy_id() {
        let sig = vec![7u8; 64];
        let channel = reply_channel(&sig);
        assert_eq!(RhoDeployId::unapply(&channel), Some(sig.as_slice()));
    }

    #[test]
    fn reply_outcome_reads_the_channel_value() {
        let reply = RhoString::apply("42".to_string());
        assert_eq!(
            reply_outcome(&[data_with(Some(reply.clone()))]),
            ShardOutcome::Value(reply)
        );
        assert!(matches!(
            reply_outcome(&[data_with(None)]),
            ShardOutcome::Error(_)
        ));
        assert!(matches!(reply_outcome(&[]), ShardOutcome::Error(_)));
    }

    #[test]
    fn failure_is_a_shard_error_tuple() {
        let value = ShardOutcome::Error("no route".to_string()).into_value();
        let tuple = RhoTupleN::unapply(&value).expect("shard-error is a tuple");
        assert_eq!(tuple.len(), 2);
        assert_eq!(RhoString::unapply(&tuple[0]), Some(SHARD_ERROR_TAG));
        assert_eq!(RhoString::unapply(&tuple[1]), Some("no route"));
    }
}
