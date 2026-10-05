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
//! **What is proven, and what is not.** A delivery *does* become a signed deploy, and the deploy
//! *does* land in a block — `node/tests/ocapn_listener.rs` asserts the deploy's own
//! `ProcessedWithSuccess` verdict from the node. The reply **value** does not arrive: the deploy
//! runs, succeeds, and its `deploy_result` is **empty**, so nothing reached
//! `` `rho:rchain:deployId` ``. **The cause is not established** — AUDIT C218 holds it, and the
//! first explanation there was wrong: [`invoke_term`]'s shape *is* right. A system process is
//! defined `arity: 1, remainder: true` (`rholang/src/system_processes.rs`), so a call's trailing
//! arguments are collected into the single list the handler destructures — the faucet's
//! five-argument `revVault!("transfer", …)` is the proof. Nothing is wrong with how the call is
//! *shaped*, so something between the registry lookup, the send, and the write is dropping the
//! reply.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use rchain_casper::api::block_api::BlockApi;
use rchain_casper::shard_invoke::{invoke_term, reply_outcome, signed_invoke, ShardOutcome};
use rchain_crypto::private_key::PrivateKey;
use rchain_models::casper::protocol::casper_message::SignedDeployData;
use rchain_ocapn::conn::{Act, Export, Identity, Session};
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
/// `chain` is the chain-backed capability to publish on each session's bootstrap, or `None` when
/// the node has no deployer key to sign with. It is shared across sessions: it holds no per-session
/// state.
pub async fn serve_ocapn(
    listen: Option<String>,
    chain: Option<Arc<dyn Export>>,
    stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let Some(listen) = listen else {
        stop_requested(stop).await;
        return Ok(());
    };
    let listener = TcpTestingOnly::bind(&listen)
        .await
        .map_err(|e| e.to_string())?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    let location = PeerLocator {
        designator: "rnode".to_string(),
        transport: "tcp-testing-only".to_string(),
        hints: BTreeMap::from([
            ("host".to_string(), local.ip().to_string()),
            ("port".to_string(), local.port().to_string()),
        ]),
    };

    loop {
        let connection = tokio::select! {
            // The operator's word, and a dropped coordinator, both land here.
            _ = stop_requested(stop.clone()) => return Ok(()),
            accepted = listener.accept_incoming_connection() => accepted.map_err(|e| e.to_string())?,
        };
        let location = location.clone();
        let chain = chain.clone();
        tokio::spawn(async move {
            // A fresh session key per session, as OCapN requires.
            let Ok(identity) = Identity::fresh(location) else {
                return;
            };
            // The fixtures are rebuilt per session (they hold per-session promise state); the
            // chain-backed capability is shared, because it does not.
            let mut bootstrap = fixtures::conformance_bootstrap();
            if let Some(chain) = chain {
                bootstrap.publish(REV_VAULT_BALANCE_SWISS.to_vec(), chain);
            }
            match Session::accept(connection, &identity, Arc::new(bootstrap)).await {
                Ok(mut session) => {
                    // One session per connection; its end is this task's end.
                    let _ = session.run().await;
                }
                Err(_) => {
                    // A refused handshake has already been answered with `op:abort` where one was
                    // owed; there is nothing further to say on the wire.
                }
            }
        });
    }
}

/// A chain-backed capability: a delivery to it becomes a caller-signed deploy, and the CapTP
/// promise resolves from the value that deploy put on its reply channel.
///
/// **The method comes from this object, not from the message.** An OCapN peer sends the method name
/// as a Syrup *Symbol* (it is Endo's calling convention), and the bridge's `Par` mapping refuses an
/// inbound Symbol — Rholang has no symbol for one to land in. So a capability is one (target,
/// method) pair, which is also the shape an ERTP issuer wants: `getBrand`, `makeEmptyPurse` and the
/// rest are separate capabilities, not tags on a message.
pub struct ChainCapability {
    block_api: Arc<dyn BlockApi>,
    /// The key that signs. Today it is the node's own dev deployer key, so a bridged deploy spends
    /// the node's REV and the far contract sees the *node* as the caller. Binding a CapTP session
    /// to a caller's secp256k1 identity is the identity work `docs/src/node/ocapn.md` lists as
    /// future; until then this is the node acting as itself.
    key: PrivateKey,
    shard_id: String,
    target_uri: String,
    method: String,
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
            method: "getBalance".to_string(),
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
        let pars = args
            .iter()
            .map(par_value::value_to_par)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let term = invoke_term(self.target_uri.as_str(), self.method.as_str(), &pars);
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
        Ok(Act::value(
            par_value::par_to_value(&reply).map_err(|e| e.to_string())?,
        ))
    }
}
