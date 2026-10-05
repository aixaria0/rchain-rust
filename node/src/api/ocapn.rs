//! The node's OCapN listener (issue #249).
//!
//! OCapN is the object-capability network an Agoric vat speaks: a peer dials, the two sides
//! handshake over a netlayer, and the peer works against objects the node exports. This module is
//! the node's end of that — a listener that serves a fresh session per connection — and the
//! **bridge**: a chain-backed capability whose deliveries become signed deploys.
//!
//! **The transport is `tcp-testing-only`**, which the OCapN project's own README flags as "HIGHLY
//! INSECURE — DO NOT USE IN PRODUCTION": plain TCP, no encryption, no authentication. The listener
//! is therefore off unless `api-server.ocapn-listen` names an address, and a node that is reachable
//! from anywhere it does not control should leave it unset.
//!
//! **The bridge reuses Layer 1 rather than re-implementing it.** `docs/src/node/shard-invoke.md`
//! established that a cross-shard call *is* a caller-signed deploy whose reply arrives on
//! `` `rho:rchain:deployId` ``; a CapTP delivery is the same object with a different caller, so this
//! builds the term with [`invoke_term`], signs it with [`signed_invoke`], submits it through
//! `BlockApi::deploy` exactly as the faucet does, and reads the reply with [`reply_outcome`].
//!
//! **The reply path is the whole round trip.** A delivery is built into a term, signed, submitted,
//! and the value the deployed contract writes to the deploy's reply channel comes back as the CapTP
//! promise's fulfilment: `node/tests/ocapn_listener.rs` reads a REV balance out of a block that way.
//! **The reply channel being *bound* rather than written as a bare URI is load-bearing** (AUDIT
//! C218): a backticked `` `rho:rchain:deployId` `` is an ordinary, guessable name that nothing
//! reads, while the unforgeable per-deploy channel is what a `deployId(`rho:rchain:deployId`)`
//! binding introduces. Writing the URI instead sent every reply into a channel nobody watched and
//! the deploy still reported success.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use rchain_casper::api::block_api::BlockApi;
use rchain_casper::shard_invoke::{
    invoke_member_term, invoke_term, reply_outcome, signed_invoke, ShardOutcome,
};
use rchain_crypto::private_key::PrivateKey;
use rchain_models::casper::protocol::casper_message::SignedDeployData;
use rchain_ocapn::conn::{Act, Export, Identity};
use rchain_ocapn::fixtures;
use rchain_ocapn::locator::PeerLocator;
use rchain_ocapn::netlayer::Netlayer;
use rchain_ocapn::par_value;
use rchain_ocapn::syrup::Value;
use rchain_ocapn::tcp_testing_only::TcpTestingOnly;
use rchain_shared::base16;
use tokio::sync::watch;

use crate::runtime::shutdown::stop_requested;

/// The swiss number of the chain-backed capability.
pub const REV_VAULT_BALANCE_SWISS: &[u8] = b"rho:rchain:revVault/getBalance";

/// The swiss number of the **ERTP object API** — the contract's own registry shorthand, looked up on
/// chain. Unlike the REV balance capability this one takes **the method from the message**, because
/// it is an object with arms (`makeIssuerKit`, `getRevIssuer`) and what it *returns* are capabilities
/// (a brand, a mint, an issuer), which is what issue #249's clause 4 asks a peer to be able to hold.
pub const ERTP_SWISS: &[u8] = b"rho:rchain:ertp";

/// The ERTP contract as a chain capability: every delivery names its own method, and a reply that
/// holds capabilities comes back as descriptors.
pub fn ertp_capability(
    block_api: Arc<dyn BlockApi>,
    key: PrivateKey,
    shard_id: String,
) -> ChainCapability {
    ChainCapability {
        block_api,
        key,
        shard_id,
        target_uri: "rho:rchain:ertp".to_string(),
        pattern: None,
        method: None,
        reply_timeout: CHAIN_REPLY_TIMEOUT,
        reply_interval: CHAIN_REPLY_INTERVAL,
    }
}

/// Phlo for a bridged deploy — the same budget the faucet and the 2PC coordinator use.
const CHAIN_PHLO_LIMIT: i64 = 1_000_000;
const CHAIN_PHLO_PRICE: i64 = 1;
/// How long a delivery waits for its deploy's reply, and how often it looks.
const CHAIN_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
const CHAIN_REPLY_INTERVAL: Duration = Duration::from_millis(250);
/// How far back to look for the reply datum, in blocks. Matches the socket-level callers.
const CHAIN_REPLY_DEPTH: i32 = 50;

/// Serve OCapN on `listen` (`host:port`) until the node is asked to stop.
///
/// `None` means the listener is not configured. The task is spawned either way — one that is simply
/// waiting on the stop word when there is nothing to serve — so the node's listener set stays
/// uniform and its `select!` does not need a second optional arm.
///
/// `chain` is the chain-backed capabilities to publish on each session's bootstrap — a swiss number
/// and the object it names — empty when the node has no deployer key to sign with. They are shared
/// across sessions: they hold no per-session state.
pub async fn serve_ocapn(
    listen: Option<String>,
    chain: Vec<(Vec<u8>, Arc<dyn Export>)>,
    stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let Some(listen) = listen else {
        stop_requested(stop).await;
        return Ok(());
    };
    let bound = TcpTestingOnly::bind(&listen)
        .await
        .map_err(|e| e.to_string())?;
    // `local_addr` belongs to the concrete netlayer, not the trait: take it before the Arc.
    let local = bound.local_addr().map_err(|e| e.to_string())?;
    let listener: Arc<dyn Netlayer> = Arc::new(bound);
    let location = PeerLocator {
        designator: "rnode".to_string(),
        transport: "tcp-testing-only".to_string(),
        hints: BTreeMap::from([
            ("host".to_string(), local.ip().to_string()),
            ("port".to_string(), local.port().to_string()),
        ]),
    };
    // One registry and one gift store for the node's whole OCapN surface: the crossed-hello rule
    // compares the sessions *this node* has with a peer, and a handoff is deposited on one session
    // and withdrawn on another.
    let registry = Arc::new(rchain_ocapn::owner::SessionRegistry::default());
    let handoffs = Arc::new(rchain_ocapn::handoff::Handoffs::default());

    loop {
        let connection = tokio::select! {
            // The operator's word, and a dropped coordinator, both land here.
            _ = stop_requested(stop.clone()) => return Ok(()),
            accepted = listener.accept_incoming_connection() => accepted.map_err(|e| e.to_string())?,
        };
        let location = location.clone();
        let chain = chain.clone();
        let netlayer = listener.clone();
        let registry = registry.clone();
        let handoffs = handoffs.clone();
        tokio::spawn(async move {
            // A fresh session key per session, as OCapN requires.
            let Ok(identity) = Identity::fresh(location.clone()) else {
                return;
            };
            // The per-session objects that dial find their session through this slot, which is
            // filled in once the session exists — `accept` needs the bootstrap before that.
            let slot = rchain_ocapn::owner::session_slot();
            // The fixtures are rebuilt per session (they hold per-session promise state); the
            // chain-backed capability is shared, because it does not.
            let mut bootstrap =
                fixtures::conformance_bootstrap_with(handoffs, slot.clone(), registry.clone());
            for (swiss, capability) in chain {
                bootstrap.publish(swiss, capability);
            }
            fixtures::publish_dialing_fixtures(
                &mut bootstrap,
                netlayer,
                location,
                registry.clone(),
                slot.clone(),
            );
            // **Book, then answer** — see `owner::accept_and_book`: a session has to be in the
            // registry before the peer can act on it, or a delivery that arrives on the first round
            // trip after our start-session (a handoff, a fetch) reaches an object that cannot find
            // the session it belongs to.
            let (handle, loop_, context, peer_location) =
                match rchain_ocapn::owner::accept_and_book(
                    connection,
                    &identity,
                    Arc::new(bootstrap),
                    &registry,
                )
                .await
                {
                    Ok(parts) => parts,
                    Err(_) => {
                        // A refused handshake, and a session that lost its crossing, have both
                        // already been answered with `op:abort` where one was owed.
                        return;
                    }
                };
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(context);
            // One session per connection; its end is this task's end.
            let _ = loop_.run().await;
            registry.forget(&peer_location, &handle.own_pi, handle.dialed);
        });
    }
}

/// A chain-backed capability: a delivery to it becomes a caller-signed deploy, and the CapTP
/// promise resolves from the value that deploy put on its reply channel.
///
/// **The method rides the message**, as the first Syrup *Symbol* of the delivery — Endo's own calling
/// convention. This used to be a fixed `(target, method)` pair, justified in this comment by the
/// claim that "`getBrand`, `makeEmptyPurse` and the rest are separate capabilities, not tags on a
/// message" — which `docs/src/node/ertp.md`'s object model refutes: an issuer that answers exactly
/// one method is not an issuer a peer can hold. A *published* capability may still fix its method
/// (`revVault.getBalance`), because that shape is pinned by tests and by the conformance suite.
pub struct ChainCapability {
    block_api: Arc<dyn BlockApi>,
    /// The key that signs. Today it is the node's own dev deployer key, so a bridged deploy spends
    /// the node's REV and the far contract sees the *node* as the caller. Binding a CapTP session
    /// to a caller's secp256k1 identity is the identity work `docs/src/node/ocapn.md` lists as
    /// future; until then this is the node acting as itself.
    key: PrivateKey,
    shard_id: String,
    /// The registry URI the object is reachable at: a contract's own URI, or the URI a *bridge
    /// deploy* registered its reply under (see [`invoke_member_term`]).
    target_uri: String,
    /// How to bind the object **inside** the registered value: `None` for a symbolic urn, where the
    /// registered value *is* the object, and otherwise the pattern the reply's own shape produced —
    /// an ERTP kit's `(brand, mint, issuer)` is the issuer under `(_, _, member)`.
    pattern: Option<String>,
    /// A fixed method, for the capabilities this node *publishes* (`revVault.getBalance`). `None`
    /// means the method is the first `Symbol` of each delivery — which is what an ERTP object, an
    /// object with several arms, requires.
    method: Option<String>,
    reply_timeout: Duration,
    reply_interval: Duration,
}

impl ChainCapability {
    /// The `revVault.getBalance` capability: one REV address in, one balance out.
    pub fn rev_vault_balance(
        block_api: Arc<dyn BlockApi>,
        key: PrivateKey,
        shard_id: String,
    ) -> ChainCapability {
        ChainCapability {
            block_api,
            key,
            shard_id,
            target_uri: "rho:rchain:revVault".to_string(),
            pattern: None,
            method: Some("getBalance".to_string()),
            reply_timeout: CHAIN_REPLY_TIMEOUT,
            reply_interval: CHAIN_REPLY_INTERVAL,
        }
    }

    /// Wait for the deploy's reply channel to carry a value.
    async fn await_reply(&self, deploy_id: &[u8]) -> Result<rchain_models::ast::Par, String> {
        let name = rchain_casper::shard_invoke::reply_channel(deploy_id);
        let deadline = Instant::now() + self.reply_timeout;
        loop {
            let (data, _length) = self
                .block_api
                .get_listening_name_data_response(CHAIN_REPLY_DEPTH, &name)
                .await?;
            if !data.is_empty() {
                return match reply_outcome(&data) {
                    ShardOutcome::Value(par) => Ok(par),
                    // A registry miss produces nothing on the channel; `reply_outcome` says so.
                    ShardOutcome::Error(reason) => Err(reason),
                };
            }
            if Instant::now() >= deadline {
                // Say *why* nothing arrived. `NotProcessed` means the deploy never reached a block;
                // `ProcessedWithError` means it ran and was refused — and a refused deploy is the
                // ordinary way a lookup or a guard fails, which the caller otherwise cannot tell
                // apart from a slow chain.
                let deploy_id_hex = base16::encode(deploy_id);
                let status = self.block_api.deploy_status(&deploy_id.to_vec()).await;
                return Err(format!(
                    "timed out waiting for deploy {deploy_id_hex} to reply ({status:?})"
                ));
            }
            tokio::time::sleep(self.reply_interval).await;
        }
    }
}

#[async_trait]
impl Export for ChainCapability {
    async fn deliver(&self, args: &[Value]) -> Result<Act, String> {
        // The arguments crossed CapTP as Syrup; Rholang is what the chain runs. A value with no
        // `Par` shape (a Symbol, a float) is refused here rather than guessed at.
        // **The method is the first `Symbol` of the delivery**, which is Endo's calling convention
        // and the only shape that can serve an ERTP object: an issuer has `getBrand`,
        // `makeEmptyPurse` and `getAmountOf`, so "one capability, one method" would be one capability
        // per arm — and a peer cannot hold *an issuer*. A published capability may still fix its
        // method (`revVault.getBalance`), because that shape is pinned by tests and by the suite.
        let (method, args) = match &self.method {
            Some(fixed) => (fixed.clone(), args),
            None => match args.split_first() {
                Some((Value::Symbol(m), rest)) => (m.clone(), rest),
                _ => {
                    return Err(
                        "a chain capability expects the method as the first Symbol".to_string()
                    )
                }
            },
        };
        let pars = args
            .iter()
            .map(par_value::value_to_par)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        // A capability whose method rides the message is one that may *return* capabilities, and a
        // returned capability has no rholang source literal — so the deploy registers its reply in
        // the same evaluation that produced it (`invoke_member_term`), and answers the deploy's reply
        // channel with `(uri, value)`: the fresh registry URI, and the value whose capabilities the
        // node then addresses *through* that URI by path.
        //
        // The deploy is signed by the node's own key in **both** shapes. A fresh key per object would
        // give the object a URI of its own under `insertSigned`, but it has no REV, so its deploy
        // cannot pay for itself (`preCharge: insufficient funds`) — measured, first run of this test.
        // `insertArbitrary` mints the URI without a key, so the funded key can sign.
        let registering = self.method.is_none();
        let term = if registering {
            invoke_member_term(
                self.target_uri.as_str(),
                self.pattern.as_deref(),
                &method,
                &pars,
            )
        } else {
            invoke_term(self.target_uri.as_str(), &method, &pars)
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // A deploy expires once `latest_block_number - valid_after_block_number > DEPLOY_LIFESPAN`,
        // so it is anchored to the current height rather than to `-1` (the faucet's note).
        let vabn = self.block_api.status().await.latest_block_number;
        // **The signed deploy must be dropped before the first `await`.** `Signed` borrows a
        // `&dyn SignaturesAlg`, which is not `Sync`, so holding one across an await makes this
        // future non-`Send` — and `Export::deliver`'s future must be `Send`. Everything needed
        // afterwards is owned by the end of this block.
        let (data, deployer, sig) = {
            let signed = signed_invoke(
                &term,
                &self.key,
                timestamp,
                CHAIN_PHLO_LIMIT,
                CHAIN_PHLO_PRICE,
                vabn,
                &self.shard_id,
            )?;
            (signed.data, signed.pk.bytes().to_vec(), signed.sig.clone())
        };
        let deploy = SignedDeployData {
            data,
            deployer,
            sig: sig.clone(),
            sig_algorithm: "secp256k1".to_string(),
        };
        // `ApiErr<String>` is a plain `String` error (the socket-level callers join a list).
        self.block_api.deploy(&deploy).await?;

        let reply = self.await_reply(&sig).await?;
        if !registering {
            return Ok(Act::value(
                par_value::par_to_value(&reply).map_err(|e| e.to_string())?,
            ));
        }
        // `(uri, value)` — the URI this reply was registered under, and the value itself.
        let (uri, value) = split_registered_reply(&reply)?;
        // The value may *hold* capabilities. Each becomes an export of its own, addressed by the URI
        // the deploy just registered plus the pattern that binds it inside that value — so a peer that
        // asked for a kit gets a descriptor per member, and a peer that asked for a purse gets one.
        let mut leaves = Vec::new();
        capability_patterns(&value, &mut leaves);
        if leaves.is_empty() {
            return Ok(Act::value(
                par_value::par_to_value(&value).map_err(|e| e.to_string())?,
            ));
        }
        let objects: Vec<Arc<dyn Export>> = leaves
            .into_iter()
            .map(|pattern| {
                Arc::new(ChainCapability {
                    block_api: self.block_api.clone(),
                    key: self.key.clone(),
                    shard_id: self.shard_id.clone(),
                    target_uri: uri.clone(),
                    pattern: Some(pattern),
                    method: None,
                    reply_timeout: self.reply_timeout,
                    reply_interval: self.reply_interval,
                }) as Arc<dyn Export>
            })
            .collect();
        // **One object answers as one descriptor, not as a one-element list.** The peer asked for a
        // purse or an issuer, and `E(obj).makeEmptyPurse()` should hand it a purse — a list of one
        // would make every caller destructure a collection to reach the only thing in it.
        //
        // Written as a split rather than a length check plus an `expect`, because `expect` in
        // production code is a hard violation of the type system's partiality gate — and because the
        // empty case is real enough to deserve a name: it cannot happen here (a leaf is what put the
        // pattern in the list), and if it ever did, a promise that names the reason and breaks is
        // better than a panicking node.
        let mut objects = objects.into_iter();
        let first = objects.next();
        let rest: Vec<Arc<dyn Export>> = objects.collect();
        match first {
            Some(only) if rest.is_empty() => Ok(Act::object(only)),
            Some(only) => Ok(Act::objects(
                std::iter::once(only).chain(rest).collect::<Vec<_>>(),
            )),
            None => Err("the reply registered no capability to hand the peer".to_string()),
        }
    }
}

/// Split `(uri, value)`, the reply a registering deploy puts on its reply channel.
///
/// The reply is refused rather than guessed at when it is not that pair: a build of
/// `invoke_member_term` that answered something else would otherwise be read as "no capabilities in
/// here", and the peer would get a value where it should have got an object.
fn split_registered_reply(
    reply: &rchain_models::ast::Par,
) -> Result<(String, rchain_models::ast::Par), String> {
    let Some(rchain_models::ast::Expr::ETuple(tuple)) = reply.exprs.first() else {
        return Err(format!(
            "a registering deploy answers `(uri, value)`, this one answered {reply:?}"
        ));
    };
    let [first, second] = tuple.ps.as_slice() else {
        return Err(format!(
            "a registering deploy answers a pair, this one answered {} element(s)",
            tuple.ps.len()
        ));
    };
    // A `rho:id:…` URI comes back as a **uri literal** (`GUri`) — `rho:registry:ops`'s `buildUri`
    // returns one — while a symbolic target like `rho:rchain:ertp` would be a string. Both name the
    // same registry key: `rho:registry:lookup` accepts either and reduces it to its text.
    match first.exprs.first() {
        Some(rchain_models::ast::Expr::GUri(uri) | rchain_models::ast::Expr::GString(uri)) => {
            Ok((uri.clone(), second.clone()))
        }
        other => Err(format!(
            "the registered URI is neither a uri nor a string: {other:?}"
        )),
    }
}

/// Every capability inside a reply, with the **pattern** that binds it in a later deploy: a name or a
/// bundle *is* a capability, and a tuple is searched element by element. Everything else is data.
///
/// **The pattern is written from the value's shape, not from a position path.** The obvious encoding
/// is a path — `[2]` for the third element — and [`invoke_member_term`] would then spell it
/// `(_, _, member)`. That only works if the pattern knows how many elements the tuple has, and
/// Rholang has no rest pattern for tuples: `(_, member)` does not match a 3-element tuple, it
/// matches *nothing*, and the deploy then answers nothing at all (measured — an ERTP kit, whose reply
/// is `(brand, mint, issuer)`, sat there until the bridge's timeout). So the arity at every level the
/// pattern crosses is taken from the value the reply carried, and the sibling positions — whatever
/// their number — are wildcarded.
fn capability_patterns(par: &rchain_models::ast::Par, out: &mut Vec<String>) {
    capability_patterns_at(par, "member", out)
}

/// The recursive half: `at` is the pattern naming the leaf, and every tuple on the way down replaces
/// it with a pattern that keeps only the branch leading to it.
fn capability_patterns_at(par: &rchain_models::ast::Par, at: &str, out: &mut Vec<String>) {
    if !par.unforgeables.is_empty() || !par.bundles.is_empty() {
        out.push(at.to_string());
        return;
    }
    for expr in &par.exprs {
        if let rchain_models::ast::Expr::ETuple(tuple) = expr {
            for (index, element) in tuple.ps.iter().enumerate() {
                let fields: Vec<String> = tuple
                    .ps
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        if i == index {
                            at.to_string()
                        } else {
                            "_".to_string()
                        }
                    })
                    .collect();
                capability_patterns_at(element, &format!("({})", fields.join(", ")), out);
            }
        }
    }
}
