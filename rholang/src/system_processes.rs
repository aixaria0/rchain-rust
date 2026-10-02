//! Built-in system contracts (port of `interpreter/SystemProcesses.scala`).
//!
//! `FixedChannels`/`BodyRefs` are the byte channels and dispatch-table ids; [`SystemProcesses`]
//! builds the `ScalaBodyFn` handlers (stdout/stderr, crypto verify/hash, block data, REV address,
//! deployer-id ops, registry ops, sys-auth-token ops) that the runtime installs and dispatches to.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

use qucalc::{achieves_zfa, dialectical_synthesis, pauli_phase};
use rchain_crypto::hash::{blake2b256, keccak256, sha256};
use rchain_crypto::public_key::PublicKey;
use rchain_crypto::signatures::ed25519::Ed25519;
use rchain_crypto::signatures::secp256k1::Secp256k1;
use rchain_models::ast::Par;
use rchain_models::casper::protocol::casper_message::BlockMessage;
use rchain_models::rholang::RhoType::{
    RhoBoolean, RhoByteArray, RhoDeployerId, RhoList, RhoMap, RhoName, RhoNil, RhoNumber, RhoSet,
    RhoString, RhoSysAuthToken, RhoTupleN, RhoUri,
};
use rchain_models::runtime::ListParWithRandom;
use rchain_models::validator::Validator;
use rchain_shared::refined::{BlockHeight, NonNegI64, SeqNum};

use crate::contract_call::ContractCall;
use crate::dispatch::{RholangAndScalaDispatcher, ScalaBodyFn};
use crate::errors::RholangError;
use crate::native_state::{NativeSystemState, TxnState};
use crate::pretty_printer::PrettyPrinter;
use crate::reduce::{Dispatch, Tuplespace};
use crate::registry;
use crate::scheduler::DfsPath;
use crate::storage::ChargingRSpace;
use crate::util::rev_address::RevAddress;

/// A byte-name channel (port of `SystemProcesses.byteName`): `GPrivate(<single byte>)`.
pub fn byte_name(b: u8) -> Par {
    RhoName::apply_bytes(vec![b])
}

/// The nonce a system contract is registered with (the Scala `Long.MaxValue`): consumers destructure
/// the registry reply as `(nonce, value)` and ignore this element.
pub const SYSTEM_CONTRACT_NONCE: i64 = i64::MAX;

/// The shorthands that must resolve through `rho:registry:lookup` to a **native system channel**.
///
/// Each names an arity-1 channel that `definitions()` already installs as a continuation; what is
/// missing on a fresh chain is only the *registry alias*, so that `lookup!(\`rho:rchain:pos\`, *ch)`
/// finds something instead of answering `Nil`. Kept as a table (rather than a lookup inside
/// `definitions()`) so the genesis path can seed it without a `SystemProcesses` instance; a test
/// asserts every urn here is a real definition with the channel this module maps it to.
pub const SYSTEM_CHANNEL_ALIAS_URNS: &[&str] = &[
    "rho:rchain:pos",
    "rho:rchain:revVault",
    "rho:rchain:multiSigRevVault",
];

/// The registry value a system-channel shorthand resolves to: `(nonce, bundle+{channel})` — the
/// shape `rho:registry:insertSigned:secp256k1` stores, which is what every consumer destructures
/// (`for (@(_, PoS) <- ch)`). `None` for a urn that is not one of [`SYSTEM_CHANNEL_ALIAS_URNS`].
pub fn system_channel_alias(urn: &str) -> Option<Par> {
    let channel = match urn {
        "rho:rchain:pos" => FixedChannels::pos(),
        "rho:rchain:revVault" => FixedChannels::rev_vault(),
        "rho:rchain:multiSigRevVault" => FixedChannels::multi_sig_rev_vault(),
        _ => return None,
    };
    Some(RhoTupleN::apply(vec![
        RhoNumber::apply(SYSTEM_CONTRACT_NONCE),
        crate::runtime::write_bundle(channel),
    ]))
}

/// The fixed system channels (port of `SystemProcesses.FixedChannels`).
pub struct FixedChannels;
impl FixedChannels {
    pub fn stdout() -> Par {
        byte_name(0)
    }
    pub fn stdout_ack() -> Par {
        byte_name(1)
    }
    pub fn stderr() -> Par {
        byte_name(2)
    }
    pub fn stderr_ack() -> Par {
        byte_name(3)
    }
    pub fn ed25519_verify() -> Par {
        byte_name(4)
    }
    pub fn sha256_hash() -> Par {
        byte_name(5)
    }
    pub fn keccak256_hash() -> Par {
        byte_name(6)
    }
    pub fn blake2b256_hash() -> Par {
        byte_name(7)
    }
    pub fn secp256k1_verify() -> Par {
        byte_name(8)
    }
    pub fn get_block_data() -> Par {
        byte_name(10)
    }
    pub fn get_invalid_blocks() -> Par {
        byte_name(11)
    }
    pub fn rev_address() -> Par {
        byte_name(12)
    }
    pub fn deployer_id_ops() -> Par {
        byte_name(13)
    }
    pub fn reg_lookup() -> Par {
        byte_name(14)
    }
    pub fn reg_insert_random() -> Par {
        byte_name(15)
    }
    pub fn reg_insert_signed() -> Par {
        byte_name(16)
    }
    pub fn reg_ops() -> Par {
        byte_name(17)
    }
    pub fn sys_auth_token_ops() -> Par {
        byte_name(18)
    }
    pub fn pos() -> Par {
        byte_name(19)
    }
    pub fn rev_vault() -> Par {
        byte_name(20)
    }
    pub fn multi_sig_rev_vault() -> Par {
        byte_name(21)
    }
    pub fn qucalc_zfa() -> Par {
        byte_name(22)
    }
    pub fn qucalc_grant() -> Par {
        byte_name(23)
    }
    pub fn qucalc_verify() -> Par {
        byte_name(24)
    }
    pub fn qucalc_fuse() -> Par {
        byte_name(25)
    }
    pub fn gov_resolve_weights() -> Par {
        byte_name(26)
    }
    pub fn gov_trust_levels() -> Par {
        byte_name(27)
    }
    pub fn gov_censure() -> Par {
        byte_name(28)
    }
    pub fn gov_tally() -> Par {
        byte_name(29)
    }
    pub fn txn() -> Par {
        byte_name(30)
    }
    pub fn http() -> Par {
        byte_name(31)
    }
}

/// The dispatch-table ids (port of `SystemProcesses.BodyRefs`).
pub struct BodyRefs;
impl BodyRefs {
    pub const STDOUT: i64 = 0;
    pub const STDOUT_ACK: i64 = 1;
    pub const STDERR: i64 = 2;
    pub const STDERR_ACK: i64 = 3;
    pub const ED25519_VERIFY: i64 = 4;
    pub const SHA256_HASH: i64 = 5;
    pub const KECCAK256_HASH: i64 = 6;
    pub const BLAKE2B256_HASH: i64 = 7;
    pub const SECP256K1_VERIFY: i64 = 9;
    pub const GET_BLOCK_DATA: i64 = 11;
    pub const GET_INVALID_BLOCKS: i64 = 12;
    pub const REV_ADDRESS: i64 = 13;
    pub const DEPLOYER_ID_OPS: i64 = 14;
    pub const REG_OPS: i64 = 15;
    pub const SYS_AUTHTOKEN_OPS: i64 = 16;
    pub const REG_LOOKUP: i64 = 17;
    pub const REG_INSERT_RANDOM: i64 = 18;
    pub const REG_INSERT_SIGNED: i64 = 19;
    pub const POS: i64 = 20;
    pub const REV_VAULT: i64 = 21;
    pub const MULTI_SIG_REV_VAULT: i64 = 22;
    pub const QUCALC_ZFA: i64 = 23;
    pub const QUCALC_GRANT: i64 = 24;
    pub const QUCALC_VERIFY: i64 = 25;
    pub const QUCALC_FUSE: i64 = 26;
    pub const GOV_RESOLVE_WEIGHTS: i64 = 27;
    pub const GOV_TRUST_LEVELS: i64 = 28;
    pub const GOV_CENSURE: i64 = 29;
    pub const GOV_TALLY: i64 = 30;
    pub const TXN: i64 = 31;
    pub const HTTP: i64 = 32;
}

/// Per-block data exposed to the `rho:block:data` contract (port of `SystemProcesses.BlockData`).
#[derive(Clone, Debug)]
pub struct BlockData {
    pub block_number: BlockHeight,
    pub sender: PublicKey,
    pub seq_num: SeqNum,
    /// The block's informational timestamp (proposer's wall clock, ms since the Unix epoch). It is
    /// not a consensus input; it is exposed here for RChain applications.
    pub timestamp: i64,
}

impl BlockData {
    pub fn empty() -> Self {
        BlockData {
            block_number: BlockHeight::zero(),
            sender: PublicKey::new(vec![0]),
            seq_num: SeqNum::zero(),
            timestamp: 0,
        }
    }

    /// Build the per-block data from a block message (port of `BlockData.fromBlock`).
    pub fn from_block(block: &BlockMessage) -> Self {
        BlockData {
            block_number: block.block_number,
            sender: PublicKey::new(block.sender.as_bytes().to_vec()),
            seq_num: block.seq_num,
            timestamp: block.timestamp,
        }
    }
}

/// A system-contract definition: urn + fixed channel + arity + dispatch id + handler.
pub struct Definition {
    pub urn: String,
    pub fixed_channel: Par,
    pub arity: i32,
    /// Whether the last argument is a remainder (variable-arity method dispatch).
    pub remainder: bool,
    pub body_ref: i64,
    pub handler: ScalaBodyFn,
}

fn illegal_arg(msg: &str) -> RholangError {
    RholangError::ReduceError(msg.to_string())
}

/// Parse a rholang list of integers 0..7 into a twist sequence.
fn parse_twists(p: &Par) -> Result<Vec<u8>, RholangError> {
    RhoList::unapply(p)
        .and_then(|ps| {
            ps.iter()
                .map(|q| {
                    RhoNumber::unapply(q)
                        .and_then(|n| u8::try_from(n).ok())
                        .filter(|v| *v <= 7)
                })
                .collect::<Option<Vec<u8>>>()
        })
        .ok_or_else(|| illegal_arg("expected a list of twist values 0..7"))
}

// --- Governance parsing helpers -------------------------------------------
// These decode the rholang wire forms accepted by the `rho:gov:*` processes.
//
// A *member id* is either a plain string or a deployer-id unforgeable; the
// latter is canonicalized to the base16 encoding of its public key, so the same
// deployer always maps to the same id (unforgeable identity, deterministic
// ordering). The envelope layer binds the signer to `*deployerId`, so a member
// cannot spoof another member's id.

/// Canonical member id: a string, or a deployer-id unforgeable (hex of its public key).
///
/// NB: the two namespaces share one string domain — a plain string member id equal to
/// the base16 hex of someone's public key would collide with that unforgeable id. In
/// practice the envelope layer binds the signer to `*deployerId`, so a member cannot
/// *impersonate* another; this is only a shared-namespace caveat, not a spoofing vector.
fn member_id(p: &Par) -> Option<String> {
    RhoString::unapply(p)
        .map(|s| s.to_string())
        .or_else(|| RhoDeployerId::unapply(p).map(rchain_shared::base16::encode))
}

fn parse_string_list(p: &Par) -> Result<Vec<String>, RholangError> {
    RhoList::unapply(p)
        .and_then(|ps| {
            ps.iter()
                .map(|q| RhoString::unapply(q).map(|s| s.to_string()))
                .collect()
        })
        .ok_or_else(|| illegal_arg("expected a list of strings"))
}

fn parse_member_list(p: &Par) -> Result<Vec<String>, RholangError> {
    RhoList::unapply(p)
        .and_then(|ps| ps.iter().map(member_id).collect())
        .ok_or_else(|| illegal_arg("expected a list of member ids (string or deployerId)"))
}

fn parse_member_map(p: &Par) -> Result<BTreeMap<String, String>, RholangError> {
    RhoMap::unapply(p)
        .and_then(|kvs| {
            kvs.iter()
                .map(|(k, v)| Some((member_id(k)?, member_id(v)?)))
                .collect()
        })
        .ok_or_else(|| illegal_arg("expected a map of member id -> member id"))
}

fn parse_member_int_map(p: &Par) -> Result<BTreeMap<String, i64>, RholangError> {
    RhoMap::unapply(p)
        .and_then(|kvs| {
            kvs.iter()
                .map(|(k, v)| Some((member_id(k)?, RhoNumber::unapply(v)?)))
                .collect()
        })
        .ok_or_else(|| illegal_arg("expected a map of member id -> int"))
}

fn parse_rating_list(p: &Par) -> Result<Vec<(String, String, i64)>, RholangError> {
    RhoList::unapply(p)
        .and_then(|ps| {
            ps.iter()
                .map(|q| {
                    let t = RhoTupleN::unapply(q)?;
                    if t.len() != 3 {
                        return None;
                    }
                    Some((
                        member_id(&t[0])?,
                        member_id(&t[1])?,
                        RhoNumber::unapply(&t[2])?,
                    ))
                })
                .collect()
        })
        .ok_or_else(|| illegal_arg("expected a list of (rater, ratee, level) tuples"))
}

fn parse_censure_list(p: &Par) -> Result<Vec<(String, String)>, RholangError> {
    RhoList::unapply(p)
        .and_then(|ps| {
            ps.iter()
                .map(|q| {
                    let t = RhoTupleN::unapply(q)?;
                    if t.len() != 2 {
                        return None;
                    }
                    Some((member_id(&t[0])?, member_id(&t[1])?))
                })
                .collect()
        })
        .ok_or_else(|| illegal_arg("expected a list of (censor, target) tuples"))
}

fn parse_voucher_list(p: &Par) -> Result<Vec<(String, String, i64)>, RholangError> {
    // Same shape as ratings: (voucher, vouchee, staked level).
    parse_rating_list(p)
}

fn parse_ranked_ballots(p: &Par) -> Result<BTreeMap<String, Vec<String>>, RholangError> {
    RhoMap::unapply(p)
        .and_then(|kvs| {
            kvs.iter()
                .map(|(k, v)| {
                    let member = member_id(k)?;
                    let ranking = parse_string_list(v).ok()?;
                    Some((member, ranking))
                })
                .collect()
        })
        .ok_or_else(|| illegal_arg("expected a map of member -> ranked options"))
}

fn member_int_map(m: &BTreeMap<String, i64>) -> Par {
    RhoMap::apply(
        m.iter()
            .map(|(k, v)| (RhoString::apply(k.clone()), RhoNumber::apply(*v)))
            .collect(),
    )
}

fn string_list(ss: &[String]) -> Par {
    RhoList::apply(ss.iter().map(|s| RhoString::apply(s.clone())).collect())
}

/// The system-process context (port of `SystemProcesses[F]`).
pub struct SystemProcesses {
    contract_call: ContractCall<ChargingRSpace, Weak<RholangAndScalaDispatcher>>,
    pretty_printer: PrettyPrinter,
    block_data: Arc<Mutex<BlockData>>,
    native_state: Arc<NativeSystemState>,
}

/// Install both arities of a **vault handle** on `name_bytes` and return the channel.
///
/// Arity 2 is the oracle's `@"balance", ret` and arity 5 its
/// `@"transfer", @targetAddress, @amount, authKey, ret` (`RevVault.rho:196-200`). A native handler has
/// one arity and RSpace matches on arity, so serving that API from Rust means two continuations under
/// one name — which is why the dispatch id is derived from the name *and* the arity
/// (`ContractCall::native_body_ref`).
///
/// The channel is built here rather than taken from `install_native`'s return so both arities are
/// known to have landed before the caller is handed the capability; a handle that resolves for
/// `balance` but not `transfer` would be worse than no handle.
async fn install_vault_handle<T, D>(
    cc: ContractCall<T, D>,
    native: Arc<NativeSystemState>,
    name_bytes: Vec<u8>,
    address: String,
) -> Result<Par, RholangError>
where
    T: Tuplespace + Clone + Send + Sync + 'static,
    D: Dispatch + Clone + Send + Sync + 'static,
{
    for (arity, handler) in [
        (
            2,
            vault_balance_handler(cc.clone(), native.clone(), address.clone()),
        ),
        (
            5,
            vault_transfer_handler(cc.clone(), native.clone(), address.clone()),
        ),
    ] {
        cc.install_native(name_bytes.clone(), arity, handler)
            .await?;
    }
    Ok(RhoName::apply_bytes(name_bytes))
}

/// A vault handle's `balance` arm: replies the balance **directly**, as the oracle does
/// (`revVault(@"balance", ret)` → `purse!("getBalance", *ret)`, and the wallet vector destructures a
/// bare balance rather than an `Either`).
fn vault_balance_handler<T, D>(
    cc: ContractCall<T, D>,
    native: Arc<NativeSystemState>,
    address: String,
) -> ScalaBodyFn
where
    T: Tuplespace + Clone + Send + Sync + 'static,
    D: Dispatch + Clone + Send + Sync + 'static,
{
    Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
        let cc = cc.clone();
        let native = native.clone();
        let address = address.clone();
        Box::pin(async move {
            let (pars, rand) = cc
                .unapply(&args)
                .ok_or_else(|| illegal_arg("a vault handle expects a method and arguments"))?;
            let [op, ret] = pars.as_slice() else {
                return Err(illegal_arg("vault balance expects a return channel"));
            };
            let op = RhoString::unapply(op)
                .ok_or_else(|| illegal_arg("a vault method must be a string"))?;
            // RSpace matched on *arity*, not on the method string, so the method is checked here —
            // which is what the oracle's `contract v(@"balance", ret)` does in its pattern. A send
            // that carries the wrong method at the right arity is answered with an error rather than
            // left pending: a caller who is told is better off than one who waits.
            if op != "balance" {
                return Err(illegal_arg(&format!(
                    "vault handle: {op} is not a method of this arity"
                )));
            }
            let balance = native
                .vault_balance(&address)
                .await
                .map_err(|e| illegal_arg(&e))?
                .unwrap_or(NonNegI64::zero());
            cc.produce(&rand, &[RhoNumber::apply(i64::from(balance))], ret, path)
                .await
        })
    })
}

/// A vault handle's `transfer` arm: `(true, Nil)` on success, `(false, reason)` on a refusal — the
/// `Either` shape both wallet vectors destructure.
///
/// **The authority is the name, not a key.** The oracle checks an `AuthKey` whose shape is the vault's
/// own address (`RevVault.rho:257`); here the presented value must *resolve to this vault's address*,
/// which is the same rule expressed in the encoding this port has. It is deliberately not "the
/// caller's `deployerId`": a contract holding this handle has no deployer key of its own, which is the
/// whole point of a capability, and it is what the multi-signature vault needs.
fn vault_transfer_handler<T, D>(
    cc: ContractCall<T, D>,
    native: Arc<NativeSystemState>,
    address: String,
) -> ScalaBodyFn
where
    T: Tuplespace + Clone + Send + Sync + 'static,
    D: Dispatch + Clone + Send + Sync + 'static,
{
    Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
        let cc = cc.clone();
        let native = native.clone();
        let address = address.clone();
        Box::pin(async move {
            let (pars, rand) = cc
                .unapply(&args)
                .ok_or_else(|| illegal_arg("a vault handle expects a method and arguments"))?;
            let [op, to, amount, auth, ret] = pars.as_slice() else {
                return Err(illegal_arg(
                    "vault transfer expects a target, an amount, an auth key and a return channel",
                ));
            };
            let op = RhoString::unapply(op)
                .ok_or_else(|| illegal_arg("a vault method must be a string"))?;
            if op != "transfer" {
                return Err(illegal_arg(&format!(
                    "vault handle: {op} is not a method of this arity"
                )));
            }
            let to = RhoString::unapply(to)
                .ok_or_else(|| illegal_arg("transfer expects a string to-address"))?;
            let amount = RhoNumber::unapply(amount)
                .ok_or_else(|| illegal_arg("transfer expects a number amount"))?;
            let amount = NonNegI64::try_from(amount).map_err(|e| illegal_arg(&e.to_string()))?;

            // **Two authorities, and a handle is neither.** A `deployerId` authorises the vault at
            // its own address (the classic rule, reused here rather than re-spelled); a *name*
            // authorises what `unforgeableAuthKey` recorded for it — the authority map, **not** the
            // handle map. Reading the handle map here would make `findOrCreate(victim_address)` a
            // spend right over that vault, which is the hole the two maps exist to keep apart.
            let authorised = match RhoDeployerId::unapply(auth) {
                Some(id) => RevAddress::from_deployer_id(id)
                    .map(|a| a.to_base58())
                    .is_some_and(|resolved| resolved == address),
                None => match RhoName::unapply(auth) {
                    Some(presented) => native
                        .vault_authority_address(&presented.id)
                        .await
                        .map_err(|e| illegal_arg(&e))?
                        .is_some_and(|resolved| resolved == address),
                    None => false,
                },
            };
            if !authorised {
                let out = RhoTupleN::apply(vec![
                    RhoBoolean::apply(false),
                    RhoString::apply("Invalid AuthKey".to_string()),
                ]);
                return cc.produce(&rand, &[out], ret, path).await;
            }

            let outcome = native
                .transfer_vault(&address, to, amount)
                .await
                .map_err(|e| illegal_arg(&e))?;
            let out = match outcome {
                Ok(()) => RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()]),
                Err(reason) => {
                    RhoTupleN::apply(vec![RhoBoolean::apply(false), RhoString::apply(reason)])
                }
            };
            cc.produce(&rand, &[out], ret, path).await
        })
    })
}

impl SystemProcesses {
    pub fn new(
        space: ChargingRSpace,
        dispatcher: Arc<RholangAndScalaDispatcher>,
        block_data: Arc<Mutex<BlockData>>,
        native_state: Arc<NativeSystemState>,
    ) -> Self {
        SystemProcesses {
            // Weak: the dispatch table lives inside the dispatcher, and each handler holds a
            // `ContractCall`. A strong self-reference there would keep the dispatcher (and with it
            // the whole forked runtime/hot store) alive forever (issues #18/#23).
            contract_call: ContractCall::new(
                space.cost().clone(),
                space,
                Arc::downgrade(&dispatcher),
            ),
            pretty_printer: PrettyPrinter::new(),
            block_data,
            native_state,
        }
    }

    /// The ordered list of standard system contracts (port of `stdSystemProcesses` +
    /// `stdRhoCryptoProcesses`).
    pub fn definitions(&self) -> Vec<Definition> {
        vec![
            Definition {
                urn: "rho:io:stdout".to_string(),
                fixed_channel: FixedChannels::stdout(),
                arity: 1,
                remainder: false,
                body_ref: BodyRefs::STDOUT,
                handler: self.stdout(),
            },
            Definition {
                urn: "rho:io:stdoutAck".to_string(),
                fixed_channel: FixedChannels::stdout_ack(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::STDOUT_ACK,
                handler: self.stdout_ack(),
            },
            Definition {
                urn: "rho:io:stderr".to_string(),
                fixed_channel: FixedChannels::stderr(),
                arity: 1,
                remainder: false,
                body_ref: BodyRefs::STDERR,
                handler: self.stderr(),
            },
            Definition {
                urn: "rho:io:stderrAck".to_string(),
                fixed_channel: FixedChannels::stderr_ack(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::STDERR_ACK,
                handler: self.stderr_ack(),
            },
            Definition {
                urn: "rho:block:data".to_string(),
                fixed_channel: FixedChannels::get_block_data(),
                arity: 1,
                remainder: false,
                body_ref: BodyRefs::GET_BLOCK_DATA,
                handler: self.get_block_data(),
            },
            Definition {
                urn: "rho:rev:address".to_string(),
                fixed_channel: FixedChannels::rev_address(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::REV_ADDRESS,
                handler: self.rev_address(),
            },
            Definition {
                urn: "rho:rchain:deployerId:ops".to_string(),
                fixed_channel: FixedChannels::deployer_id_ops(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::DEPLOYER_ID_OPS,
                handler: self.deployer_id_ops(),
            },
            Definition {
                urn: "rho:registry:ops".to_string(),
                fixed_channel: FixedChannels::reg_ops(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::REG_OPS,
                handler: self.registry_ops(),
            },
            Definition {
                urn: "sys:authToken:ops".to_string(),
                fixed_channel: FixedChannels::sys_auth_token_ops(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::SYS_AUTHTOKEN_OPS,
                handler: self.sys_auth_token_ops(),
            },
            Definition {
                urn: "rho:registry:lookup".to_string(),
                fixed_channel: FixedChannels::reg_lookup(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::REG_LOOKUP,
                handler: self.registry_lookup(),
            },
            Definition {
                urn: "rho:registry:insertArbitrary".to_string(),
                fixed_channel: FixedChannels::reg_insert_random(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::REG_INSERT_RANDOM,
                handler: self.registry_insert_arbitrary(),
            },
            Definition {
                urn: "rho:registry:insertSigned:secp256k1".to_string(),
                fixed_channel: FixedChannels::reg_insert_signed(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::REG_INSERT_SIGNED,
                handler: self.registry_insert_signed(),
            },
            Definition {
                urn: "rho:rchain:pos".to_string(),
                fixed_channel: FixedChannels::pos(),
                arity: 1,
                remainder: true,
                body_ref: BodyRefs::POS,
                handler: self.pos(),
            },
            Definition {
                urn: "rho:rchain:revVault".to_string(),
                fixed_channel: FixedChannels::rev_vault(),
                arity: 1,
                remainder: true,
                body_ref: BodyRefs::REV_VAULT,
                handler: self.rev_vault(),
            },
            Definition {
                urn: "rho:rchain:multiSigRevVault".to_string(),
                fixed_channel: FixedChannels::multi_sig_rev_vault(),
                arity: 1,
                remainder: true,
                body_ref: BodyRefs::MULTI_SIG_REV_VAULT,
                // **Not `self.rev_vault()`** (AUDIT C114). Sharing the single-signer handler made
                // this name a silent downgrade: a caller asking for multi-signature custody received
                // single-key custody with no indication. See `multi_sig_rev_vault`.
                handler: self.multi_sig_rev_vault(),
            },
            Definition {
                urn: "rho:crypto:secp256k1Verify".to_string(),
                fixed_channel: FixedChannels::secp256k1_verify(),
                arity: 4,
                remainder: false,
                body_ref: BodyRefs::SECP256K1_VERIFY,
                handler: self.secp256k1_verify(),
            },
            Definition {
                urn: "rho:crypto:blake2b256Hash".to_string(),
                fixed_channel: FixedChannels::blake2b256_hash(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::BLAKE2B256_HASH,
                handler: self.blake2b256_hash(),
            },
            Definition {
                urn: "rho:crypto:keccak256Hash".to_string(),
                fixed_channel: FixedChannels::keccak256_hash(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::KECCAK256_HASH,
                handler: self.keccak256_hash(),
            },
            Definition {
                urn: "rho:crypto:sha256Hash".to_string(),
                fixed_channel: FixedChannels::sha256_hash(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::SHA256_HASH,
                handler: self.sha256_hash(),
            },
            Definition {
                urn: "rho:crypto:ed25519Verify".to_string(),
                fixed_channel: FixedChannels::ed25519_verify(),
                arity: 4,
                remainder: false,
                body_ref: BodyRefs::ED25519_VERIFY,
                handler: self.ed25519_verify(),
            },
            Definition {
                urn: "rho:qucalc:zfa".to_string(),
                fixed_channel: FixedChannels::qucalc_zfa(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::QUCALC_ZFA,
                handler: self.qucalc_zfa(),
            },
            Definition {
                urn: "rho:qucalc:grant".to_string(),
                fixed_channel: FixedChannels::qucalc_grant(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::QUCALC_GRANT,
                handler: self.qucalc_grant(),
            },
            Definition {
                urn: "rho:qucalc:verify".to_string(),
                fixed_channel: FixedChannels::qucalc_verify(),
                arity: 2,
                remainder: false,
                body_ref: BodyRefs::QUCALC_VERIFY,
                handler: self.qucalc_verify(),
            },
            Definition {
                urn: "rho:qucalc:fuse".to_string(),
                fixed_channel: FixedChannels::qucalc_fuse(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::QUCALC_FUSE,
                handler: self.qucalc_fuse(),
            },
            Definition {
                urn: "rho:gov:resolveWeights".to_string(),
                fixed_channel: FixedChannels::gov_resolve_weights(),
                arity: 4,
                remainder: false,
                body_ref: BodyRefs::GOV_RESOLVE_WEIGHTS,
                handler: self.gov_resolve_weights(),
            },
            Definition {
                urn: "rho:gov:trustLevels".to_string(),
                fixed_channel: FixedChannels::gov_trust_levels(),
                arity: 3,
                remainder: false,
                body_ref: BodyRefs::GOV_TRUST_LEVELS,
                handler: self.gov_trust_levels(),
            },
            Definition {
                urn: "rho:gov:censure".to_string(),
                fixed_channel: FixedChannels::gov_censure(),
                arity: 4,
                remainder: false,
                body_ref: BodyRefs::GOV_CENSURE,
                handler: self.gov_censure(),
            },
            Definition {
                urn: "rho:gov:tally".to_string(),
                fixed_channel: FixedChannels::gov_tally(),
                arity: 4,
                remainder: false,
                body_ref: BodyRefs::GOV_TALLY,
                handler: self.gov_tally(),
            },
            Definition {
                urn: "rho:txn".to_string(),
                fixed_channel: FixedChannels::txn(),
                arity: 1,
                remainder: true,
                body_ref: BodyRefs::TXN,
                handler: self.txn(),
            },
            Definition {
                urn: "rho:io:http".to_string(),
                fixed_channel: FixedChannels::http(),
                arity: 1,
                remainder: true,
                body_ref: BodyRefs::HTTP,
                handler: self.http(),
            },
        ]
    }

    // --- io ------------------------------------------------------------

    fn stdout(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let pp = self.pretty_printer.clone();
        Box::new(move |args: Vec<ListParWithRandom>, _path: DfsPath| {
            let cc = cc.clone();
            let pp = pp.clone();
            Box::pin(async move {
                let (pars, _) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("stdout expects one argument"))?;
                match pars.as_slice() {
                    [arg] => {
                        println!("{}", pp.build_string(arg));
                        Ok(())
                    }
                    _ => Err(illegal_arg("stdout expects one argument")),
                }
            })
        })
    }

    fn stdout_ack(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let pp = self.pretty_printer.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let pp = pp.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("stdoutAck expects two arguments"))?;
                match pars.as_slice() {
                    [arg, ack] => {
                        println!("{}", pp.build_string(arg));
                        cc.produce(&rand, &[Par::default()], ack, path).await
                    }
                    _ => Err(illegal_arg("stdoutAck expects two arguments")),
                }
            })
        })
    }

    fn stderr(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let pp = self.pretty_printer.clone();
        Box::new(move |args: Vec<ListParWithRandom>, _path: DfsPath| {
            let cc = cc.clone();
            let pp = pp.clone();
            Box::pin(async move {
                let (pars, _) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("stderr expects one argument"))?;
                match pars.as_slice() {
                    [arg] => {
                        eprintln!("{}", pp.build_string(arg));
                        Ok(())
                    }
                    _ => Err(illegal_arg("stderr expects one argument")),
                }
            })
        })
    }

    fn stderr_ack(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let pp = self.pretty_printer.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let pp = pp.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("stderrAck expects two arguments"))?;
                match pars.as_slice() {
                    [arg, ack] => {
                        eprintln!("{}", pp.build_string(arg));
                        cc.produce(&rand, &[Par::default()], ack, path).await
                    }
                    _ => Err(illegal_arg("stderrAck expects two arguments")),
                }
            })
        })
    }

    // --- crypto --------------------------------------------------------

    fn verify_signature_contract(
        &self,
        name: &'static str,
        algorithm: fn(&[u8], &[u8], &[u8]) -> bool,
    ) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg(&format!(
                        "{name} expects data, signature, public key (all as byte arrays), and an acknowledgement channel"
                    ))
                })?;
                match pars.as_slice() {
                    [data, signature, pub_key, ack] => {
                        let (Some(d), Some(s), Some(p)) = (
                            RhoByteArray::unapply(data),
                            RhoByteArray::unapply(signature),
                            RhoByteArray::unapply(pub_key),
                        ) else {
                            return Err(illegal_arg(&format!(
                                "{name} expects data, signature, public key (all as byte arrays), and an acknowledgement channel"
                            )));
                        };
                        let verified = algorithm(d, s, p);
                        cc.produce(&rand, &[RhoBoolean::apply(verified)], ack, path).await
                    }
                    _ => Err(illegal_arg(&format!(
                        "{name} expects data, signature, public key (all as byte arrays), and an acknowledgement channel"
                    ))),
                }
            })
        })
    }

    fn hash_contract(&self, name: &'static str, algorithm: fn(&[u8]) -> Vec<u8>) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg(&format!("{name} expects a byte array and return channel"))
                })?;
                match pars.as_slice() {
                    [input, ack] => match RhoByteArray::unapply(input) {
                        Some(bytes) => {
                            let hash = algorithm(bytes);
                            cc.produce(&rand, &[RhoByteArray::apply(hash)], ack, path)
                                .await
                        }
                        None => Err(illegal_arg(&format!(
                            "{name} expects a byte array and return channel"
                        ))),
                    },
                    _ => Err(illegal_arg(&format!(
                        "{name} expects a byte array and return channel"
                    ))),
                }
            })
        })
    }

    fn secp256k1_verify(&self) -> ScalaBodyFn {
        self.verify_signature_contract("secp256k1Verify", Secp256k1::verify_bytes)
    }

    fn ed25519_verify(&self) -> ScalaBodyFn {
        self.verify_signature_contract("ed25519Verify", Ed25519::verify_bytes)
    }

    fn sha256_hash(&self) -> ScalaBodyFn {
        self.hash_contract("sha256Hash", sha256::hash)
    }

    fn keccak256_hash(&self) -> ScalaBodyFn {
        self.hash_contract("keccak256Hash", keccak256::hash)
    }

    fn blake2b256_hash(&self) -> ScalaBodyFn {
        self.hash_contract("blake2b256Hash", blake2b256::hash)
    }

    fn qucalc_zfa(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("qucalc:zfa expects two arguments"))?;
                match pars.as_slice() {
                    [twists, ack] => {
                        let values = RhoList::unapply(twists)
                            .and_then(|ps| {
                                ps.iter()
                                    .map(|p| {
                                        RhoNumber::unapply(p)
                                            .and_then(|n| u8::try_from(n).ok())
                                            .filter(|v| *v <= 7)
                                    })
                                    .collect::<Option<Vec<u8>>>()
                            })
                            .ok_or_else(|| {
                                illegal_arg("qucalc:zfa expects a list of twist values 0..7")
                            })?;
                        let zfa = achieves_zfa(&values);
                        let phase = pauli_phase(&values).map(|p| p.code()).unwrap_or(0);
                        let result =
                            RhoTupleN::apply(vec![RhoBoolean::apply(zfa), RhoNumber::apply(phase)]);
                        cc.produce(&rand, &[result], ack, path).await
                    }
                    _ => Err(illegal_arg("qucalc:zfa expects two arguments")),
                }
            })
        })
    }

    fn qucalc_grant(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("qucalc:grant expects a twist list and return channel")
                })?;
                match pars.as_slice() {
                    [twists, ret] => {
                        let values = parse_twists(twists)?;
                        if achieves_zfa(&values) {
                            // Mint a capability: a content-addressed registry URI whose value
                            // is the ZFA-balanced twist sequence. Persisted across deploys.
                            let uri = registry::build_uri(&blake2b256::hash(&values));
                            let stored = RhoList::apply(
                                values
                                    .iter()
                                    .map(|&v| RhoNumber::apply(i64::from(v)))
                                    .collect(),
                            );
                            native.registry_insert(&uri, &stored);
                            cc.produce(&rand, &[RhoUri::apply(uri)], ret, path).await
                        } else {
                            cc.produce(&rand, &[RhoNil::apply()], ret, path).await
                        }
                    }
                    _ => Err(illegal_arg(
                        "qucalc:grant expects a twist list and return channel",
                    )),
                }
            })
        })
    }

    fn qucalc_verify(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("qucalc:verify expects a capability uri and return channel")
                })?;
                match pars.as_slice() {
                    [cap, ret] => {
                        let uri = RhoUri::unapply(cap)
                            .or_else(|| RhoString::unapply(cap))
                            .ok_or_else(|| illegal_arg("qucalc:verify expects a uri string"))?
                            .to_string();
                        let ok = match native
                            .registry_lookup(&uri)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Some(stored) => parse_twists(&stored)
                                .map(|v| achieves_zfa(&v))
                                .unwrap_or(false),
                            None => false,
                        };
                        cc.produce(&rand, &[RhoBoolean::apply(ok)], ret, path).await
                    }
                    _ => Err(illegal_arg(
                        "qucalc:verify expects a capability uri and return channel",
                    )),
                }
            })
        })
    }

    fn qucalc_fuse(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("qucalc:fuse expects subject, predicate and return channel")
                })?;
                match pars.as_slice() {
                    [subject, predicate, ret] => {
                        let s = parse_twists(subject)?;
                        let p = parse_twists(predicate)?;
                        let synth = dialectical_synthesis(&s, &p);
                        if synth.zfa {
                            // Blanket fusion resolved to a stable fluxoid: mint it as a capability.
                            let uri = registry::build_uri(&blake2b256::hash(&synth.geometry));
                            let geometry = RhoList::apply(
                                synth
                                    .geometry
                                    .iter()
                                    .map(|&v| RhoNumber::apply(i64::from(v)))
                                    .collect(),
                            );
                            native.registry_insert(&uri, &geometry);
                            let out = RhoTupleN::apply(vec![geometry, RhoUri::apply(uri)]);
                            cc.produce(&rand, &[out], ret, path).await
                        } else {
                            cc.produce(&rand, &[RhoNil::apply()], ret, path).await
                        }
                    }
                    _ => Err(illegal_arg(
                        "qucalc:fuse expects subject, predicate and return channel",
                    )),
                }
            })
        })
    }

    // --- governance (rho:gov:*) ------------------------------------------

    /// The most members a `rho:gov:*` call may name (audit F-3).
    ///
    /// The four governance handlers fold over the union of their arguments — `censure` cubically, the
    /// others superlinearly — and every one of them took its argument counts straight off a deploy with
    /// no bound but the 16 MiB message cap. This is the same refusal-at-entry discipline the parser
    /// already applies to terms (`MAX_AST_DEPTH`, `MAX_CHAIN_LENGTH`, `MAX_VALUE_DEPTH`): bound the
    /// input rather than try to interrupt the work, which on this design is not possible.
    ///
    /// **Consensus-visible**: a call naming more members than this is now refused where it used to
    /// run. Recorded as a deliberate divergence in `spec/RUST-VS-SCALA.md` §3.
    ///
    /// **Why 512 — measured, not chosen.** The first draft of this constant was 4096, on the
    /// reasoning that a real governance set is tens of members and 4096 is far above any legitimate
    /// call. The falsifier refuted the reasoning by timing the uncapped call: a 4097-member
    /// `censure` fold takes **14.2 s**, of which ~9.8 s is fold and the rest harness. Measured at
    /// the same harness offset, the fold alone is ~0.1 s at 256 members, ~0.36 s at 512 and ~1.15 s
    /// at 1024 — near-quadratic in this range, so the cost of being generous is steep. 512 is the
    /// largest round size whose fold is comfortably sub-second, and it is still an order of magnitude
    /// above any real governance set (the genesis PoS set is 2).
    ///
    /// **What this number does not yet cover.** The measurement above drives the *quadratic* path —
    /// empty censures, so the fixed point converges in one round. The cubic path (a full censure and
    /// voucher list) is worse per member and was not measured at the bound. That is the reason to keep
    /// the bound low rather than to raise it, and a later pass that wants a larger one should measure
    /// the cubic case first.
    const MAX_GOV_UNIVERSE: usize = 512;

    /// Bound and charge a `rho:gov:*` fold (audit F-3).
    ///
    /// Shared by the four handlers because the defect was shared: each parsed its arguments and called
    /// into `qucalc` with no charge at all, so the cost of a cubic fold was paid by the validator and
    /// nothing by the deploy. The charge is the squared universe; the bound is the refusal above.
    fn charge_gov_fold(
        cc: &ContractCall<ChargingRSpace, std::sync::Weak<RholangAndScalaDispatcher>>,
        universe: usize,
    ) -> Result<(), RholangError> {
        if universe > Self::MAX_GOV_UNIVERSE {
            return Err(illegal_arg(&format!(
                "governance input names {universe} members, over the {} limit",
                Self::MAX_GOV_UNIVERSE
            )));
        }
        cc.cost()
            .charge(crate::accounting::Costs::gov_fold_cost(universe as i64))
    }

    /// `rho:gov:resolveWeights(directVoters, delegations, trust, ret)` — resolve liquid-democracy
    /// weights: `Map<directVoter, weight>`. Pure and deterministic (see `qucalc::gov`).
    fn gov_resolve_weights(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("gov:resolveWeights expects directVoters, delegations, trust and a return channel")
                })?;
                let [voters, delegations, trust, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "gov:resolveWeights expects directVoters, delegations, trust and a return channel",
                    ));
                };
                let dv = parse_member_list(voters)?;
                let del = parse_member_map(delegations)?;
                let tr = parse_member_int_map(trust)?;
                Self::charge_gov_fold(&cc, dv.len() + del.len() + tr.len())?;
                let out = qucalc::gov::resolve_weights(&dv, &del, &tr);
                cc.produce(&rand, &[member_int_map(&out)], ret, path).await
            })
        })
    }

    /// `rho:gov:trustLevels(ratings, admins, ret)` — the admin-rooted web of trust as a least
    /// fixed point: `Map<member, level>`.
    fn gov_trust_levels(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("gov:trustLevels expects ratings, admins and a return channel")
                })?;
                let [ratings, admins, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "gov:trustLevels expects ratings, admins and a return channel",
                    ));
                };
                let r = parse_rating_list(ratings)?;
                let a = parse_member_list(admins)?;
                Self::charge_gov_fold(&cc, r.len() + a.len())?;
                let out = qucalc::gov::trust_levels(&r, &a);
                cc.produce(&rand, &[member_int_map(&out)], ret, path).await
            })
        })
    }

    /// `rho:gov:censure(censures, levels, vouchers, ret)` — accountability: `(discredited,
    /// newLevels)` via a ⅔ quorum (floored at 2) with voucher slashing.
    fn gov_censure(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg(
                        "gov:censure expects censures, levels, vouchers and a return channel",
                    )
                })?;
                let [censures, levels, vouchers, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "gov:censure expects censures, levels, vouchers and a return channel",
                    ));
                };
                let c = parse_censure_list(censures)?;
                let lv = parse_member_int_map(levels)?;
                let v = parse_voucher_list(vouchers)?;
                Self::charge_gov_fold(&cc, c.len() + lv.len() + v.len())?;
                let (disc, new_levels) = qucalc::gov::censure(&c, &lv, &v);
                let disc_list: Vec<String> = disc.into_iter().collect();
                let out =
                    RhoTupleN::apply(vec![string_list(&disc_list), member_int_map(&new_levels)]);
                cc.produce(&rand, &[out], ret, path).await
            })
        })
    }

    /// `rho:gov:tally(ballots, weights, mode, ret)` — weighted ranked-choice (IRV) or approval
    /// tally. Returns the winning option string, or `Nil` when empty.
    fn gov_tally(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("gov:tally expects ballots, weights, mode and a return channel")
                })?;
                let [ballots, weights, mode, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "gov:tally expects ballots, weights, mode and a return channel",
                    ));
                };
                let b = parse_ranked_ballots(ballots)?;
                let w = parse_member_int_map(weights)?;
                Self::charge_gov_fold(&cc, b.len() + w.len())?;
                let mode = RhoString::unapply(mode)
                    .ok_or_else(|| illegal_arg("gov:tally expects a mode string"))?;
                let winner = match mode {
                    "ranked" => qucalc::gov::tally_ranked(&b, &w),
                    "approval" => qucalc::gov::tally_approval(&b, &w),
                    _ => {
                        return Err(illegal_arg(
                            "gov:tally mode must be \"ranked\" or \"approval\"",
                        ))
                    }
                };
                match winner {
                    Some(name) => {
                        cc.produce(&rand, &[RhoString::apply(name)], ret, path)
                            .await
                    }
                    None => cc.produce(&rand, &[RhoNil::apply()], ret, path).await,
                }
            })
        })
    }

    // --- block / rev / ops ---------------------------------------------

    fn get_block_data(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let bd = self.block_data.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let bd = bd.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("blockData expects only a return channel"))?;
                match pars.as_slice() {
                    [ack] => {
                        let (block_number, sender_bytes, timestamp) = {
                            let data = bd.lock().unwrap_or_else(|p| p.into_inner());
                            (
                                i64::from(data.block_number),
                                data.sender.bytes().to_vec(),
                                data.timestamp,
                            )
                        };
                        let reply = vec![
                            RhoNumber::apply(block_number),
                            RhoByteArray::apply(sender_bytes),
                            RhoNumber::apply(timestamp),
                        ];
                        cc.produce(&rand, &reply, ack, path).await
                    }
                    _ => Err(illegal_arg("blockData expects only a return channel")),
                }
            })
        })
    }

    fn rev_address(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("revAddress expects an operation, an argument and an acknowledgement channel"))?;
                let [op, arg, ack] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "revAddress expects an operation, an argument and an acknowledgement channel",
                    ));
                };
                let Some(op) = RhoString::unapply(op) else {
                    return Err(illegal_arg("revAddress expects an operation string"));
                };
                let response = match op {
                    "validate" => match RhoString::unapply(arg) {
                        Some(address) => RevAddress::parse(address)
                            .err()
                            .map(RhoString::apply)
                            .unwrap_or_default(),
                        None => Par::default(),
                    },
                    "fromPublicKey" => match RhoByteArray::unapply(arg) {
                        Some(pk) => RevAddress::from_public_key(&PublicKey::new(pk.to_vec()))
                            .map(|ra| RhoString::apply(ra.to_base58()))
                            .unwrap_or_default(),
                        None => Par::default(),
                    },
                    "fromDeployerId" => match RhoDeployerId::unapply(arg) {
                        Some(id) => RevAddress::from_deployer_id(id)
                            .map(|ra| RhoString::apply(ra.to_base58()))
                            .unwrap_or_default(),
                        None => Par::default(),
                    },
                    "fromUnforgeable" => match RhoName::unapply(arg) {
                        Some(g) => RhoString::apply(RevAddress::from_unforgeable(g).to_base58()),
                        None => Par::default(),
                    },
                    _ => return Err(illegal_arg("revAddress: unknown operation")),
                };
                cc.produce(&rand, &[response], ack, path).await
            })
        })
    }

    fn deployer_id_ops(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("deployerIdOps expects an operation, an argument and an acknowledgement channel"))?;
                let [op, arg, ack] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "deployerIdOps expects an operation, an argument and an acknowledgement channel",
                    ));
                };
                let response = match RhoString::unapply(op) {
                    Some("pubKeyBytes") => match RhoDeployerId::unapply(arg) {
                        Some(pk) => RhoByteArray::apply(pk.to_vec()),
                        None => Par::default(),
                    },
                    _ => return Err(illegal_arg("deployerIdOps: unknown operation")),
                };
                cc.produce(&rand, &[response], ack, path).await
            })
        })
    }

    fn registry_ops(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("registryOps expects an operation, an argument and an acknowledgement channel"))?;
                let [op, arg, ack] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "registryOps expects an operation, an argument and an acknowledgement channel",
                    ));
                };
                let response = match RhoString::unapply(op) {
                    Some("buildUri") => match RhoByteArray::unapply(arg) {
                        Some(ba) => RhoUri::apply(registry::build_uri(&blake2b256::hash(ba))),
                        None => Par::default(),
                    },
                    _ => return Err(illegal_arg("registryOps: unknown operation")),
                };
                cc.produce(&rand, &[response], ack, path).await
            })
        })
    }

    /// `sys:authToken:ops` — the system auth token's `check`.
    ///
    /// **This is vestigial, and it is recorded here so it is not mistaken for a live control.** In the
    /// Scala this was load-bearing: `Pos.rhox`'s `slash` took a `sysAuthToken` and refused without
    /// one, so the *system* could slash and an arbitrary deploy could not. In this port no production
    /// code mints the token — `RhoSysAuthToken::apply` appears only in this file's tests — so `check`
    /// answers `false` for every argument a program can construct, and nothing is gated on it.
    ///
    /// That is why **it is not a finding**, checked rather than assumed before it was dismissed: the
    /// answer fails closed (a token nobody can hold is never accepted), and the rholang surface
    /// grammar cannot write a `GSysAuthToken` unforgeable at all, so a deploy cannot present one. The
    /// privilege it used to guard moved rather than vanished — `slash` is now a block-level system
    /// deploy, and what stops a proposer abusing it is AUDIT C110's justification check on every
    /// validating node, not a capability a contract holds.
    ///
    /// Left in place rather than removed because the urn is part of the oracle's surface and a deploy
    /// that looks it up should find what the Scala has. A future reader who sees an untokened
    /// capability check should read this paragraph before treating it as a hole.
    fn sys_auth_token_ops(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("sysAuthTokenOps expects an operation, an argument and an acknowledgement channel"))?;
                let [op, arg, ack] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "sysAuthTokenOps expects an operation, an argument and an acknowledgement channel",
                    ));
                };
                let response = match RhoString::unapply(op) {
                    Some("check") => RhoBoolean::apply(RhoSysAuthToken::unapply(arg)),
                    _ => return Err(illegal_arg("sysAuthTokenOps: unknown operation")),
                };
                cc.produce(&rand, &[response], ack, path).await
            })
        })
    }

    // --- native registry -------------------------------------------------

    /// `rho:registry:lookup(uri, ret)` — send the **stored value alone** on `ret`, or `Nil` when the
    /// uri is unknown. Never wrapped.
    ///
    /// The oracle is the genesis `Registry.rho` contract, whose `lookup` forwards
    /// `TreeHashMap!("get", …)` and that sends the stored value by itself
    /// (`legacy/casper/src/main/resources/Registry.rho:397-401`, with recorded output in
    /// `legacy/rholang/examples/tut-registry.rho:8,42-47`). Wrapping it in `(uri, value)` — as this
    /// handler did — breaks every client written for the oracle, which consumes the reply as
    /// `lookup!(uri, *ch) | for (X <- ch) { X!(…) }`: the pair binds to the name and the send is a
    /// silent no-op. System contracts are unaffected because their *stored* value is itself a
    /// `(nonce, data)` pair (via `insertSigned`), which clients destructure as `@(_, X)`.
    /// Recorded as C18 in `spec/AUDIT.md`.
    fn registry_lookup(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("registry lookup expects a uri and return channel")
                })?;
                let [uri, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "registry lookup expects a uri and return channel",
                    ));
                };
                let uri_str = RhoUri::unapply(uri)
                    .or_else(|| RhoString::unapply(uri))
                    .ok_or_else(|| illegal_arg("registry lookup expects a uri string"))?
                    .to_string();
                match native
                    .registry_lookup(&uri_str)
                    .await
                    .map_err(|e| illegal_arg(&e))?
                {
                    Some(value) => cc.produce(&rand, &[value], ret, path).await,
                    None => cc.produce(&rand, &[RhoNil::apply()], ret, path).await,
                }
            })
        })
    }

    /// `rho:registry:insertArbitrary(data, ret)` — store `data` under a fresh URI and return it.
    fn registry_insert_arbitrary(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("insertArbitrary expects data and a return channel")
                })?;
                let [data, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "insertArbitrary expects data and a return channel",
                    ));
                };
                let uri = registry::build_uri(&blake2b256::hash(&rand.to_bytes()));
                native.registry_insert(&uri, data);
                cc.produce(&rand, &[RhoUri::apply(uri)], ret, path).await
            })
        })
    }

    /// `rho:registry:insertSigned:secp256k1((nonce, data), deployerID, ret)` — store `(nonce, data)`
    /// under the deployer-derived URI, or `Nil` when the nonce is stale.
    fn registry_insert_signed(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg(
                        "insertSigned expects (nonce, data), deployerID and a return channel",
                    )
                })?;
                let [signed, deployer_id, ret] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "insertSigned expects (nonce, data), deployerID and a return channel",
                    ));
                };
                let tuple = RhoTupleN::unapply(signed)
                    .ok_or_else(|| illegal_arg("insertSigned expects a (nonce, data) tuple"))?;
                let [nonce_par, data] = tuple else {
                    return Err(illegal_arg("insertSigned expects a (nonce, data) tuple"));
                };
                let nonce = RhoNumber::unapply(nonce_par)
                    .ok_or_else(|| illegal_arg("insertSigned nonce must be a number"))?;
                let pub_key = RhoDeployerId::unapply(deployer_id)
                    .ok_or_else(|| illegal_arg("insertSigned expects a deployerID"))?;
                let uri = registry::build_uri(&blake2b256::hash(pub_key));

                if let Some(stored) = native
                    .registry_lookup(&uri)
                    .await
                    .map_err(|e| illegal_arg(&e))?
                {
                    let old_nonce = RhoTupleN::unapply(&stored)
                        .and_then(|ps| ps.first())
                        .and_then(RhoNumber::unapply)
                        .unwrap_or(0);
                    if nonce <= old_nonce {
                        return cc.produce(&rand, &[RhoNil::apply()], ret, path).await;
                    }
                }
                native.registry_insert(
                    &uri,
                    &RhoTupleN::apply(vec![RhoNumber::apply(nonce), data.clone()]),
                );
                cc.produce(&rand, &[RhoUri::apply(uri)], ret, path).await
            })
        })
    }

    // --- native PoS ------------------------------------------------------

    /// `rho:rchain:pos` — native method dispatch over the PoS state (`getBonds`, `getActiveValidators`).
    fn pos(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        let block_data = self.block_data.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            let block_data = block_data.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("pos expects a method and arguments"))?;
                let [op, rest_par] = pars.as_slice() else {
                    return Err(illegal_arg("pos expects a method and arguments"));
                };
                let op = RhoString::unapply(op)
                    .ok_or_else(|| illegal_arg("pos method must be a string"))?;
                let rest = RhoList::unapply(rest_par)
                    .ok_or_else(|| illegal_arg("pos arguments must be a list"))?;
                eprintln!("[pos] called: {} with {} argument(s)", op, rest.len());
                // A Rholang *list literal* reaches us wrapped — one element that is the list — while a
                // list built as a program value (the tests, `RhoList::apply`) arrives unwrapped. Accept
                // both: no `pos` method takes a list as its only argument, so unwrapping a
                // single-element list is unambiguous. Getting this wrong is invisible — the send simply
                // does not match and the deploy still looks successful.
                let rest: &[Par] = match rest {
                    [only] if RhoList::unapply(only).is_some() => {
                        RhoList::unapply(only).unwrap_or(rest)
                    }
                    _ => rest,
                };
                // The current block number drives the bond/withdraw quarantine bookkeeping.
                let block_number = {
                    let bd = block_data.lock().unwrap_or_else(|p| p.into_inner());
                    i64::from(bd.block_number)
                };
                match op {
                    "getBonds" => {
                        let [ret] = rest else {
                            return Err(illegal_arg("getBonds expects a return channel"));
                        };
                        let bonds = native.bonds().await.map_err(|e| illegal_arg(&e))?;
                        eprintln!("[pos] getBonds -> {} entries", bonds.len());
                        let kvs: Vec<(Par, Par)> = bonds
                            .iter()
                            .map(|(v, stake)| {
                                (
                                    RhoByteArray::apply(v.as_bytes().to_vec()),
                                    RhoNumber::apply(i64::from(*stake)),
                                )
                            })
                            .collect();
                        cc.produce(&rand, &[RhoMap::apply(kvs)], ret, path).await
                    }
                    "getActiveValidators" => {
                        let [ret] = rest else {
                            return Err(illegal_arg(
                                "getActiveValidators expects a return channel",
                            ));
                        };
                        let validators = native
                            .active_validators()
                            .await
                            .map_err(|e| illegal_arg(&e))?;
                        eprintln!("[pos] getActiveValidators -> {} entries", validators.len());
                        let ps: Vec<Par> = validators
                            .iter()
                            .map(|v| RhoByteArray::apply(v.as_bytes().to_vec()))
                            .collect();
                        cc.produce(&rand, &[RhoSet::apply(ps)], ret, path).await
                    }
                    // The admission diagnostic: what the *state* says about trust. Without it, a `trust`
                    // that reported success and did not stick is indistinguishable from one that never
                    // ran, because a returned error value is still a successful deploy (#74).
                    "getTrusted" => {
                        let [ret] = rest else {
                            return Err(illegal_arg("getTrusted expects a return channel"));
                        };
                        let trusted = native.trusted().await.map_err(|e| illegal_arg(&e))?;
                        eprintln!("[pos] getTrusted -> {} entries", trusted.len());
                        let ps: Vec<Par> = trusted
                            .iter()
                            .map(|v| RhoByteArray::apply(v.as_bytes().to_vec()))
                            .collect();
                        cc.produce(&rand, &[RhoSet::apply(ps)], ret, path).await
                    }
                    // **The delegator's own position** (law 57, #193), symmetric with `getBonds`.
                    //
                    // Scoped to the key the caller asks about, so the read is bounded by *that*
                    // delegator rather than by the whole `pos:delegations` ledger — which is unbounded
                    // in the number of delegators and whose per-delegation floor is the only DoS
                    // control (`spec/RUST-VS-SCALA.md` §3 item 12, residual O5). An operator-scoped
                    // listing (every delegator of one key) is the other direction and is deliberately
                    // not here: it is the unbounded one, and nothing needs it yet.
                    //
                    // Each entry's four fields come from three leaves: the principal from
                    // `pos:delegations`, the accrued reward from `pos:delegated_rewards`, the staged
                    // exit's deadline from `pos:pending_delegations` (or `Nil` when nothing is
                    // staged).
                    "getDelegations" => {
                        let [delegator, ret] = rest else {
                            return Err(illegal_arg(
                                "getDelegations expects a delegator public key and a return channel",
                            ));
                        };
                        let delegator = RhoByteArray::unapply(delegator)
                            .and_then(|b| Validator::try_from(b).ok())
                            .ok_or_else(|| {
                                illegal_arg("getDelegations expects a 65-byte delegator public key")
                            })?;
                        // **The accessors, never a `set_*`.** An absent leaf reads as an empty map, and
                        // a *write* of an empty map from a read path would put a trie leaf under a
                        // chain that has never delegated and move its root — the dormancy requirement
                        // law 57 states and `spec/RUST-FIRST.md` § *Dormancy* records.
                        let ledger = native.delegations().await.map_err(|e| illegal_arg(&e))?;
                        let pending = native
                            .pending_delegations()
                            .await
                            .map_err(|e| illegal_arg(&e))?;
                        let rewards = native
                            .delegated_rewards()
                            .await
                            .map_err(|e| illegal_arg(&e))?;
                        let entries: Vec<Par> = ledger
                            .iter()
                            .filter(|(key, _)| key.delegator == delegator)
                            .map(|(key, amount)| {
                                let accrued = rewards.get(key).map_or(0, |r| i64::from(*r));
                                let staged = match pending.get(key) {
                                    Some(deadline) => RhoNumber::apply(*deadline),
                                    None => RhoNil::apply(),
                                };
                                RhoTupleN::apply(vec![
                                    RhoByteArray::apply(key.operator.as_bytes().to_vec()),
                                    RhoNumber::apply(i64::from(*amount)),
                                    RhoNumber::apply(accrued),
                                    staged,
                                ])
                            })
                            .collect();
                        eprintln!("[pos] getDelegations -> {} entries", entries.len());
                        cc.produce(&rand, &[RhoList::apply(entries)], ret, path).await
                    }
                    "bond" => {
                        let [deployer_id, amount, ret] = rest else {
                            eprintln!("[pos] bad argument shape: bond");
                            return Err(illegal_arg(
                                "bond expects deployerId, amount and return channel",
                            ));
                        };
                        // Capability, not data: only the unforgeable `GDeployerId` carried by the
                        // normalizer's `rho:rchain:deployerId` binding satisfies this unapply, so a
                        // program-authored byte array can no longer bond someone else's key.
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("bond expects a deployerId"))?;
                        let amount = RhoNumber::unapply(amount)
                            .ok_or_else(|| illegal_arg("bond expects a number amount"))?;
                        let amount =
                            NonNegI64::try_from(amount).map_err(|e| illegal_arg(&e.to_string()))?;
                        let validator = Validator::try_from(deployer_id)
                            .map_err(|e| illegal_arg(&e.to_string()))?;
                        let out = match native
                            .bond(&validator, amount, block_number)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Ok(()) => {
                                eprintln!("[pos] ok");
                                RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()])
                            }
                            Err(msg) => {
                                eprintln!("[pos] refused: {msg}");
                                RhoTupleN::apply(vec![
                                    RhoBoolean::apply(false),
                                    RhoString::apply(msg),
                                ])
                            }
                        };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    "withdraw" => {
                        let [deployer_id, ret] = rest else {
                            eprintln!("[pos] bad argument shape: withdraw");
                            return Err(illegal_arg(
                                "withdraw expects deployerId and return channel",
                            ));
                        };
                        // Capability, not data (see `bond`).
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("withdraw expects a deployerId"))?;
                        let validator = Validator::try_from(deployer_id)
                            .map_err(|e| illegal_arg(&e.to_string()))?;
                        let out = match native
                            .withdraw(&validator, block_number)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Ok(()) => {
                                eprintln!("[pos] ok");
                                RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()])
                            }
                            Err(msg) => {
                                eprintln!("[pos] refused: {msg}");
                                RhoTupleN::apply(vec![
                                    RhoBoolean::apply(false),
                                    RhoString::apply(msg),
                                ])
                            }
                        };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    // **Delegated stake** (law 57, #193). A *method on this channel* rather than a
                    // block-level system deploy: `delegate` is reached by an ordinary deploy, so it
                    // needs no `SystemDeployData` variant and replays from this deploy's own COMM
                    // trace, unlike `CloseBlock`/`Slash`/`RecordSpoke`. Nothing else in the node had to
                    // learn about it — the aggregate it writes is what `select_active` already draws
                    // from, so `compute_bonds`, the block's bond cache and finality follow for free.
                    "delegate" => {
                        let [deployer_id, operator, amount, ret] = rest else {
                            eprintln!("[pos] bad argument shape: delegate");
                            return Err(illegal_arg(
                                "delegate expects deployerId, operator public key, amount and return \
                                 channel",
                            ));
                        };
                        // Capability, not data (see `bond`): the *delegator* is whoever signed the
                        // deploy, so the principal can only ever come out of the signer's own vault.
                        let delegator = RhoDeployerId::unapply(deployer_id)
                            .and_then(|bytes| Validator::try_from(bytes).ok())
                            .ok_or_else(|| illegal_arg("delegate expects a deployerId"))?;
                        // The operator is a *named* key rather than the caller — which is the whole
                        // point of the primitive, and why this is not `bond`.
                        let operator = RhoByteArray::unapply(operator)
                            .and_then(|bytes| Validator::try_from(bytes).ok())
                            .ok_or_else(|| {
                                illegal_arg("delegate expects a 65-byte validator public key")
                            })?;
                        let amount = RhoNumber::unapply(amount)
                            .ok_or_else(|| illegal_arg("delegate expects a number amount"))?;
                        let amount =
                            NonNegI64::try_from(amount).map_err(|e| illegal_arg(&e.to_string()))?;
                        let out = match native
                            .delegate(&delegator, &operator, amount)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Ok(()) => {
                                eprintln!("[pos] ok");
                                RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()])
                            }
                            Err(msg) => {
                                eprintln!("[pos] refused: {msg}");
                                RhoTupleN::apply(vec![
                                    RhoBoolean::apply(false),
                                    RhoString::apply(msg),
                                ])
                            }
                        };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    "undelegate" => {
                        let [deployer_id, operator, ret] = rest else {
                            eprintln!("[pos] bad argument shape: undelegate");
                            return Err(illegal_arg(
                                "undelegate expects deployerId, operator public key and return \
                                 channel",
                            ));
                        };
                        // Capability, not data (see `bond`): only the delegator may withdraw its own
                        // delegation.
                        let delegator = RhoDeployerId::unapply(deployer_id)
                            .and_then(|bytes| Validator::try_from(bytes).ok())
                            .ok_or_else(|| illegal_arg("undelegate expects a deployerId"))?;
                        let operator = RhoByteArray::unapply(operator)
                            .and_then(|bytes| Validator::try_from(bytes).ok())
                            .ok_or_else(|| {
                                illegal_arg("undelegate expects a 65-byte validator public key")
                            })?;
                        let out = match native
                            .undelegate(&delegator, &operator, block_number)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Ok(()) => {
                                eprintln!("[pos] ok");
                                RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()])
                            }
                            Err(msg) => {
                                eprintln!("[pos] refused: {msg}");
                                RhoTupleN::apply(vec![
                                    RhoBoolean::apply(false),
                                    RhoString::apply(msg),
                                ])
                            }
                        };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    "trust" | "untrust" => {
                        let [deployer_id, target, ret] = rest else {
                            eprintln!("[pos] bad argument shape: trust/untrust");
                            return Err(illegal_arg(
                                "trust/untrust expects deployerId, target public key and return channel",
                            ));
                        };
                        // Capability, not data (see `bond`): the caller must be the deployer.
                        let caller = RhoDeployerId::unapply(deployer_id)
                            .and_then(|bytes| Validator::try_from(bytes).ok())
                            .ok_or_else(|| illegal_arg("trust/untrust expects a deployerId"))?;
                        let target = RhoByteArray::unapply(target)
                            .and_then(|bytes| Validator::try_from(bytes).ok())
                            .ok_or_else(|| {
                                illegal_arg("trust/untrust expects a 65-byte validator public key")
                            })?;
                        let result = if op == "trust" {
                            native.trust(&caller, &target).await
                        } else {
                            native.untrust(&caller, &target).await
                        };
                        let out = match result.map_err(|e| illegal_arg(&e))? {
                            Ok(()) => {
                                eprintln!("[pos] ok");
                                RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()])
                            }
                            Err(msg) => {
                                eprintln!("[pos] refused: {msg}");
                                RhoTupleN::apply(vec![
                                    RhoBoolean::apply(false),
                                    RhoString::apply(msg),
                                ])
                            }
                        };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    _ => Err(illegal_arg(&format!("pos: unknown method {op}"))),
                }
            })
        })
    }

    // --- native HTTP-result oracle (RCHIP #54) ---------------------------

    /// `rho:io:http` — the deterministic HTTP-result oracle, framed as a **Git pull request** on
    /// external data:
    ///
    /// * `record(url, value)` — *capture* a value the deployer fetched off-chain. First writer
    ///   wins; a later capture of an already-recorded URL is a no-op returning `false`. This is the
    ///   "initial value", asserted inside a signed deploy, so the chain records *who* claimed *what*.
    /// * `get(url)` — read the recorded value (or `Nil`), deterministically.
    /// * `check(url, expected)` — is the record equal to `expected`?
    /// * `height(url)` — the block at which the value was captured (or `Nil`).
    ///
    /// Consensus safety comes from the capture being **part of the deploy**: replay/validation
    /// never performs a network fetch, so the result is a pure function of the deploy and the
    /// recorded state. (A proposer-side live fetch would have to commit its value into the block
    /// for validators to reproduce it; that is a follow-up that changes the block format.)
    fn http(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        let block_data = self.block_data.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            let block_data = block_data.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("http expects a method and arguments"))?;
                let [op, rest_par] = pars.as_slice() else {
                    return Err(illegal_arg("http expects a method and arguments"));
                };
                let op = RhoString::unapply(op)
                    .ok_or_else(|| illegal_arg("http method must be a string"))?;
                let rest = RhoList::unapply(rest_par)
                    .ok_or_else(|| illegal_arg("http arguments must be a list"))?;
                let block_number = {
                    let bd = block_data.lock().unwrap_or_else(|p| p.into_inner());
                    i64::from(bd.block_number)
                };
                match op {
                    "record" => {
                        let [url_par, value_par, ret] = rest else {
                            return Err(illegal_arg(
                                "http record expects url, value and a return channel",
                            ));
                        };
                        let url = RhoString::unapply(url_par)
                            .ok_or_else(|| illegal_arg("http record expects a string url"))?;
                        let value = RhoString::unapply(value_par)
                            .ok_or_else(|| illegal_arg("http record expects a string value"))?;
                        let recorded = native
                            .record_http(url, value, block_number)
                            .await
                            .map_err(|e| illegal_arg(&e))?;
                        cc.produce(&rand, &[RhoBoolean::apply(recorded)], ret, path)
                            .await
                    }
                    "get" => {
                        let [url_par, ret] = rest else {
                            return Err(illegal_arg("http get expects a url and a return channel"));
                        };
                        let url = RhoString::unapply(url_par)
                            .ok_or_else(|| illegal_arg("http get expects a string url"))?;
                        let out =
                            match native.http_record(url).await.map_err(|e| illegal_arg(&e))? {
                                Some((value, _)) => RhoString::apply(value),
                                None => RhoNil::apply(),
                            };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    "check" => {
                        let [url_par, expected_par, ret] = rest else {
                            return Err(illegal_arg(
                                "http check expects url, an expected value and a return channel",
                            ));
                        };
                        let url = RhoString::unapply(url_par)
                            .ok_or_else(|| illegal_arg("http check expects a string url"))?;
                        let expected = RhoString::unapply(expected_par)
                            .ok_or_else(|| illegal_arg("http check expects a string value"))?;
                        let consistent =
                            match native.http_record(url).await.map_err(|e| illegal_arg(&e))? {
                                Some((value, _)) => value == expected,
                                None => false,
                            };
                        cc.produce(&rand, &[RhoBoolean::apply(consistent)], ret, path)
                            .await
                    }
                    "height" => {
                        let [url_par, ret] = rest else {
                            return Err(illegal_arg(
                                "http height expects a url and a return channel",
                            ));
                        };
                        let url = RhoString::unapply(url_par)
                            .ok_or_else(|| illegal_arg("http height expects a string url"))?;
                        let out =
                            match native.http_record(url).await.map_err(|e| illegal_arg(&e))? {
                                Some((_, block)) => RhoNumber::apply(block),
                                None => RhoNil::apply(),
                            };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    _ => Err(illegal_arg(&format!("http: unknown method {op}"))),
                }
            })
        })
    }

    // --- native vault ----------------------------------------------------

    /// `rho:rchain:revVault` — native method dispatch over the vault balance map.
    /// The `rho:rchain:multiSigRevVault` **native fixed channel** handler.
    ///
    /// **It refuses, and the refusal is the fix** (AUDIT C114). This channel used to be wired to
    /// [`Self::rev_vault`] — the *single-signer* handler — so a deploy that put funds behind the
    /// multi-signature name got single-key custody: no quorum, no co-signers, no confirmation step,
    /// and nothing said so.
    ///
    /// **The urn is served by the installed contract, and this handler is not that path** (AUDIT
    /// C114's alternative, taken 2026-09-27). Two honest options existed — install the contract, or
    /// stop answering to its name — and the first was taken once the remediation pass closed and
    /// there was no genesis block to break: `casper/src/genesis/mod.rs:248` installs
    /// `MultiSigRevVault.rho` (adapted), and `GENESIS_ALIASES` maps this shorthand to
    /// `GenesisAliasSource::Contract`, so **`rho:registry:lookup` on this urn returns the
    /// multi-signature contract**. What still refuses is the *native fixed channel* of the same name,
    /// reached by binding the urn directly as a name rather than looking it up.
    ///
    /// Keeping that refusal is deliberate: the fixed channel answers no method directly, because
    /// binding a name to it is exactly the shape that used to yield silently weaker custody. The
    /// message below therefore names the path that *does* work, rather than claiming the capability is
    /// absent — the earlier wording said "this node does not install the multi-signature vault
    /// contract", which stopped being true when the contract was installed, and a refusal that
    /// misdirects a caller is the same class of defect as the one this handler exists to fix.
    /// `the_fixed_channel_refusal_points_at_the_installed_contract` pins that.
    fn multi_sig_rev_vault(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        Box::new(move |args: Vec<ListParWithRandom>, _path: DfsPath| {
            let cc = cc.clone();
            Box::pin(async move {
                let (pars, _rand) = cc.unapply(&args).ok_or_else(|| {
                    illegal_arg("multiSigRevVault expects a method and arguments")
                })?;
                let [op, _rest] = pars.as_slice() else {
                    return Err(illegal_arg(
                        "multiSigRevVault expects a method and arguments",
                    ));
                };
                let op = RhoString::unapply(op)
                    .ok_or_else(|| illegal_arg("multiSigRevVault method must be a string"))?;
                Err(illegal_arg(&format!(
                    "multiSigRevVault: '{op}' is not available on the native fixed channel. The \
                     multi-signature vault contract (casper/src/genesis/resources/MultiSigRevVault.rho) \
                     **is** installed at genesis and this urn's registry alias resolves to it — reach \
                     it with `rho:registry:lookup!(`rho:rchain:multiSigRevVault`, *ch)` and call the \
                     contract you get back, which has the quorum, the co-signers and the confirmation \
                     step. Binding this urn as a name gets the fixed channel, which answers no method: \
                     that binding used to return single-key custody under a multi-signature name. For \
                     single-key custody, `rho:rchain:revVault` is named for it."
                )))
            })
        })
    }

    fn rev_vault(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("revVault expects a method and arguments"))?;
                let [op, rest_par] = pars.as_slice() else {
                    return Err(illegal_arg("revVault expects a method and arguments"));
                };
                let op = RhoString::unapply(op)
                    .ok_or_else(|| illegal_arg("revVault method must be a string"))?;
                let rest = RhoList::unapply(rest_par)
                    .ok_or_else(|| illegal_arg("revVault arguments must be a list"))?;
                match op {
                    "getBalance" => {
                        let [addr, ret] = rest else {
                            return Err(illegal_arg(
                                "getBalance expects an address and return channel",
                            ));
                        };
                        let addr = RhoString::unapply(addr)
                            .ok_or_else(|| illegal_arg("getBalance expects a string address"))?;
                        let balance = match native
                            .vault_balance(addr)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Some(b) => b,
                            None => NonNegI64::zero(),
                        };
                        cc.produce(&rand, &[RhoNumber::apply(i64::from(balance))], ret, path)
                            .await
                    }
                    "deposit" => {
                        // Unauthenticated mint removed (issue #4): the Scala RevVault mints REV
                        // only via the genesis `init` path; a deploy callable `deposit` would let
                        // any deploy create REV from nothing.
                        Err(illegal_arg(
                            "revVault: deposit is not callable (REV is minted at genesis only)",
                        ))
                    }
                    "transfer" => {
                        let [deployer_id, to, amount, ret] = rest else {
                            return Err(illegal_arg(
                                "transfer expects deployerId, to, amount and return channel",
                            ));
                        };
                        // Capability, not data: the `from` account is derived from the caller's
                        // unforgeable deployerId, so a deploy can only spend its own vault.
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("transfer expects a deployerId"))?;
                        let from = RevAddress::from_deployer_id(deployer_id)
                            .ok_or_else(|| illegal_arg("transfer: invalid deployerId"))?
                            .to_base58();
                        let to = RhoString::unapply(to)
                            .ok_or_else(|| illegal_arg("transfer expects a string to-address"))?;
                        let amount = RhoNumber::unapply(amount)
                            .ok_or_else(|| illegal_arg("transfer expects a number amount"))?;
                        let amount =
                            NonNegI64::try_from(amount).map_err(|e| illegal_arg(&e.to_string()))?;
                        // **The movement is `transfer_vault`, shared with the capability handle.**
                        // The two arms differ only in where `from` comes from — the caller's
                        // `deployerId` here, a presented name there — and the spend rule must not be
                        // able to differ with it. The self-transfer no-op and the insufficient-balance
                        // refusal both live in that one place now.
                        match native
                            .transfer_vault(&from, to, amount)
                            .await
                            .map_err(|e| illegal_arg(&e))?
                        {
                            Ok(()) => cc.produce(&rand, &[RhoNil::apply()], ret, path).await,
                            // The classic arm reports a refusal as a deploy error rather than as
                            // `(false, reason)`: that is its existing contract, and changing a reply
                            // shape is not what this change is for.
                            Err(reason) => Err(illegal_arg(&reason)),
                        }
                    }
                    "findOrCreate" => {
                        let [deployer_id, ret] = rest else {
                            return Err(illegal_arg(
                                "findOrCreate expects deployerId and return channel",
                            ));
                        };
                        // **Two shapes, one arm** (additive, like the handle itself). The port's
                        // classic shape takes the caller's own `deployerId`, so a deploy can only
                        // open its own vault; the oracle's takes a REV *address*
                        // (`RevVault.rho:103`), which is what `MultiSigRevVault.rho` calls with —
                        // it opens a vault for an address derived from a `new` name it holds. Both
                        // mint a handle, and neither is an authority: the address form opens a vault
                        // its caller may not be able to spend from, which is the point.
                        let addr = match RhoDeployerId::unapply(deployer_id) {
                            Some(id) => RevAddress::from_deployer_id(id)
                                .ok_or_else(|| illegal_arg("findOrCreate: invalid deployerId"))?
                                .to_base58(),
                            None => RhoString::unapply(deployer_id)
                                .ok_or_else(|| {
                                    illegal_arg(
                                        "findOrCreate expects a deployerId or a REV address",
                                    )
                                })?
                                .to_string(),
                        };
                        native
                            .find_or_create_vault(&addr)
                            .await
                            .map_err(|e| illegal_arg(&e))?;

                        // **The handle: the capability half of the vault, and the reason
                        // `spec/RUST-FIRST.md`'s B2 could be revisited.** The oracle's `findOrCreate`
                        // returns a `MakeMint` *purse* — a capability a contract can be handed and
                        // spend from, which is what the multi-signature vault needs (it holds REV
                        // under a name, not under a key). This port's vault was address-keyed only,
                        // so a contract could not hold one.
                        //
                        // The name is drawn from the send's own RNG (`unapply`'s `random_state`),
                        // which is the deploy's carried state — so a replay mints the same bytes and
                        // the same continuations, which is what makes this replayable rather than a
                        // `new_random` hole in the state hash.
                        let mut rand = rand;
                        let name_bytes = rand.next();
                        native.set_vault_name(&name_bytes, &addr);
                        let handle = install_vault_handle(
                            cc.clone(),
                            native.clone(),
                            name_bytes,
                            addr.clone(),
                        )
                        .await
                        .map_err(|e| illegal_arg(&e.to_string()))?;

                        let out = RhoTupleN::apply(vec![RhoBoolean::apply(true), handle]);
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    "deployerAuthKey" => {
                        let [deployer_id, ret] = rest else {
                            return Err(illegal_arg(
                                "deployerAuthKey expects a deployerId and a return channel",
                            ));
                        };
                        // The human half of the authority (`RevVault.rho:94` makes an `AuthKey` whose
                        // shape is the deployer's own address). The port returns the `deployerId`
                        // itself: it is unforgeable, it names the signer, and the transfer check
                        // derives the address from it exactly as the classic arm does — so `transfer`
                        // accepts one value for "I am this vault's key holder" on both paths.
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("deployerAuthKey expects a deployerId"))?;
                        let key = RhoDeployerId::apply(deployer_id.to_vec());
                        cc.produce(&rand, &[key], ret, path).await
                    }
                    "unforgeableAuthKey" => {
                        let [unf, ret] = rest else {
                            return Err(illegal_arg(
                                "unforgeableAuthKey expects a name and a return channel",
                            ));
                        };
                        // The oracle makes an `AuthKey` whose *shape* is the vault's own REV address
                        // (`RevVault.rho:94-101`). The port keeps the shape and drops the token
                        // machinery: a name is an authority by being a name, so the value returned
                        // here **is** the one `transfer` accepts, and what makes it valid is that it
                        // resolves to the same address the vault being spent resolves to.
                        let unf = RhoName::unapply(unf)
                            .ok_or_else(|| illegal_arg("unforgeableAuthKey expects a name"))?;
                        let name_bytes = unf.id.clone();
                        let addr = RevAddress::from_unforgeable(unf).to_base58();
                        // **An authority, not a handle** — this is the half `findOrCreate` must not
                        // be able to mint: the caller supplies the name, so only a holder can make
                        // the entry, and the spend check reads this map rather than the handle map.
                        native.set_vault_authority(&name_bytes, &addr);
                        let key = RhoName::apply_bytes(name_bytes);
                        cc.produce(&rand, &[key], ret, path).await
                    }
                    _ => Err(illegal_arg(&format!("revVault: unknown method {op}"))),
                }
            })
        })
    }

    /// The cross-shard two-phase-commit participant (Laws 26–29): a per-shard REV escrow that a
    /// coordinator drives via `prepare` (lock + vote) → `commit` (apply) / `abort` (compensate).
    fn txn(&self) -> ScalaBodyFn {
        let cc = self.contract_call.clone();
        let native = self.native_state.clone();
        Box::new(move |args: Vec<ListParWithRandom>, path: DfsPath| {
            let cc = cc.clone();
            let native = native.clone();
            Box::pin(async move {
                let (pars, rand) = cc
                    .unapply(&args)
                    .ok_or_else(|| illegal_arg("txn expects a method and arguments"))?;
                let [op, rest_par] = pars.as_slice() else {
                    return Err(illegal_arg("txn expects a method and arguments"));
                };
                let op = RhoString::unapply(op)
                    .ok_or_else(|| illegal_arg("txn method must be a string"))?;
                let rest = RhoList::unapply(rest_par)
                    .ok_or_else(|| illegal_arg("txn arguments must be a list"))?;
                match op {
                    "prepare" => {
                        let [txn_id, coordinator, amount, to, deployer_id, ret] = rest else {
                            return Err(illegal_arg(
                                "prepare expects txnId, coordinator, amount, to, deployerId and return channel",
                            ));
                        };
                        let txn_id = RhoByteArray::unapply(txn_id)
                            .ok_or_else(|| illegal_arg("prepare expects a byte-array txnId"))?;
                        let coordinator = RhoByteArray::unapply(coordinator).ok_or_else(|| {
                            illegal_arg("prepare expects a byte-array coordinator")
                        })?;
                        let coordinator_pk = PublicKey::new(coordinator.to_vec());
                        let amount = RhoNumber::unapply(amount)
                            .ok_or_else(|| illegal_arg("prepare expects a number amount"))?;
                        let amount =
                            NonNegI64::try_from(amount).map_err(|e| illegal_arg(&e.to_string()))?;
                        let to = RhoString::unapply(to)
                            .ok_or_else(|| illegal_arg("prepare expects a string to-address"))?;
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("prepare expects a deployerId"))?;
                        let from = RevAddress::from_deployer_id(deployer_id)
                            .ok_or_else(|| illegal_arg("prepare: invalid deployerId"))?
                            .to_base58();
                        let vote = match native
                            .txn_prepare(txn_id, &coordinator_pk, amount, &from, to)
                            .await
                        {
                            Ok(TxnState::Prepared) => "ready".to_string(),
                            Ok(state) => txn_state_string(state),
                            Err(_) => "abort".to_string(),
                        };
                        cc.produce(&rand, &[RhoString::apply(vote)], ret, path)
                            .await
                    }
                    "commit" => {
                        let [txn_id, deployer_id, ret] = rest else {
                            return Err(illegal_arg(
                                "commit expects txnId, deployerId and return channel",
                            ));
                        };
                        let txn_id = RhoByteArray::unapply(txn_id)
                            .ok_or_else(|| illegal_arg("commit expects a byte-array txnId"))?;
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("commit expects a deployerId"))?;
                        let Some(rec) = native.txn(txn_id).await.map_err(|e| illegal_arg(&e))?
                        else {
                            return Err(illegal_arg("commit: unknown transaction"));
                        };
                        if deployer_id != rec.coordinator.bytes() {
                            return Err(illegal_arg("commit: not the coordinator"));
                        }
                        let state = native
                            .txn_commit(txn_id)
                            .await
                            .map_err(|e| illegal_arg(&e))?;
                        cc.produce(
                            &rand,
                            &[RhoString::apply(txn_state_string(state))],
                            ret,
                            path,
                        )
                        .await
                    }
                    "abort" => {
                        let [txn_id, deployer_id, ret] = rest else {
                            return Err(illegal_arg(
                                "abort expects txnId, deployerId and return channel",
                            ));
                        };
                        let txn_id = RhoByteArray::unapply(txn_id)
                            .ok_or_else(|| illegal_arg("abort expects a byte-array txnId"))?;
                        let deployer_id = RhoDeployerId::unapply(deployer_id)
                            .ok_or_else(|| illegal_arg("abort expects a deployerId"))?;
                        let Some(rec) = native.txn(txn_id).await.map_err(|e| illegal_arg(&e))?
                        else {
                            return Err(illegal_arg("abort: unknown transaction"));
                        };
                        if deployer_id != rec.coordinator.bytes() {
                            return Err(illegal_arg("abort: not the coordinator"));
                        }
                        let state = native
                            .txn_abort(txn_id)
                            .await
                            .map_err(|e| illegal_arg(&e))?;
                        cc.produce(
                            &rand,
                            &[RhoString::apply(txn_state_string(state))],
                            ret,
                            path,
                        )
                        .await
                    }
                    "recover" => {
                        let [txn_id, ret] = rest else {
                            return Err(illegal_arg("recover expects txnId and return channel"));
                        };
                        let txn_id = RhoByteArray::unapply(txn_id)
                            .ok_or_else(|| illegal_arg("recover expects a byte-array txnId"))?;
                        let out = match native.txn(txn_id).await.map_err(|e| illegal_arg(&e))? {
                            Some(rec) => RhoTupleN::apply(vec![
                                RhoString::apply(txn_state_string(rec.state)),
                                RhoByteArray::apply(rec.coordinator.bytes().to_vec()),
                                RhoNumber::apply(i64::from(rec.amount)),
                                RhoString::apply(rec.from),
                                RhoString::apply(rec.to),
                            ]),
                            None => RhoNil::apply(),
                        };
                        cc.produce(&rand, &[out], ret, path).await
                    }
                    _ => Err(illegal_arg(&format!("txn: unknown method {op}"))),
                }
            })
        })
    }
}

/// Render a cross-shard transaction state as its rholang reply string.
fn txn_state_string(state: TxnState) -> String {
    match state {
        TxnState::Prepared => "prepared".to_string(),
        TxnState::Committed => "committed".to_string(),
        TxnState::Aborted => "aborted".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
    use rchain_models::ast::{GPrivate, GUnforgeable};
    use rchain_models::runtime::{BindPattern, ListParWithRandom, TaggedContinuation};
    use rchain_models::sorted::SortedProc;
    use rchain_rspace::errors::RSpaceError;
    use rchain_rspace::tuple_space::{
        ContResult, Result as RSpaceResult, Tuplespace as RSpaceTuplespace,
    };
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    struct MockSpace {
        produced: Mutex<Vec<(SortedProc, ListParWithRandom, bool)>>,
    }

    #[async_trait]
    impl RSpaceTuplespace<SortedProc, BindPattern, ListParWithRandom, TaggedContinuation>
        for MockSpace
    {
        async fn consume(
            &self,
            _channels: &[SortedProc],
            _patterns: &[BindPattern],
            _continuation: TaggedContinuation,
            _persist: bool,
            _peeks: BTreeSet<usize>,
        ) -> Result<
            Option<(
                ContResult<SortedProc, BindPattern, TaggedContinuation>,
                Vec<RSpaceResult<SortedProc, ListParWithRandom>>,
            )>,
            RSpaceError,
        > {
            Ok(None)
        }

        async fn produce(
            &self,
            channel: SortedProc,
            data: ListParWithRandom,
            persist: bool,
        ) -> Result<
            Option<(
                ContResult<SortedProc, BindPattern, TaggedContinuation>,
                Vec<RSpaceResult<SortedProc, ListParWithRandom>>,
            )>,
            RSpaceError,
        > {
            self.produced
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((channel, data, persist));
            Ok(None)
        }

        async fn install(
            &self,
            _channels: &[SortedProc],
            _patterns: &[BindPattern],
            _continuation: TaggedContinuation,
        ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError> {
            Ok(None)
        }
    }

    fn mock_system_processes(mock: &Arc<MockSpace>) -> (SystemProcesses, Vec<Definition>) {
        let charging = ChargingRSpace::new(
            mock.clone(),
            Arc::new(crate::accounting::CostAccounting::from_initial(
                crate::accounting::Costs::unsafe_max(),
            )),
        );
        let dispatcher = Arc::new(RholangAndScalaDispatcher::new(
            std::collections::BTreeMap::new(),
        ));
        let block_data = Arc::new(Mutex::new(BlockData::empty()));
        let native_state = Arc::new(NativeSystemState::new(Arc::new(
            rchain_rspace::native_store::InMemNativeStore::empty(),
        )));
        let sp = SystemProcesses::new(charging, dispatcher, block_data, native_state);
        let defs = sp.definitions();
        (sp, defs)
    }

    /// The `(urn, callArity)` pairs `spec/conformance/protocol.tsv` declares, read from the emitted
    /// corpus. The path is relative to this crate, like the one
    /// `rholang/tests/lean_protocol_corpus.rs` reads.
    fn catalog_arities() -> Vec<(String, i32)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../spec/conformance/protocol.tsv");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "read {}: {e}\n(run tools/emit-lean-corpus.sh)",
                path.display()
            )
        });
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let mut c = line.split('\t');
                assert_eq!(c.next(), Some("protocol"), "layer column: {line}");
                let urn = c.next().expect("urn").to_string();
                let _args = c.next().expect("args");
                let arity: i32 = c
                    .next()
                    .expect("callArity")
                    .parse()
                    .unwrap_or_else(|e| panic!("{urn}: callArity: {e}"));
                (urn, arity)
            })
            .collect()
    }

    /// Which catalog rows disagree with the installed table — the comparison as a *function*, so the
    /// drift it exists to catch can be shown to be caught without breaking the tree.
    fn arity_mismatches(catalog: &[(String, i32)], defs: &[Definition]) -> Vec<String> {
        let mut bad = Vec::new();
        for (urn, call_arity) in catalog {
            match defs.iter().find(|d| d.urn == *urn) {
                Some(d) if d.arity == *call_arity => {}
                Some(d) => bad.push(format!(
                    "{urn}: the catalog declares arity {call_arity}, the node installs {}",
                    d.arity
                )),
                None => bad.push(format!(
                    "{urn}: in the Lean catalog, but no `Definition` installs it"
                )),
            }
        }
        bad
    }

    /// **Every urn the Lean catalog declares an arity for agrees with the `Definition.arity` this
    /// node installs** (AUDIT C158).
    ///
    /// `spec/conformance/protocol.tsv` is emitted from `Rchain/Protocol.lean`'s `replyCatalog`, whose
    /// doc says the `callArity` it records *is* the node's `Definition.arity` — counting the reply
    /// channel where a row has one. Nothing compared the two tables until this test. The corpus
    /// consumer classifies **replies**, so a drifted arity on a replying row makes the receive wait
    /// and is caught; a `kind = none` row (`rho:io:stdout`, `rho:io:stderr`) replies nothing, so the
    /// same drift there was silence on silence — the defect C158 recorded, and the reason `callArity`
    /// was emitted into the corpus at all.
    #[tokio::test]
    async fn every_catalog_urn_arity_matches_the_definition_the_node_installs() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let catalog = catalog_arities();
        assert!(
            !catalog.is_empty(),
            "the emitted catalog has no rows — the corpus or the emitter is broken"
        );
        let bad = arity_mismatches(&catalog, &defs);
        assert!(
            bad.is_empty(),
            "the Lean catalog and the node's installed arities disagree:\n{}",
            bad.join("\n")
        );
    }

    /// The falsifier for the test above: the comparison reports a drift, so a green there is
    /// evidence rather than the absence of a check. The catalog is hand-built against the **real**
    /// installed table, so this needs no fabricated `Definition`.
    #[tokio::test]
    async fn a_drifted_catalog_arity_is_reported() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let drifted = vec![("rho:io:stdout".to_string(), 99i32)];
        let bad = arity_mismatches(&drifted, &defs);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(
            bad[0].contains("rho:io:stdout") && bad[0].contains("99"),
            "the report names the urn and the declared arity: {}",
            bad[0]
        );

        // And a urn the node does not install is reported rather than skipped.
        let unknown = vec![("rho:not:a:urn".to_string(), 1i32)];
        let bad = arity_mismatches(&unknown, &defs);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("no `Definition` installs it"), "{}", bad[0]);
    }

    /// **The fixed channel's refusal must point at the installed contract, not deny it exists**
    /// (AUDIT C114).
    ///
    /// `rho:rchain:multiSigRevVault` is served two ways, and only one of them answers: the
    /// **fixed channel** this handler owns (which refuses, deliberately), and the **registry alias**,
    /// which `GENESIS_ALIASES` maps to the installed `MultiSigRevVault.rho` contract. The refusal's
    /// old wording — "this node does not install the multi-signature vault contract … so there is no
    /// quorum, no co-signers and no confirmation step" — became false the moment the contract was
    /// installed, and it told a caller to give up on a capability the node has. This pins the
    /// correction: the message must name the lookup path that works.
    ///
    /// It is a *string* assertion on purpose. The defect was a message that asserted something untrue
    /// about the tree, and nothing else in the crate can catch that — the handler's behaviour (a
    /// refusal) was right the whole time.
    #[tokio::test]
    async fn the_fixed_channel_refusal_points_at_the_installed_contract() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);
        let multi_sig = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::MULTI_SIG_REV_VAULT)
            .expect("the multi-sig fixed channel has a definition");

        let err = (multi_sig.handler)(
            vec![lpw(vec![
                RhoString::apply("create".to_string()),
                RhoList::apply(vec![]),
            ])],
            DfsPath::root(),
        )
        .await
        .expect_err("the fixed channel answers no method directly");
        let msg = err.to_string();

        assert!(
            msg.contains("registry:lookup"),
            "the refusal must name the path that reaches the contract: {msg}"
        );
        assert!(
            !msg.contains("does not install"),
            "the contract is installed at genesis, so the refusal must not say otherwise: {msg}"
        );
    }

    fn lpw(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars: pars.into_iter().map(SortedProc::new).collect(),
            random_state: Blake2b512Random::new_random(128),
        }
    }

    #[tokio::test]
    async fn blake2b256_hash_contract_replies_with_hash() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let handler = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::BLAKE2B256_HASH)
            .expect("blake2b256Hash definition");
        let input = vec![1u8, 2, 3, 4];
        let ack = FixedChannels::stdout();
        let args = vec![lpw(vec![RhoByteArray::apply(input.clone()), ack.clone()])];
        (handler.handler)(args, DfsPath::root()).await.unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ack);
        assert_eq!(
            produced[0].1.pars,
            vec![SortedProc::new(RhoByteArray::apply(blake2b256::hash(
                &input
            )))]
        );
    }

    /// **`rho:io:http`: the four operations and both refusals.** The whole dispatch — `record`, `get`,
    /// `check`, `height` and the unknown-method arm — was unexecuted: nothing in the tree calls this
    /// urn (law 39's catalog pins its *schema*, not its behaviour), so a port bug in it would have
    /// been invisible. The operations are what a contract uses to write and read a value at a block
    /// height, and the refusals are what a contract gets for a typo or a malformed call.
    #[tokio::test]
    async fn the_http_contract_records_gets_checks_and_reports_its_height() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);
        let handler = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::HTTP)
            .expect("http definition");

        let s = |v: &str| RhoString::apply(v.to_string());
        // `http!("record", ["url", "value", ret])` — the handler's arity-1 shape is
        // `[method, [args…]]`, so one injected par carries the op and one the argument list.
        let call = |op: &str, rest: Vec<Par>| -> Vec<ListParWithRandom> {
            vec![lpw(vec![s(op), RhoList::apply(rest)])]
        };
        let reply = || -> Par {
            let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
            assert_eq!(produced.len(), 1, "each call answers exactly once");
            produced[0].1.pars[0].as_par().clone()
        };
        let clear = || {
            mock.produced
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clear();
        };

        // `record` writes a value at the current block; it answers whether it was newly recorded.
        let ret = FixedChannels::stdout();
        (handler.handler)(
            call("record", vec![s("u"), s("v"), ret.clone()]),
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert_eq!(
            reply(),
            RhoBoolean::apply(true),
            "a fresh record is recorded"
        );
        clear();

        // `get` reads it back, and answers Nil (not an error) for a url that was never recorded.
        (handler.handler)(call("get", vec![s("u"), ret.clone()]), DfsPath::root())
            .await
            .unwrap();
        assert_eq!(
            RhoString::unapply(&reply()),
            Some("v"),
            "the value recorded comes back"
        );
        clear();
        (handler.handler)(
            call("get", vec![s("never-recorded"), ret.clone()]),
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert!(
            RhoNil::unapply(&reply()),
            "an unknown url is Nil, the same answer an unmatched `for` gives"
        );
        clear();

        // `check` compares the stored value, in both directions.
        (handler.handler)(
            call("check", vec![s("u"), s("v"), ret.clone()]),
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert_eq!(reply(), RhoBoolean::apply(true));
        clear();
        (handler.handler)(
            call("check", vec![s("u"), s("other"), ret.clone()]),
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert_eq!(
            reply(),
            RhoBoolean::apply(false),
            "a mismatch is false, not an error"
        );
        clear();

        // `height` reports the block the record was written at.
        (handler.handler)(call("height", vec![s("u"), ret.clone()]), DfsPath::root())
            .await
            .unwrap();
        assert_eq!(
            reply(),
            RhoNumber::apply(0),
            "BlockData::empty() is height 0"
        );
        clear();

        // Both refusals: an operation that does not exist, and a call that is not shaped like one.
        let unknown = (handler.handler)(call("nope", vec![]), DfsPath::root()).await;
        assert!(
            format!("{unknown:?}").contains("unknown method nope"),
            "an unknown operation is refused by name: {unknown:?}"
        );
        let wrong_arity =
            (handler.handler)(call("record", vec![s("url-only")]), DfsPath::root()).await;
        assert!(
            format!("{wrong_arity:?}").contains("expects url, value and a return channel"),
            "and a record missing its value says what it wanted: {wrong_arity:?}"
        );
        let not_a_list =
            (handler.handler)(vec![lpw(vec![s("get"), s("not-a-list")])], DfsPath::root()).await;
        assert!(
            format!("{not_a_list:?}").contains("arguments must be a list"),
            "the argument list is demanded too: {not_a_list:?}"
        );
    }

    /// **The crypto contracts: a verdict, and the shape they demand.** `verify_signature_contract`
    /// builds both `secp256k1Verify` and `ed25519Verify`, and its body had not run — so neither the
    /// verdict `false` nor either refusal had ever been produced. A signature check that answered
    /// `true` for garbage would be the worst bug in the tree; the point of pinning the *false* case
    /// (with data, a bogus signature and a well-formed key) is that "false" is a real verdict and not
    /// a shape the code cannot reach.
    #[tokio::test]
    async fn the_signature_contracts_answer_a_verdict_and_refuse_a_malformed_call() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);
        let verify = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::SECP256K1_VERIFY)
            .expect("secp256k1Verify definition");
        let reply = || -> Par {
            let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
            produced[0].1.pars[0].as_par().clone()
        };
        let clear = || {
            mock.produced
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clear();
        };
        let bytes = |bs: Vec<u8>| RhoByteArray::apply(bs);
        let ack = FixedChannels::stdout();

        // Data, a signature that cannot be one, and a public key of the right length: `false`.
        (verify.handler)(
            vec![lpw(vec![
                bytes(vec![1, 2, 3]),
                bytes(vec![0u8; 65]),
                bytes(vec![2u8; 65]),
                ack.clone(),
            ])],
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert_eq!(
            reply(),
            RhoBoolean::apply(false),
            "a bogus signature verifies as false — and that is a reachable verdict"
        );
        clear();

        // Two refusals: the wrong number of arguments, and arguments that are not byte arrays.
        let arity = (verify.handler)(
            vec![lpw(vec![bytes(vec![1, 2, 3]), ack.clone()])],
            DfsPath::root(),
        )
        .await;
        assert!(
            format!("{arity:?}").contains("secp256k1Verify expects data, signature, public key"),
            "the refusal names the contract and what it wants: {arity:?}"
        );
        let types = (verify.handler)(
            vec![lpw(vec![
                RhoString::apply("not bytes".to_string()),
                bytes(vec![0u8; 65]),
                bytes(vec![2u8; 65]),
                ack.clone(),
            ])],
            DfsPath::root(),
        )
        .await;
        assert!(
            format!("{types:?}").contains("(all as byte arrays)"),
            "and refuses a non-byte-array argument: {types:?}"
        );
    }

    /// **`rho:registry:ops` `buildUri`, and its three ways of not answering.** The op derives a
    /// registry URI from a hash of the argument; an argument that is not a byte array answers the
    /// empty par, and an unknown operation is refused. Both halves matter: the empty-par arm is the
    /// port's convention for "this is not a URI" (a client's `for` simply does not match), and the
    /// refusal is what stops a typo from looking like that convention.
    #[tokio::test]
    async fn the_registry_ops_build_a_uri_and_refuse_what_they_cannot() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);
        let ops = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::REG_OPS)
            .expect("registry ops definition");
        let reply = || -> Par {
            let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
            produced[0].1.pars[0].as_par().clone()
        };
        let ack = FixedChannels::stdout();

        let uri = {
            (ops.handler)(
                vec![lpw(vec![
                    RhoString::apply("buildUri".to_string()),
                    RhoByteArray::apply(vec![1, 2, 3]),
                    ack.clone(),
                ])],
                DfsPath::root(),
            )
            .await
            .unwrap();
            reply()
        };
        assert_eq!(
            RhoUri::unapply(&uri),
            Some(registry::build_uri(&blake2b256::hash(&[1, 2, 3])).as_str()),
            "the URI is the hash of the argument, by the registry's own builder"
        );
        mock.produced
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();

        // A non-byte-array argument: the empty par, which no `for` matches.
        (ops.handler)(
            vec![lpw(vec![
                RhoString::apply("buildUri".to_string()),
                RhoString::apply("not bytes".to_string()),
                ack.clone(),
            ])],
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert!(
            RhoNil::unapply(&reply()),
            "an argument that cannot be a hash answers Nil, not an error"
        );
        mock.produced
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();

        for (label, args) in [
            (
                "an unknown operation",
                vec![
                    RhoString::apply("nope".to_string()),
                    RhoByteArray::apply(vec![1]),
                    ack.clone(),
                ],
            ),
            (
                "the wrong arity",
                vec![RhoString::apply("buildUri".to_string())],
            ),
        ] {
            let err = (ops.handler)(vec![lpw(args)], DfsPath::root()).await;
            assert!(
                format!("{err:?}").contains("registryOps"),
                "{label} is refused by name: {err:?}"
            );
        }
    }

    /// **`sys:authToken:ops` `check`.** The system-auth token is an unforgeable name, and this is the
    /// predicate that recognises it — the answer a contract's capability check branches on. Both
    /// directions in one test, so neither can pass vacuously: the token itself is `true`, and a
    /// *different* unforgeable — and a plain value — are `false` rather than an error.
    #[tokio::test]
    async fn the_sys_auth_token_contract_recognizes_the_token_and_nothing_else() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);
        let ops = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::SYS_AUTHTOKEN_OPS)
            .expect("sys auth token ops definition");
        let ack = FixedChannels::stdout();
        let call = |arg: Par| {
            vec![lpw(vec![
                RhoString::apply("check".to_string()),
                arg,
                ack.clone(),
            ])]
        };
        let reply = || -> Par {
            let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
            produced[0].1.pars[0].as_par().clone()
        };
        let clear = || {
            mock.produced
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clear();
        };

        (ops.handler)(call(RhoSysAuthToken::apply()), DfsPath::root())
            .await
            .unwrap();
        assert_eq!(reply(), RhoBoolean::apply(true), "the token is recognised");
        clear();

        // Another unforgeable is not the token — the case a capability check must not confuse — and
        // neither is an ordinary value.
        (ops.handler)(
            call(Par {
                unforgeables: vec![GUnforgeable::GPrivate(GPrivate { id: vec![7u8; 32] })],
                ..Default::default()
            }),
            DfsPath::root(),
        )
        .await
        .unwrap();
        assert_eq!(
            reply(),
            RhoBoolean::apply(false),
            "a private name is not the token"
        );
        clear();
        (ops.handler)(call(RhoNumber::apply(1)), DfsPath::root())
            .await
            .unwrap();
        assert_eq!(reply(), RhoBoolean::apply(false), "nor is an integer");
        clear();

        let err = (ops.handler)(
            vec![lpw(vec![RhoString::apply("nope".to_string())])],
            DfsPath::root(),
        )
        .await;
        assert!(
            format!("{err:?}").contains("sysAuthTokenOps"),
            "an unknown operation is refused by name: {err:?}"
        );
    }

    #[tokio::test]
    async fn registry_insert_arbitrary_then_lookup_round_trips() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let insert = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::REG_INSERT_RANDOM)
            .expect("insertArbitrary definition");
        let data = RhoNumber::apply(42);
        let ret = FixedChannels::stdout();
        (insert.handler)(vec![lpw(vec![data.clone(), ret.clone()])], DfsPath::root())
            .await
            .unwrap();

        let uri = {
            let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
            assert_eq!(produced.len(), 1);
            assert_eq!(produced[0].0.as_par(), &ret);
            produced[0].1.pars[0].clone()
        };
        assert!(
            RhoUri::unapply(uri.as_par()).is_some(),
            "insertArbitrary returns a URI"
        );

        let lookup = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::REG_LOOKUP)
            .expect("lookup definition");
        let ret2 = FixedChannels::stdout_ack();
        (lookup.handler)(
            vec![lpw(vec![uri.as_par().clone(), ret2.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 2);
        assert_eq!(produced[1].0.as_par(), &ret2);
        // The stored value goes out on its own, unpaired with the uri (C18) — a pair would bind to
        // the name at the consumer and make its send a no-op.
        let sent = &produced[1].1.pars[0];
        assert_eq!(sent.as_par(), &data, "lookup sends the stored value alone");
    }

    #[tokio::test]
    async fn pos_get_bonds_returns_bonds_map() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let native = NativeSystemState::new(Arc::new(
            rchain_rspace::native_store::InMemNativeStore::empty(),
        ));
        let mut bonds = std::collections::BTreeMap::new();
        bonds.insert(
            rchain_models::validator::Validator::new([1u8; 65]),
            NonNegI64::try_from(10).unwrap(),
        );
        bonds.insert(
            rchain_models::validator::Validator::new([2u8; 65]),
            NonNegI64::try_from(20).unwrap(),
        );
        native.set_bonds(&bonds);

        let charging = ChargingRSpace::new(
            mock.clone(),
            Arc::new(crate::accounting::CostAccounting::from_initial(
                crate::accounting::Costs::unsafe_max(),
            )),
        );
        let dispatcher = Arc::new(RholangAndScalaDispatcher::new(
            std::collections::BTreeMap::new(),
        ));
        let block_data = Arc::new(Mutex::new(BlockData::empty()));
        let sp = SystemProcesses::new(charging, dispatcher, block_data, Arc::new(native));
        let defs = sp.definitions();

        let pos = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::POS)
            .expect("pos definition");
        let ret = FixedChannels::stdout();
        let args = vec![lpw(vec![
            RhoString::apply("getBonds".to_string()),
            RhoList::apply(vec![ret.clone()]),
        ])];
        (pos.handler)(args, DfsPath::root()).await.unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        let map = RhoMap::unapply(produced[0].1.pars[0].as_par()).expect("getBonds returns a map");
        assert_eq!(map.len(), 2);
    }

    #[tokio::test]
    async fn rev_vault_transfer_uses_deployer_capability_and_deposit_is_rejected() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        // Build the system processes with an explicit native store so vault balances can be
        // seeded by deployer-derived address.
        let charging = ChargingRSpace::new(
            mock.clone(),
            Arc::new(crate::accounting::CostAccounting::from_initial(
                crate::accounting::Costs::unsafe_max(),
            )),
        );
        let dispatcher = Arc::new(RholangAndScalaDispatcher::new(
            std::collections::BTreeMap::new(),
        ));
        let block_data = Arc::new(Mutex::new(BlockData::empty()));
        let native_store = Arc::new(rchain_rspace::native_store::InMemNativeStore::empty());
        let native_state = Arc::new(NativeSystemState::new(native_store));
        let sp = SystemProcesses::new(charging, dispatcher, block_data, native_state.clone());
        let defs = sp.definitions();
        let vault = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::REV_VAULT)
            .expect("revVault definition");

        let alice_id = RhoDeployerId::apply(vec![1; 65]);
        let alice = RevAddress::from_deployer_id(&[1; 65])
            .expect("alice address")
            .to_base58();
        let bob = RevAddress::from_deployer_id(&[2; 65])
            .expect("bob address")
            .to_base58();
        native_state.set_vault_balance(&alice, NonNegI64::try_from(100).unwrap());
        native_state.set_vault_balance(&bob, NonNegI64::try_from(50).unwrap());

        // deposit is a genesis-only mint now; a deploy call must be rejected.
        let ret = FixedChannels::stdout();
        let err = (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("deposit".to_string()),
                RhoList::apply(vec![
                    RhoString::apply(alice.clone()),
                    RhoNumber::apply(100),
                    ret,
                ]),
            ])],
            DfsPath::root(),
        )
        .await
        .expect_err("deposit must be rejected");
        assert!(err.to_string().contains("deposit is not callable"), "{err}");

        // transfer(*aliceDeployerId, bob, 30, _) — the from-account is derived from the caller's
        // deployerId, not taken as a forgeable address string.
        let ret = FixedChannels::stdout();
        (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("transfer".to_string()),
                RhoList::apply(vec![
                    alice_id,
                    RhoString::apply(bob.clone()),
                    RhoNumber::apply(30),
                    ret,
                ]),
            ])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        // getBalance(alice, ret) and getBalance(bob, ret) — reads stay address-keyed.
        let alice_ret = FixedChannels::stdout_ack();
        (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("getBalance".to_string()),
                RhoList::apply(vec![RhoString::apply(alice.clone()), alice_ret.clone()]),
            ])],
            DfsPath::root(),
        )
        .await
        .unwrap();
        let bob_ret = FixedChannels::stdout();
        (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("getBalance".to_string()),
                RhoList::apply(vec![RhoString::apply(bob.clone()), bob_ret.clone()]),
            ])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        // 1 transfer + 2 getBalance = 3 produces (deposit produced nothing).
        assert_eq!(produced.len(), 3);
        // The last two are the getBalance replies.
        assert_eq!(produced[1].0.as_par(), &alice_ret);
        assert_eq!(
            RhoNumber::unapply(produced[1].1.pars[0].as_par()).expect("alice balance"),
            70
        );
        assert_eq!(produced[2].0.as_par(), &bob_ret);
        assert_eq!(
            RhoNumber::unapply(produced[2].1.pars[0].as_par()).expect("bob balance"),
            80
        );

        // A forgeable byte array is not a deployerId: transfer must reject it.
        let ret = FixedChannels::stdout();
        let err = (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("transfer".to_string()),
                RhoList::apply(vec![
                    RhoByteArray::apply(vec![1; 65]),
                    RhoString::apply(bob),
                    RhoNumber::apply(1),
                    ret,
                ]),
            ])],
            DfsPath::root(),
        )
        .await
        .expect_err("transfer with a byte array must be rejected");
        assert!(err.to_string().contains("deployerId"), "{err}");
    }

    #[tokio::test]
    async fn rev_vault_self_transfer_is_a_noop() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let charging = ChargingRSpace::new(
            mock.clone(),
            Arc::new(crate::accounting::CostAccounting::from_initial(
                crate::accounting::Costs::unsafe_max(),
            )),
        );
        let dispatcher = Arc::new(RholangAndScalaDispatcher::new(
            std::collections::BTreeMap::new(),
        ));
        let block_data = Arc::new(Mutex::new(BlockData::empty()));
        let native_state = Arc::new(NativeSystemState::new(Arc::new(
            rchain_rspace::native_store::InMemNativeStore::empty(),
        )));
        let sp = SystemProcesses::new(charging, dispatcher, block_data, native_state.clone());
        let defs = sp.definitions();
        let vault = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::REV_VAULT)
            .expect("revVault definition");

        let alice_id = RhoDeployerId::apply(vec![1; 65]);
        let alice = RevAddress::from_deployer_id(&[1; 65])
            .expect("alice address")
            .to_base58();
        native_state.set_vault_balance(&alice, NonNegI64::try_from(100).unwrap());

        // transfer(alice, alice, 30, _) must succeed and leave the balance unchanged (the Scala
        // purse split/deposit nets to zero); without the guard the read-then-write would double it.
        let ret = FixedChannels::stdout();
        (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("transfer".to_string()),
                RhoList::apply(vec![
                    alice_id,
                    RhoString::apply(alice.clone()),
                    RhoNumber::apply(30),
                    ret,
                ]),
            ])],
            DfsPath::root(),
        )
        .await
        .expect("self-transfer must succeed");

        let balance = native_state
            .vault_balance(&alice)
            .await
            .expect("read balance")
            .expect("vault exists");
        assert_eq!(
            i64::from(balance),
            100,
            "self-transfer must not change the balance"
        );

        // An amount above the balance must still fail (the guard sits after the balance check).
        let ret2 = FixedChannels::stdout();
        let err = (vault.handler)(
            vec![lpw(vec![
                RhoString::apply("transfer".to_string()),
                RhoList::apply(vec![
                    RhoDeployerId::apply(vec![1; 65]),
                    RhoString::apply(alice.clone()),
                    RhoNumber::apply(200),
                    ret2,
                ]),
            ])],
            DfsPath::root(),
        )
        .await
        .expect_err("self-transfer above balance must be rejected");
        assert!(err.to_string().contains("insufficient balance"), "{err}");
    }

    #[tokio::test]
    async fn qucalc_zfa_reports_balance_and_phase() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let zfa = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::QUCALC_ZFA)
            .expect("qucalc:zfa definition");
        let ack = FixedChannels::stdout();

        // ^v = [0, 1] = σ_y · −σ_y = −I: Pauli-closed AND count-balanced -> ZFA, phase −1.
        let twists = RhoList::apply(vec![RhoNumber::apply(0), RhoNumber::apply(1)]);
        (zfa.handler)(vec![lpw(vec![twists, ack.clone()])], DfsPath::root())
            .await
            .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ack);
        let parts = RhoTupleN::unapply(produced[0].1.pars[0].as_par()).expect("(zfa, phase) tuple");
        assert_eq!(parts.len(), 2);
        assert_eq!(RhoBoolean::unapply(&parts[0]), Some(true));
        assert_eq!(RhoNumber::unapply(&parts[1]), Some(-1));
    }

    #[tokio::test]
    async fn qucalc_grant_then_verify_across_deploys() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let grant = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::QUCALC_GRANT)
            .expect("grant definition");
        let verify = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::QUCALC_VERIFY)
            .expect("verify definition");

        // Deploy 1: mint a ZFA-balanced proof (^v) as a capability.
        let ret = FixedChannels::stdout();
        let twists = RhoList::apply(vec![RhoNumber::apply(0), RhoNumber::apply(1)]);
        (grant.handler)(vec![lpw(vec![twists, ret.clone()])], DfsPath::root())
            .await
            .unwrap();

        let cap = {
            let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
            assert_eq!(produced.len(), 1);
            assert_eq!(produced[0].0.as_par(), &ret);
            assert!(
                RhoUri::unapply(produced[0].1.pars[0].as_par()).is_some(),
                "grant returns a capability uri"
            );
            produced[0].1.pars[0].clone()
        };

        // Deploy 2: the capability persists in the native registry across deploys.
        let ret2 = FixedChannels::stdout_ack();
        (verify.handler)(
            vec![lpw(vec![cap.as_par().clone(), ret2.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 2);
        assert_eq!(produced[1].0.as_par(), &ret2);
        assert_eq!(
            RhoBoolean::unapply(produced[1].1.pars[0].as_par()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn qucalc_fuse_mints_syllogism_as_capability() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let fuse = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::QUCALC_FUSE)
            .expect("fuse definition");

        // Thesis ^< (Socrates) ⊕ Antithesis >v (Mortal) via middle term +- -> ^<>v (ZFA).
        let subject = RhoList::apply(vec![RhoNumber::apply(0), RhoNumber::apply(3)]); // ^<
        let predicate = RhoList::apply(vec![RhoNumber::apply(2), RhoNumber::apply(1)]); // >v
        let ret = FixedChannels::stdout();
        (fuse.handler)(
            vec![lpw(vec![subject, predicate, ret.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        let tuple =
            RhoTupleN::unapply(produced[0].1.pars[0].as_par()).expect("(geometry, cap) tuple");
        assert_eq!(tuple.len(), 2);
        let geometry = parse_twists(&tuple[0]).expect("geometry is a twist list");
        assert_eq!(geometry, vec![0u8, 3, 2, 1]); // ^<>v
        assert!(
            RhoUri::unapply(&tuple[1]).is_some(),
            "returns a capability uri"
        );
    }

    #[tokio::test]
    async fn gov_resolve_weights_reports_weight_map() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let resolve = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::GOV_RESOLVE_WEIGHTS)
            .expect("gov:resolveWeights definition");

        // A, B, C; B delegates A, C delegates B. A and C vote directly -> A=2 (self+B), C=1.
        let voters = RhoList::apply(vec![
            RhoString::apply("A".to_string()),
            RhoString::apply("C".to_string()),
        ]);
        let delegations = RhoMap::apply(vec![
            (
                RhoString::apply("B".to_string()),
                RhoString::apply("A".to_string()),
            ),
            (
                RhoString::apply("C".to_string()),
                RhoString::apply("B".to_string()),
            ),
        ]);
        let trust = RhoMap::apply(vec![]);
        let ret = FixedChannels::stdout();
        (resolve.handler)(
            vec![lpw(vec![voters, delegations, trust, ret.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        let w = parse_member_int_map(produced[0].1.pars[0].as_par()).expect("weights map");
        assert_eq!(w.get("A"), Some(&2));
        assert_eq!(w.get("C"), Some(&1));
    }

    #[tokio::test]
    async fn gov_resolve_weights_accepts_deployer_ids() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let resolve = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::GOV_RESOLVE_WEIGHTS)
            .expect("gov:resolveWeights definition");

        // Members identified by deployer-id unforgeables: B delegates A, only A votes.
        let a = RhoDeployerId::apply(vec![0x01]);
        let b = RhoDeployerId::apply(vec![0x02]);
        let a_id = rchain_shared::base16::encode(&[0x01]);
        let voters = RhoList::apply(vec![a.clone()]);
        let delegations = RhoMap::apply(vec![(b, a)]);
        let trust = RhoMap::apply(vec![]);
        let ret = FixedChannels::stdout();
        (resolve.handler)(
            vec![lpw(vec![voters, delegations, trust, ret.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        let w = parse_member_int_map(produced[0].1.pars[0].as_par()).expect("weights map");
        assert_eq!(
            w.get(&a_id),
            Some(&2),
            "A carries its own + B's delegated weight"
        );
    }

    #[tokio::test]
    async fn gov_trust_levels_reports_admin_rooted_levels() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let trust = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::GOV_TRUST_LEVELS)
            .expect("gov:trustLevels definition");

        let ratings = RhoList::apply(vec![
            RhoTupleN::apply(vec![
                RhoString::apply("Alice".to_string()),
                RhoString::apply("Bob".to_string()),
                RhoNumber::apply(3),
            ]),
            RhoTupleN::apply(vec![
                RhoString::apply("Bob".to_string()),
                RhoString::apply("Carol".to_string()),
                RhoNumber::apply(2),
            ]),
        ]);
        let admins = RhoList::apply(vec![RhoString::apply("Alice".to_string())]);
        let ret = FixedChannels::stdout();
        (trust.handler)(
            vec![lpw(vec![ratings, admins, ret.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        let lv = parse_member_int_map(produced[0].1.pars[0].as_par()).expect("levels map");
        assert_eq!(lv.get("Alice"), Some(&5));
        assert_eq!(lv.get("Bob"), Some(&3));
        assert_eq!(lv.get("Carol"), Some(&2));
    }

    #[tokio::test]
    async fn gov_censure_discredits_and_slashes() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let censure = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::GOV_CENSURE)
            .expect("gov:censure definition");

        let censures = RhoList::apply(vec![
            RhoTupleN::apply(vec![
                RhoString::apply("A".to_string()),
                RhoString::apply("D".to_string()),
            ]),
            RhoTupleN::apply(vec![
                RhoString::apply("B".to_string()),
                RhoString::apply("D".to_string()),
            ]),
        ]);
        let levels = RhoMap::apply(vec![
            (RhoString::apply("A".to_string()), RhoNumber::apply(5)),
            (RhoString::apply("B".to_string()), RhoNumber::apply(5)),
            (RhoString::apply("C".to_string()), RhoNumber::apply(5)),
            (RhoString::apply("D".to_string()), RhoNumber::apply(0)),
        ]);
        let vouchers = RhoList::apply(vec![
            RhoTupleN::apply(vec![
                RhoString::apply("A".to_string()),
                RhoString::apply("D".to_string()),
                RhoNumber::apply(2),
            ]),
            RhoTupleN::apply(vec![
                RhoString::apply("B".to_string()),
                RhoString::apply("D".to_string()),
                RhoNumber::apply(1),
            ]),
        ]);
        let ret = FixedChannels::stdout();
        (censure.handler)(
            vec![lpw(vec![censures, levels, vouchers, ret.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        let tuple =
            RhoTupleN::unapply(produced[0].1.pars[0].as_par()).expect("(discredited, levels)");
        assert_eq!(tuple.len(), 2);
        let disc = parse_member_list(&tuple[0]).expect("discredited list");
        assert_eq!(disc, vec!["D".to_string()]);
        let lv = parse_member_int_map(&tuple[1]).expect("levels map");
        assert_eq!(lv.get("A"), Some(&3), "A slashed by 2");
        assert_eq!(lv.get("B"), Some(&4), "B slashed by 1");
        assert_eq!(lv.get("C"), Some(&5));
    }

    /// A governance call naming more members than the bound is refused (audit F-3).
    ///
    /// The four `rho:gov:*` handlers fold over the union of their arguments — `censure` cubically —
    /// and took their counts straight off a deploy with no bound but the message cap. This is the
    /// refusal-at-entry that replaces it: bound the input rather than interrupt the work, which on
    /// this design is not possible (a builtin runs its CPU synchronously and cannot be preempted).
    #[tokio::test]
    async fn a_governance_call_over_the_member_bound_is_refused() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);
        let censure = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::GOV_CENSURE)
            .expect("gov:censure definition");

        let levels_of = |n: usize| {
            RhoMap::apply(
                (0..n)
                    .map(|i| (RhoString::apply(format!("m{i}")), RhoNumber::apply(5)))
                    .collect(),
            )
        };
        let empty = RhoList::apply(vec![]);
        let ack = FixedChannels::stdout();
        let call = |levels| {
            (censure.handler)(
                vec![lpw(vec![empty.clone(), levels, empty.clone(), ack.clone()])],
                DfsPath::root(),
            )
        };

        // The control: a call at a realistic size runs, so the refusal below is about the bound and
        // not about the fixture.
        let small = call(levels_of(4)).await;
        assert!(small.is_ok(), "a small governance call must run: {small:?}");

        // One member past the bound.
        let over = call(levels_of(MAX_UNIVERSE_PROBE + 1)).await;
        assert!(
            over.is_err(),
            "a call naming more than the bound must be refused, not folded"
        );
    }

    /// The bound as the test sees it — deliberately read from the same place the handler reads it, so
    /// a change to either has to face the other.
    const MAX_UNIVERSE_PROBE: usize = SystemProcesses::MAX_GOV_UNIVERSE;

    #[tokio::test]
    async fn gov_tally_ranked_returns_winner() {
        let mock = Arc::new(MockSpace {
            produced: Mutex::new(Vec::new()),
        });
        let (_sp, defs) = mock_system_processes(&mock);

        let tally = defs
            .iter()
            .find(|d| d.body_ref == BodyRefs::GOV_TALLY)
            .expect("gov:tally definition");

        let ballots = RhoMap::apply(vec![
            (
                RhoString::apply("A".to_string()),
                RhoList::apply(vec![
                    RhoString::apply("X".to_string()),
                    RhoString::apply("Y".to_string()),
                ]),
            ),
            (
                RhoString::apply("B".to_string()),
                RhoList::apply(vec![
                    RhoString::apply("Y".to_string()),
                    RhoString::apply("X".to_string()),
                ]),
            ),
            (
                RhoString::apply("C".to_string()),
                RhoList::apply(vec![
                    RhoString::apply("Z".to_string()),
                    RhoString::apply("X".to_string()),
                ]),
            ),
        ]);
        let weights = RhoMap::apply(vec![
            (RhoString::apply("A".to_string()), RhoNumber::apply(2)),
            (RhoString::apply("B".to_string()), RhoNumber::apply(2)),
            (RhoString::apply("C".to_string()), RhoNumber::apply(1)),
        ]);
        let mode = RhoString::apply("ranked".to_string());
        let ret = FixedChannels::stdout();
        (tally.handler)(
            vec![lpw(vec![ballots, weights, mode, ret.clone()])],
            DfsPath::root(),
        )
        .await
        .unwrap();

        let produced = mock.produced.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].0.as_par(), &ret);
        assert_eq!(
            RhoString::unapply(produced[0].1.pars[0].as_par()),
            Some("X")
        );
    }
}
