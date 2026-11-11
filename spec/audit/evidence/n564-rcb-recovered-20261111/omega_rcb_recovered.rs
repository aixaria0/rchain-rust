//! #564 M22–M24: authenticated recovered deploys, SYNTHETIC pre-state.
//! The original block body, its processed trace, and its pre-state are not
//! supplied by these tests. A matching cost is not historical closure.
mod common;
use common::fringe_state;
use std::collections::BTreeMap;

use rchain_casper::genesis::contracts::Vault;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::public_key::PublicKey;
use rchain_models::casper::protocol::casper_message::{
    Event, ProcessedDeploy, ProcessedSystemDeploy, SignedDeployData,
};
use rchain_rholang::native_state::PosGenesis;
use rchain_rholang::scheduler::EffectMode;
use rchain_rholang::system_processes::BlockData;
use rchain_rholang::util::rev_address::RevAddress;
use rchain_shared::refined::NonNegI64;

const FIXTURES: &[(&str, &[u8])] = &[
    ("registry-1e4f", include_bytes!("rcb_recovered/registry-1e4f.bin")),
    ("registry-7c03", include_bytes!("rcb_recovered/registry-7c03.bin")),
    ("registry-short-9915", include_bytes!("rcb_recovered/registry-short-9915.bin")),
    ("simple-2e37", include_bytes!("rcb_recovered/simple-2e37.bin")),
    ("probe-98dd", include_bytes!("rcb_recovered/probe-98dd.bin")),
];

fn fixtures() -> Vec<(&'static str, SignedDeployData)> {
    FIXTURES.iter().map(|(name, bytes)| {
        let d = SignedDeployData::from_bytes(bytes).expect("production protobuf decode");
        assert_eq!(d.to_bytes().as_slice(), *bytes, "canonical wire round trip");
        assert!(d.verify_signature(), "authentic recovered signature");
        (*name, d)
    }).collect()
}

#[test]
fn omega_m22_production_signature_verification() {
    for (name, d) in fixtures() {
        let mut term_control = d.clone();
        term_control.data.term.push(' ');
        let mut timestamp_control = d.clone();
        timestamp_control.data.timestamp += 1;
        let mut sig_control = d.clone();
        *sig_control.sig.last_mut().unwrap() ^= 1;
        assert!(!term_control.verify_signature());
        assert!(!timestamp_control.verify_signature());
        assert!(!sig_control.verify_signature());
        println!("OMEGA_M22 name={name} signature_valid=true term_tamper=false timestamp_tamper=false signature_tamper=false term_bytes={} timestamp={} phlo_limit={} valid_after={} shard={}",
            d.data.term.len(), d.data.timestamp, d.data.phlo_limit,
            d.data.valid_after_block_number, d.data.shard_id);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    costs: Vec<u64>,
    failed: Vec<bool>,
    traces: Vec<String>,
    pre: String,
    post: String,
    verdict: String,
}

async fn run_case(label: &str, terms: &[SignedDeployData], mode: EffectMode, verbose: bool) -> Observation {
    let rm = common::build_runtime_manager_with_mode(mode).await;
    let rand = Blake2b512Random::from_init(&[0u8; 32]);
    let vault = Vault {
        rev_address: RevAddress::from_public_key(&PublicKey::new(terms[0].deployer.clone())).expect("REV address"),
        initial_balance: NonNegI64::try_from(1_000_000_000).unwrap(),
    };
    let (_, pre, _) = rm.compute_genesis(&[], &rand, BlockData::empty(), &PosGenesis::default(), &[vault]).await.expect("synthetic genesis");
    let (post, users, systems) = rm.compute_state(&pre, terms, &[], &rand, BlockData::empty(), &fringe_state(1)).await.expect("play must execute");
    let processed: Vec<ProcessedDeploy> = users.into_iter().map(|x| x.deploy).collect();
    let systems: Vec<ProcessedSystemDeploy> = systems.into_iter().map(|x| x.deploy).collect();
    let costs = processed.iter().map(|x| x.cost.cost).collect();
    let failed = processed.iter().map(|x| x.is_failed).collect();
    let traces = processed.iter().map(|x| format!("{:?}", x.deploy_log)).collect();
    for (i, d) in processed.iter().enumerate() {
        let mut produce = 0;
        let mut consume = 0;
        let mut comm = 0;
        for event in &d.deploy_log {
            match event { Event::Produce(_) => produce += 1, Event::Consume(_) => consume += 1, Event::Comm(_) => comm += 1 }
        }
        println!("OMEGA_M23_COST label={label} mode={mode:?} deploy={i} cost={} failed={} produce={produce} consume={consume} comm={comm}", d.cost.cost, d.is_failed);
        if verbose {
            for (at, event) in d.deploy_log.iter().enumerate() {
                println!("OMEGA_M23_TRACE label={label} deploy={i} at={at} event={event:?}");
            }
        }
    }
    let replay = rm.replay_compute_state(&pre, &processed, &systems, &rand, BlockData::empty(), &fringe_state(1), &BTreeMap::new(), true, &PosGenesis::default(), &[]).await;
    let verdict = match replay {
        Ok((state, _)) => {
            assert_eq!(state, post, "successful strict replay must match state");
            "ACCEPT state_match=true".to_string()
        },
        Err(e) => format!("REJECT {e:?}"),
    };
    println!("OMEGA_M23_RESULT label={label} mode={mode:?} scope=synthetic-prestate pre={pre:?} post={post:?} costs={costs:?} failed={failed:?} verdict={verdict}");
    Observation { costs, failed, traces, pre: format!("{pre:?}"), post: format!("{post:?}"), verdict }
}

#[tokio::test]
async fn omega_m23_recovered_terms_on_synthetic_prestate() {
    for (name, d) in fixtures() {
        let first = run_case(name, &[d.clone()], EffectMode::Sequential, true).await;
        for repeat in 2..=3 {
            let next = run_case(name, &[d.clone()], EffectMode::Sequential, false).await;
            assert_eq!(first, next, "deterministic repeat {repeat} for {name}");
        }
    }
}

#[tokio::test]
async fn omega_m24_registry_and_pending_probe_controls() {
    let fs = fixtures();
    let registry = &fs[0].1;
    let probe = &fs[4].1;
    // Vary only order of two authenticated recovered deploys. Neither order is
    // asserted to be the historical pre-state or original block deploy order.
    for (label, terms) in [
        ("registry-then-probe", vec![registry.clone(), probe.clone()]),
        ("pending-probe-then-registry", vec![probe.clone(), registry.clone()]),
        ("two-registries", vec![registry.clone(), fs[1].1.clone()]),
    ] {
        for mode in [EffectMode::Sequential, EffectMode::Gate, EffectMode::RelaxedValidated, EffectMode::ForkJoin] {
            run_case(label, &terms, mode, false).await;
        }
    }
}
