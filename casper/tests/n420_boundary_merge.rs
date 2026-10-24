//! **#420's discriminator, on the deployed revision `71c6a513d`**: does a merge of a boundary
//! round — three empty blocks and one carrying a deploy, all four running `CloseBlock` — reproduce a
//! state that is a function of its parents?
//!
//! The live evidence (issue #420) is that block 841's pre-state `baad68f4…` is none of its four
//! parents' post-states (`0572bb0a…` ×3, `2ad1ac84…`), all four parents are finalized, and the parent
//! state `dfff86d2…` and all four post-states carry `Root.keys` that the child state does not.
//!
//! The #280 record (`spec/audit/evidence/n280-merge-loses-a-write-results.md`, C248) says the one
//! boundary shape its fix does **not** reach is exactly this fixture: "a boundary that carried a
//! deploy against its empty siblings, which needs an epoch-running fixture". This test is that
//! fixture, built out of the runtime's own `compute_state` and the production merge entry point.
//!
//! **What this asserts, and what it deliberately does not.** It asserts the composition property a
//! child state owes its parents and its base: the merged state carries a user cell that was in the
//! base state, and the merge reports its boundary rejections. It does not assert that the live
//! `Root` loss is reproduced — a simple tuple-space cell is not the r-cash contract's linear
//! `rootState` RMW. A red assertion here is the defect; a green one narrows it to the contract layer,
//! which is the next fixture.

mod common;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use rchain_casper::merging::{BlockIndex, MergeScope, RejectionsMap};
use rchain_casper::runtime_manager::RuntimeManager;
use rchain_casper::system_deploy::SystemDeploy;
use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::public_key::PublicKey;
use rchain_models::block::state_hash::StateHash;
use rchain_models::block_hash::BlockHash;
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_models::validator::Validator;
use rchain_rholang::merging::SidecarRecord;
use rchain_rholang::native_state::NativeSystemState;
use rchain_rholang::scheduler::EffectMode;
use rchain_rholang::system_processes::BlockData;
use rchain_rholang::util::rev_address::RevAddress;
use rchain_rspace::merger::event_log_index::NumberChannelsDiff;
use rchain_shared::refined::NonNegI64;

use common::build_runtime_manager_with_mode;

/// The deployer: `[1u8; 65]` parses as a valid uncompressed secp256k1 point, which `pre_charge`
/// requires.
const DEPLOYER: [u8; 65] = [1u8; 65];
/// The four boundary producers. Distinct senders are what make the four blocks four blocks; each is
/// also the executor `pay_executor` pays.
const VALIDATORS: [[u8; 65]; 4] = [[3u8; 65], [4u8; 65], [5u8; 65], [6u8; 65]];
/// The boundary round. `epoch_length = 10`, so 840 is the live boundary and 10 is the fixture's.
const BOUNDARY: i64 = 10;

fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::from_init(&[0u8; 32])
}

fn deploy(sig: u8, term: &str) -> SignedDeployData {
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
        sig: vec![sig; 65],
        sig_algorithm: "secp256k1".to_string(),
    }
}

/// The state a block starts from: a funded deployer (so `pre_charge` passes).
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

struct Played {
    hash: BlockHash,
    post: Blake2b256Hash,
    index: BlockIndex,
}

/// Play one block through the proposer's own entry point and build the index the merge reads from
/// what the run recorded — the same construction as
/// `deploy_native_write.rs::a_merged_block_reproduces_its_own_post_state`.
async fn play(
    rm: &RuntimeManager,
    hash_byte: u8,
    sender: [u8; 65],
    pre: Blake2b256Hash,
    term: Option<&str>,
    system: &[SystemDeploy],
) -> Played {
    let block_data = BlockData {
        block_number: rchain_shared::refined::BlockHeight::try_from(BOUNDARY).expect("boundary"),
        sender: PublicKey::new(sender.to_vec()),
        seq_num: rchain_shared::refined::SeqNum::try_from(0).expect("seq"),
        timestamp: 0,
    };
    let terms: Vec<SignedDeployData> = term.map(|t| deploy(0xAA, t)).into_iter().collect();
    let (post, user, sys_results) = rm
        .compute_state(&pre, &terms, system, &fixed_rand(), block_data, &pre)
        .await
        .expect("compute_state");

    let usr: Vec<_> = user.iter().map(|u| u.deploy.clone()).collect();
    let sys: Vec<_> = sys_results.iter().map(|s| s.deploy.clone()).collect();
    let mergeable: Vec<NumberChannelsDiff> = user
        .iter()
        .map(|u| u.mergeable.clone())
        .chain(sys_results.iter().map(|s| s.mergeable.clone()))
        .collect();
    let sidecar = match rm
        .load_native_changes(post.as_bytes(), sender.as_ref(), 0)
        .await
        .expect("sidecar read")
        .expect("the block's sidecar")
    {
        SidecarRecord::Attributed(e) => e,
        other => panic!("a played block's sidecar must be attributed; got {other:?}"),
    };
    let executor = RevAddress::from_public_key(&PublicKey::new(sender.to_vec()))
        .expect("executor")
        .to_base58();
    let index = BlockIndex::apply(
        BlockHash::new([hash_byte; 32]),
        &usr,
        &sys,
        pre,
        post,
        rm.get_history_repo(),
        &mergeable,
        sidecar,
        executor,
    )
    .await
    .expect("block index");

    Played {
        hash: BlockHash::new([hash_byte; 32]),
        post,
        index,
    }
}

fn reply_has(reply: &rchain_casper::runtime_manager::CapturedReply, token: &str) -> bool {
    format!("{:?}", reply.data).contains(token)
}

/// **The falsifier.** A pre-boundary block writes `@"c"!("kept")`; the boundary round has four
/// blocks on that same pre-state, three empty and one carrying `@"d"!("new")`, all running
/// `CloseBlock`; the merge of the four is the child's pre-state. The child must contain `@"c"`.
#[tokio::test]
async fn a_boundary_round_merge_keeps_the_base_states_user_write() {
    let rm = build_runtime_manager_with_mode(EffectMode::Sequential).await;
    let start = seed(&rm).await;

    // The pre-boundary write. No system deploy: this is an ordinary block, not a boundary.
    let pre = play(
        &rm,
        0x11,
        VALIDATORS[0],
        start,
        Some(r#"@"c"!("kept")"#),
        &[],
    )
    .await;
    let kept = rm
        .play_exploratory_deploy(
            r#"for (@x <<- @"c") { @"out"!(x) }"#,
            &StateHash::new(*pre.post.as_bytes()),
        )
        .await
        .expect("the control read runs");
    assert!(
        reply_has(&kept, "kept"),
        "control: the pre-state carries the cell the boundary merge must preserve"
    );

    // The boundary round: three empty siblings and one carrying a deploy, all on `pre.post`.
    let mut blocks = Vec::new();
    for (i, sender) in VALIDATORS.iter().enumerate() {
        let term = (i == 3).then_some(r#"@"d"!("new")"#);
        let close = SystemDeploy::close_block(
            BOUNDARY,
            pre.post,
            BTreeMap::new(),
            fixed_rand().split_byte(9),
        );
        blocks.push(play(&rm, 0x20 + i as u8, *sender, pre.post, term, &[close]).await);
    }

    let indexes: BTreeMap<BlockHash, std::sync::Arc<BlockIndex>> = blocks
        .iter()
        .map(|b| (b.hash, std::sync::Arc::new(b.index.clone())))
        .collect();
    let lookup = move |h: BlockHash| {
        let found = indexes.get(&h).cloned();
        async move { found.ok_or_else(|| format!("no index for {h:?}")) }
    };
    let scope = MergeScope {
        final_scope: BTreeSet::new(),
        conflict_scope: blocks.iter().map(|b| b.hash).collect(),
        ancestry: BTreeMap::new(),
    };
    let outcome = MergeScope::merge(
        &scope,
        pre.post,
        &RejectionsMap::new(),
        rm.get_history_repo(),
        &lookup,
        |_| 0,
    )
    .await
    .expect("the boundary round merges");

    let merged = rm
        .play_exploratory_deploy(
            r#"for (@x <<- @"c") { @"out"!(x) }"#,
            &StateHash::new(*outcome.state.as_bytes()),
        )
        .await
        .expect("the merged-state read runs");
    let merged_new = rm
        .play_exploratory_deploy(
            r#"for (@x <<- @"d") { @"out"!(x) }"#,
            &StateHash::new(*outcome.state.as_bytes()),
        )
        .await
        .expect("the merged-state read runs");

    // **The base-state property**: the datum every parent executed on must survive the merge.
    assert!(
        reply_has(&merged, "kept"),
        "the boundary merge lost a user write that was in the base state every parent executed on"
    );

    // **The ledger property**: if the deploy-carrying block's own write is not in the merged state,
    // the merge must name the deploy it dropped. A silent loss is the #280/C248 signature.
    let deploy_sig = vec![0xAAu8; 65];
    let kept_new = reply_has(&merged_new, "new");
    println!(
        "merge: equals_parent={:?} deploy_write_present={kept_new} rejected_is_user={} \
         conflict/kept/rejected={}/{}/{} final_scope_rejected={} rejections_without_conflict={} \
         unapplied_kept_writes={} dropped_native_slots={}",
        blocks.iter().position(|b| b.post == outcome.state),
        outcome.rejected_deploys.contains(&deploy_sig),
        outcome.report.conflict_chains,
        outcome.report.kept_chains,
        outcome.report.rejected_chains,
        outcome.report.final_scope_rejected_chains,
        outcome.report.rejections_without_a_conflict.len(),
        outcome.report.unapplied_kept_writes.len(),
        outcome.report.dropped_native_slots.len(),
    );
    if !kept_new {
        assert!(
            outcome.rejected_deploys.contains(&deploy_sig),
            "the boundary merge dropped the deploy block's user write and did not name its deploy"
        );
    }

    // The report is the ledger #280 promised: the boundary chains contend and the merge says so.
    assert!(
        outcome.report.rejected_chains > 0 || outcome.report.final_scope_rejected_chains > 0,
        "a boundary round with four CloseBlock chains merged with an empty rejection ledger"
    );
}

/// **Does a deploy's three parallel register-shaped RMWs serialize under the deployed scheduler?**
///
/// The live `kernel` deploy (block 825) issues three `Root.register` calls in parallel, each a
/// `for (@s <- rootState) { rootState!(s.set(…)) }` consume-and-replace, and the state after that
/// block carries only one of the three keys. This runs the same shape against a plain map cell and
/// asks what the channel holds afterwards:
///
/// - **one value with all three keys** — the registers serialized; the live split is not the
///   contract's shape and the origin is elsewhere;
/// - **one value with one key** — two registers blocked and were never applied: the unchecked-reply
///   failure an installer that does not read every ack cannot see;
/// - **several values** — the channel is a bag and `keys`/`get` bind one version.
///
/// The deployed default is `dfs` (`defaults.conf: effect-scheduler = dfs`), so `Sequential` is the
/// mode that has to explain the live state; `RelaxedValidated` is the other block-path mode.
#[tokio::test]
async fn parallel_register_shaped_rmws_leave_a_countable_channel() {
    for mode in [EffectMode::Sequential, EffectMode::RelaxedValidated] {
        let rm = build_runtime_manager_with_mode(mode).await;
        let start = seed(&rm).await;
        let base = play(
            &rm,
            0x31,
            VALIDATORS[0],
            start,
            Some(r#"@"s"!({"a": 1})"#),
            &[],
        )
        .await;
        let rw = play(
            &rm,
            0x32,
            VALIDATORS[1],
            base.post,
            Some(
                r#"for (@m <- @"s") { @"s"!(m.set("b", 2)) } |
                   for (@m <- @"s") { @"s"!(m.set("c", 3)) } |
                   for (@m <- @"s") { @"s"!(m.set("d", 4)) }"#,
            ),
            &[],
        )
        .await;
        let drain = rm
            .play_exploratory_deploy(
                r#"new drain in {
                     contract drain(@unit) = { for (@x <- @"s") { @"out"!(x) | drain!(Nil) } }
                     | drain!(Nil)
                   }"#,
                &StateHash::new(*rw.post.as_bytes()),
            )
            .await
            .expect("the drain runs");
        println!(
            "parallel-RMW mode={mode:?} values={} data={:?}",
            drain.data.len(),
            format!("{:?}", drain.data)
        );
        assert!(
            !drain.data.is_empty(),
            "the drain found no value on the channel ({mode:?})"
        );
    }
}

/// **The minimal root cause.** A block whose deploy does a linear consume-and-replace on a channel
/// (`for (@x <- @"s") { @"s"!("b") }`) commits **one** value on `@"s"`. Merging that block onto its
/// pre-state — a single-block merge, no boundary, no siblings — duplicates the produced value: the
/// merged state has **two** copies of `"b"` and equals neither parent. That is the mechanism behind
/// #420's Root key sets: the Root contract is exactly this shape, so after any merge its state cell
/// is a two-value bag, and `keys`/`get` (persistent peeks) bind one of the two.
///
/// The existing round-trip guard (`a_merged_block_reproduces_its_own_post_state`) uses a **native**
/// write (`trust`), so it cannot see this. This is the tuple-space counterpart.
#[ignore = "falsifier: red on 71c6a513d — the merge duplicates a tuple-space produce; enable with the fix"]
#[tokio::test]
async fn merging_a_tuple_space_rmw_block_duplicates_its_produce() {
    const DRAIN: &str = r#"new drain in {
        contract drain(@unit) = { for (@x <- @"s") { @"out"!(x) | drain!(Nil) } }
        | drain!(Nil)
    }"#;
    let rm = build_runtime_manager_with_mode(EffectMode::Sequential).await;
    let start = seed(&rm).await;
    let base = play(&rm, 0x71, VALIDATORS[0], start, Some(r#"@"s"!("a")"#), &[]).await;
    let rw = play(
        &rm,
        0x72,
        VALIDATORS[1],
        base.post,
        Some(r#"for (@x <- @"s") { @"s"!("b") }"#),
        &[],
    )
    .await;

    let base_count = rm
        .play_exploratory_deploy(DRAIN, &StateHash::new(*base.post.as_bytes()))
        .await
        .expect("drain the base")
        .data
        .len();
    let block_count = rm
        .play_exploratory_deploy(DRAIN, &StateHash::new(*rw.post.as_bytes()))
        .await
        .expect("drain the block")
        .data
        .len();

    let indexes: BTreeMap<BlockHash, std::sync::Arc<BlockIndex>> =
        BTreeMap::from([(rw.hash, std::sync::Arc::new(rw.index.clone()))]);
    let lookup = move |h: BlockHash| {
        let found = indexes.get(&h).cloned();
        async move { found.ok_or_else(|| format!("no index for {h:?}")) }
    };
    let scope = MergeScope {
        final_scope: BTreeSet::new(),
        conflict_scope: BTreeSet::from([rw.hash]),
        ancestry: BTreeMap::new(),
    };
    let outcome = MergeScope::merge(
        &scope,
        base.post,
        &RejectionsMap::new(),
        rm.get_history_repo(),
        &lookup,
        |_| 0,
    )
    .await
    .expect("the single-block merge");
    let merged_reply = rm
        .play_exploratory_deploy(DRAIN, &StateHash::new(*outcome.state.as_bytes()))
        .await
        .expect("drain the merged state");
    let merged_count = merged_reply.data.len();

    use rchain_rspace::merger::event_log_merging_logic::produces_affected;
    for chain in &rw.index.deploy_chains {
        let idx = &chain.event_log_index;
        let affected = produces_affected(idx);
        println!(
            "produce sets: linear={:?} consumed={:?} affected={:?}",
            idx.produces_linear
                .iter()
                .map(|p| (p.channels_hash.to_hex(), p.hash.to_hex(), p.persistent))
                .collect::<Vec<_>>(),
            idx.produces_consumed
                .iter()
                .map(|p| (p.channels_hash.to_hex(), p.hash.to_hex(), p.persistent))
                .collect::<Vec<_>>(),
            affected
                .iter()
                .map(|p| (p.channels_hash.to_hex(), p.hash.to_hex(), p.persistent))
                .collect::<Vec<_>>(),
        );
    }
    println!(
        "base values={base_count} block post values={block_count} merged values={merged_count} \
         merged_equals_block_post={} data={:?}",
        outcome.state == rw.post,
        format!("{:?}", merged_reply.data)
    );
    assert_eq!(base_count, 1, "the base block leaves one value");
    assert_eq!(block_count, 1, "the block's own post leaves one value");
    assert_eq!(
        merged_count, 1,
        "the merge duplicated the block's produced value: the block committed one value on `@\"s\"` \
         and the merge produced {merged_count}"
    );
}
