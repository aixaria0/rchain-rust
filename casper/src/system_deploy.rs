//! System deploy types + concrete deploys (port of `casper/rholang/types/SystemDeploy*.scala`
//! and `casper/rholang/sysdeploys/`).

use std::collections::{BTreeMap, BTreeSet};

use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::public_key::PublicKey;
use rchain_models::ast::Par;
use rchain_models::block::state_hash::StateHash;
use rchain_models::casper::protocol::casper_message::Event;
use rchain_models::rholang::RhoType::{RhoBoolean, RhoString, RhoTupleN};
use rchain_models::validator::Validator;
use rchain_shared::refined::NonNegI64;

/// A user-level system-deploy error (port of `SystemDeployUserError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemDeployUserError(pub String);

/// A fatal platform failure (port of `SystemDeployPlatformFailure`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemDeployPlatformFailure {
    UnexpectedResult(Vec<Par>),
    UnexpectedSystemErrors(String),
    GasRefundFailure(String),
    ConsumeFailed,
}

/// Accumulated deploy events + mergeable channels (port of `EvalCollector`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvalCollector {
    pub event_log: Vec<Event>,
    pub mergeable_channels: BTreeSet<Par>,
}

impl EvalCollector {
    pub fn add(&self, log: &[Event], merge_chs: &BTreeSet<Par>) -> EvalCollector {
        let mut event_log = self.event_log.clone();
        event_log.extend(log.iter().cloned());
        let mut mergeable_channels = self.mergeable_channels.clone();
        mergeable_channels.extend(merge_chs.iter().cloned());
        EvalCollector {
            event_log,
            mergeable_channels,
        }
    }
}

/// The outcome of playing a system deploy (port of `SystemDeployResult`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemDeployResult<A> {
    PlaySucceeded {
        /// The post-state the deploy produced — typed (deferred item 1b) so the one thing every
        /// caller does with it (hand it to the next step as a state to open) cannot be given 31
        /// bytes.
        state_hash: StateHash,
        event_log: Vec<Event>,
        mergeable_channels: BTreeMap<rchain_crypto::hash::blake2b256_hash::Blake2b256Hash, i64>,
        result: A,
    },
    PlayFailed {
        event_log: Vec<Event>,
        error_msg: String,
    },
}

/// A system deploy: the rholang source plus its normalizer environment (port of `SystemDeploy`).
///
/// Rust-first: the pre-charge/refund/close-block/slash system deploys are now **native** operations
/// (see [`NativeSystemDeployOp`]); `source`/`normalizer_env`/`return_channel` remain only for the
/// legacy rholang path, which is no longer constructed.
pub struct SystemDeploy {
    pub source: &'static str,
    pub normalizer_env: BTreeMap<String, Par>,
    pub rand: Blake2b512Random,
    pub return_channel: Par,
    /// A native system-deploy operation; `Some` makes this deploy bypass the rholang source path.
    pub op: Option<NativeSystemDeployOp>,
}

/// A native system-deploy operation (rust-first replacement for the rholang PoS/registry
/// system-deploy sources; the Scala sources are a checklist only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeSystemDeployOp {
    /// **`amount` is `NonNegI64`, and that is the fix for AUDIT C109, not decoration.** A charge is a
    /// debit subtracted from the deployer's vault, so a *negative* amount inverts it into a credit —
    /// `balance - (-n) == balance + n` — while `credit_pos_vault` no-ops on the negative, leaving the
    /// staking vault untouched. The deployer is simply up by `n`, from nothing. The ingress guard
    /// caught the deploy path; this type is what makes the block-validation and replay paths
    /// (which read a *peer's* `ProcessedSystemDeploy` off the wire) structurally unable to carry one.
    PreCharge {
        deployer: PublicKey,
        amount: NonNegI64,
    },
    /// `Pos.rhox`'s `refundDeploy` is called with only the amount and reads the deployer back out of
    /// the `currentDeployerData` cell that `chargeDeploy` filled. This port carries the deployer in
    /// the deploy instead — same rule, stated in the type rather than in a mutable cell.
    Refund {
        deployer: PublicKey,
        amount: NonNegI64,
    },
    /// `pre_state_hash` is the **block's** pre-state, carried here rather than threaded through the
    /// evaluation chain, and it is the entropy the *next* epoch's active-set draw is anchored to
    /// (`rholang/src/native_state.rs`'s `close_block`, step 5). It is not the deploy's `rand`, and the
    /// difference is the whole point: `rand` is `hash(shard_id, block_number, sender, pre_state_hash)`
    /// computed at the moment of use, so a proposer can reroll it freely by proposing a different
    /// block — it is the seed this change exists to stop using. A block's `pre_state_hash` cannot be
    /// chosen at the height that draws: it is fixed by the block the drawing proposer is extending.
    CloseBlock {
        block_number: i64,
        pre_state_hash: Blake2b256Hash,
    },
    Slash {
        validator: Validator,
    },
}

impl SystemDeploy {
    pub fn pre_charge(amount: NonNegI64, pk: &PublicKey, rand: Blake2b512Random) -> SystemDeploy {
        SystemDeploy {
            source: "",
            normalizer_env: BTreeMap::new(),
            rand,
            return_channel: Par::default(),
            op: Some(NativeSystemDeployOp::PreCharge {
                deployer: pk.to_owned(),
                amount,
            }),
        }
    }

    pub fn refund(deployer: &PublicKey, amount: NonNegI64, rand: Blake2b512Random) -> SystemDeploy {
        SystemDeploy {
            source: "",
            normalizer_env: BTreeMap::new(),
            rand,
            return_channel: Par::default(),
            op: Some(NativeSystemDeployOp::Refund {
                deployer: deployer.to_owned(),
                amount,
            }),
        }
    }

    pub fn close_block(
        block_number: i64,
        pre_state_hash: Blake2b256Hash,
        rand: Blake2b512Random,
    ) -> SystemDeploy {
        SystemDeploy {
            source: "",
            normalizer_env: BTreeMap::new(),
            rand,
            return_channel: Par::default(),
            op: Some(NativeSystemDeployOp::CloseBlock {
                block_number,
                pre_state_hash,
            }),
        }
    }

    pub fn slash(validator: &Validator, rand: Blake2b512Random) -> SystemDeploy {
        SystemDeploy {
            source: "",
            normalizer_env: BTreeMap::new(),
            rand,
            return_channel: Par::default(),
            op: Some(NativeSystemDeployOp::Slash {
                validator: *validator,
            }),
        }
    }
}

/// Interpret the `(Bool, Either[String, Nil])` result of the charge/refund/close/slash deploys
/// (port of their shared `processResult`).
///
/// The `Either[String, Nil]` is a bare `GString` on `Left` (the error message) or `Nil` on `Right`;
/// a `(true, _)` result succeeds, `(false, Left(msg))` fails with `msg`, and anything else fails
/// with `<no cause>`.
pub fn process_bool_result(output: &Par) -> Result<(), SystemDeployUserError> {
    let parts = RhoTupleN::unapply(output)
        .ok_or_else(|| SystemDeployUserError("<no cause>".to_string()))?;
    let success = parts
        .first()
        .and_then(RhoBoolean::unapply)
        .ok_or_else(|| SystemDeployUserError("<no cause>".to_string()))?;
    if success {
        return Ok(());
    }
    let error = parts
        .get(1)
        .and_then(RhoString::unapply)
        .map(|s| SystemDeployUserError(s.to_string()))
        .unwrap_or_else(|| SystemDeployUserError("<no cause>".to_string()));
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `NonNegI64` for test amounts — the charge path takes the refinement since AUDIT C109.
    fn nn(v: i64) -> NonNegI64 {
        NonNegI64::try_from(v).expect("a test amount is non-negative")
    }

    #[test]
    fn pre_charge_is_native() {
        let pk = PublicKey::new(vec![1u8; 65]);
        let rand = Blake2b512Random::new_random(128);
        let d = SystemDeploy::pre_charge(nn(100), &pk, rand);
        assert_eq!(
            d.op,
            Some(NativeSystemDeployOp::PreCharge {
                deployer: pk,
                amount: nn(100)
            })
        );
    }

    /// The refund carries its payer. The Scala contract reads the deployer back out of the
    /// `currentDeployerData` cell that `chargeDeploy` filled (`Pos.rhox:425`), because its
    /// `refundDeploy` is called with only the amount; here the payer is a field of the deploy, so a
    /// refund cannot be paid to anyone but the account the pre-charge took the phlo from.
    #[test]
    fn refund_carries_its_payer() {
        let pk = PublicKey::new(vec![2u8; 65]);
        let rand = Blake2b512Random::new_random(128);
        let d = SystemDeploy::refund(&pk, nn(70), rand);
        assert_eq!(
            d.op,
            Some(NativeSystemDeployOp::Refund {
                deployer: pk,
                amount: nn(70)
            })
        );
    }

    #[test]
    fn process_bool_result_interprets_tuple() {
        use rchain_models::rholang::RhoType::{RhoBoolean, RhoNil, RhoString, RhoTupleN};

        // (true, _) succeeds regardless of the Either.
        let ok = RhoTupleN::apply(vec![RhoBoolean::apply(true), RhoNil::apply()]);
        assert_eq!(process_bool_result(&ok), Ok(()));

        // (false, Left("boom")) fails with the message.
        let fail = RhoTupleN::apply(vec![
            RhoBoolean::apply(false),
            RhoString::apply("boom".to_string()),
        ]);
        assert_eq!(
            process_bool_result(&fail),
            Err(SystemDeployUserError("boom".to_string()))
        );

        // (false, Right(Nil)) fails with no cause.
        let fail_nil = RhoTupleN::apply(vec![RhoBoolean::apply(false), RhoNil::apply()]);
        assert_eq!(
            process_bool_result(&fail_nil),
            Err(SystemDeployUserError("<no cause>".to_string()))
        );

        // A malformed result fails with no cause.
        assert_eq!(
            process_bool_result(&Par::default()),
            Err(SystemDeployUserError("<no cause>".to_string()))
        );
    }
}
