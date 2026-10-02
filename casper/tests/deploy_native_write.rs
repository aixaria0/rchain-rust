//! **A deploy's native-state write must be readable through the block path** — written as the
//! falsifier for AUDIT C205 / issue #203.
//!
//! **It passes, and that is the result, not a disappointment.** The test was written to be red on the
//! tree that has the defect; it is green, which eliminates the layer it exercises. The in-process
//! block path — `RuntimeManager::compute_state`, which is the proposer's own entry point — puts a
//! user deploy's native write into the post-state the block commits to. So the loss is **above** this
//! layer, in the node's pipeline, and no future investigation should start by re-reading this code.
//!
//! What survives as value here is that elimination, kept as a regression guard: whatever the fix turns
//! out to be, it must not make *this* stop holding.
//!
//! The live measurement that stands in for a red version of it is in
//! `spec/audit/evidence/n203-native-write-probe.md`, and it is a weaker statement than "the write is
//! lost" — see that file's correction.

mod common;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use rchain_casper::runtime_manager::RuntimeManager;
use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::public_key::PublicKey;
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_models::validator::Validator;
use rchain_rholang::native_state::NativeSystemState;
use rchain_rholang::scheduler::EffectMode;
use rchain_rholang::system_processes::BlockData;
use rchain_rholang::util::rev_address::RevAddress;
use rchain_shared::refined::NonNegI64;

use common::{build_runtime_manager_with_mode, fringe_state};

use rchain_casper::system_deploy::SystemDeploy;
use rchain_rholang::native_state::{pos_epoch_seed_key, pos_trusted_key};
use rchain_rspace::native_store::PREFIX_POS;

/// The deployer: `[1u8; 65]` parses as a valid uncompressed secp256k1 point, which the pre-charge
/// path of `compute_state` requires (the same key `scheduler.rs` uses).
const DEPLOYER: [u8; 65] = [1u8; 65];
/// The key the deploy trusts. Distinct from the deployer, so the write shows up as a difference in a
/// set rather than as a changed count.
const NEWCOMER: [u8; 65] = [2u8; 65];

fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::from_init(&[0u8; 32])
}

fn deploy(term: &str) -> SignedDeployData {
    SignedDeployData {
        data: DeployData {
            attachments: Vec::new(),
            term: term.to_string(),
            timestamp: 0,
            phlo_price: 1,
            phlo_limit: 90_000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
        },
        deployer: DEPLOYER.to_vec(),
        sig: Vec::new(),
        sig_algorithm: "secp256k1".to_string(),
    }
}

/// The state a block starts from: a funded deployer (so `pre_charge` passes) that is also a trusted
/// stakeholder (so `trust` is reachable and writes a leaf instead of refusing).
///
/// **`trust` is the op for a reason** — it is the one jimscarver's #74 reproduction used
/// (`called: trust` / `ok` / `getTrusted -> 2 entries`), so this test is the in-process twin of that
/// live measurement rather than a different experiment that happens to look similar.
async fn seed(rm: &RuntimeManager) -> Blake2b256Hash {
    let native = NativeSystemState::new(rm.runtime().native_store());
    let pk = PublicKey::new(DEPLOYER.to_vec());
    let addr = RevAddress::from_public_key(&pk)
        .expect("the test key is a valid address")
        .to_base58();
    native.set_vault_balance(
        &addr,
        NonNegI64::try_from(1_000_000_000).expect("a test balance"),
    );
    native.set_trusted(&BTreeSet::from([Validator::new(DEPLOYER)]));
    rm.runtime()
        .create_checkpoint()
        .await
        .expect("seeding checkpoint")
        .root
}

/// The term: trust a newcomer, and consume the op's own reply so the reduction is complete.
fn trust_term(newcomer: &[u8; 65]) -> String {
    format!(
        r#"new pos(`rho:rchain:pos`), deployerId(`rho:rchain:deployerId`), ret, out in {{
             pos!("trust", *deployerId, "{}".hexToBytes(), *ret) |
             for (@r <- ret) {{ out!(r) }}
           }}"#,
        rchain_shared::base16::encode(newcomer)
    )
}

/// **A `trust` dispatched from a user deploy is in the trusted set once the block that carried it has
/// committed** — read through the same store the node reads.
///
/// Green on the tree that has the live defect, and the doc above says why that is the useful result.
#[tokio::test]
async fn a_users_trust_is_readable_after_its_block_commits() {
    let rm = build_runtime_manager_with_mode(EffectMode::Sequential).await;
    let start = seed(&rm).await;

    let (post_state, user, _system) = rm
        .compute_state(
            &start,
            &[deploy(&trust_term(&NEWCOMER))],
            &[],
            &fixed_rand(),
            BlockData::empty(),
            &fringe_state(1),
        )
        .await
        .expect("compute_state");

    // The deploy succeeded, so this is not a "the deploy was refused" story, and nothing downstream
    // can treat what the live node shows as a refusal.
    assert!(
        user[0].eval_result.succeeded(),
        "the trust deploy must succeed: {:?}",
        user[0].eval_result.errors
    );

    // Read the way a node reads it — through the runtime's own native store, which is at the
    // post-state the returned hash names. **This is where the live node disagrees**: the same write is
    // absent from a *deploy's* read of that state (see this file's doc), so what this assertion pins
    // is the layer the loss is *above*, not the loss.
    let native = NativeSystemState::new(rm.runtime().native_store());
    let trusted = native
        .trusted()
        .await
        .expect("the trusted leaf is readable");
    assert!(
        trusted.contains(&Validator::new(NEWCOMER)),
        "a deploy's native write must survive its block: the trusted set is {trusted:?} after a \
         successful trust of {:?}, and the post-state hash was {post_state:?}",
        Validator::new(NEWCOMER)
    );
}

/// **C207's discriminator.** Does the *sidecar* a block saves carry the user deploy's write, or only
/// the last checkpoint's?
///
/// Why this is the test and not a merge: the live symptom is an **asymmetry** — a system deploy's
/// native write survives a merging network, a user deploy's does not — and the code says that
/// asymmetry is exactly what a sidecar carrying *one checkpoint's* set looks like.
/// `block_on` (`casper/src/runtime_manager.rs:1099-1137`) takes 1 + M hard checkpoints per block, and
/// `create_checkpoint` **replaces** `last_native_changes` (`rspace/src/rspace.rs:585-588`). So a bare
/// `last_native_changes()` read *after the block has played* returns the **last** checkpoint's set —
/// the system deploys' — and the user deploys' survives only because `block_on:1119` captures it into
/// a local first. A call site that reads it once afterwards gets the system half only.
///
/// **Neither the block nor this test has a merge in it.** The assertion is on what the block *saved*,
/// which is the layer any merge later re-applies, so it separates "the capture is lossy" from "the
/// capture is fine and the merge drops it" without building a DAG.
///
/// The second assertion is a **control**: a block with a system deploy must carry *its* write too. A
/// sidecar that is empty fails both and says something different from one that has the system half
/// and not the user half — and the second is the live asymmetry's signature.
#[tokio::test]
async fn the_sidecar_a_block_saves_carries_the_user_deploys_write_not_only_the_systems() {
    let rm = build_runtime_manager_with_mode(EffectMode::Sequential).await;
    let start = seed(&rm).await;
    let block_data = BlockData::empty();
    let sender = block_data.sender.bytes().to_vec();

    // One user deploy that writes a native leaf, and **one system deploy**, so the block takes the
    // 1 + M checkpoints the live path takes. Without the system deploy there is one checkpoint and
    // the sidecar cannot show the asymmetry at all — which is why the first version of this test
    // passed on a tree that has the live defect.
    let (post_state, user, _system) = rm
        .compute_state(
            &start,
            &[deploy(&trust_term(&NEWCOMER))],
            &[SystemDeploy::close_block(
                1,
                fringe_state(1),
                BTreeMap::new(),
                fixed_rand(),
            )],
            &fixed_rand(),
            block_data,
            &fringe_state(1),
        )
        .await
        .expect("compute_state");
    assert!(
        user[0].eval_result.succeeded(),
        "the trust deploy must succeed: {:?}",
        user[0].eval_result.errors
    );

    let sidecar = rm
        .load_native_changes(post_state.as_bytes(), &sender, 0)
        .await
        .expect("the sidecar read is not a store error")
        .expect("compute_state must have saved a sidecar for the block it just played");
    let slots: Vec<(u8, rchain_crypto::hash::blake2b256_hash::Blake2b256Hash)> =
        sidecar.iter().map(|a| a.slot()).collect();

    assert!(
        slots.contains(&(PREFIX_POS, pos_epoch_seed_key())),
        "control: the *system* deploy's write must be in the sidecar — a sidecar missing this is \
         empty rather than partial, which is a different finding. Slots: {slots:?}"
    );
    assert!(
        slots.contains(&(PREFIX_POS, pos_trusted_key())),
        "**C207**: the block's sidecar carries the system deploy's write and not the user deploy's, \
         which is the live asymmetry exactly — a merge re-applying this set drops the trust. Slots: \
         {slots:?}"
    );
}
