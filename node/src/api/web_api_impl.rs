//! Web API implementation (port of `WebApi.WebApiImpl`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use rchain_casper::api::block_api::BlockApi;
use rchain_crypto::hash::blake2b256_hash::Blake2b256Hash;
use rchain_crypto::private_key::PrivateKey;
use rchain_models::ast::Expr;
use rchain_models::casper::protocol::casper_message::SignedDeployData;
use rchain_models::casper::protocol::deploy_service::{
    BlockInfo, DeployExecStatus as CasperDeployExecStatus, LightBlockInfo,
};
use rchain_rholang::util::rev_address::RevAddress;
use rchain_shared::base16;

use super::conversion::{
    to_api_status, to_data_at_name_response, to_deploy_exec_status, to_exploratory_deploy_response,
    to_node_capabilities, to_pooled_deploy, to_rho_data_response, to_signed_deploy,
};
use super::dto::{
    ApiStatus, BlockApiException, DataAtNameByBlockHashRequest, DataAtNameRequest,
    DataAtNameResponse, DeployExecStatus, DeployRequest, ExploratoryDeployResponse, FaucetResponse,
    NodeCapabilities, PooledDeploys, RhoDataResponse,
};
use super::faucet;
use super::rho_expr::{rho_expr_to_par, unforg_to_par};
use super::web_api::WebApi;
use crate::web::transaction::{TransactionApi, TransactionResponse};

/// The web API implementation (port of `WebApi.WebApiImpl`).
pub struct WebApiImpl {
    block_api: Arc<dyn BlockApi>,
    transaction_api: Arc<dyn TransactionApi>,
    /// The dev deployer key (dev-mode only); `None` disables the faucet.
    deployer_key: Option<PrivateKey>,
    shard_id: String,
    /// Faucet accounting is one lock so concurrent requests cannot pass separate address/total checks.
    faucet_ledger: Arc<Mutex<FaucetLedger>>,
}

impl WebApiImpl {
    pub fn new(
        block_api: Arc<dyn BlockApi>,
        transaction_api: Arc<dyn TransactionApi>,
        deployer_key: Option<PrivateKey>,
        shard_id: String,
    ) -> Self {
        WebApiImpl {
            block_api,
            transaction_api,
            deployer_key,
            shard_id,
            faucet_ledger: Arc::new(Mutex::new(FaucetLedger::default())),
        }
    }

    /// Read the recipient's REV balance from the same finalised fringe used by the public
    /// explore-deploy endpoint. This is the faucet's durable per-account eligibility check: a
    /// process restart can forget local reservations, but it cannot forget REV already delivered.
    async fn finalized_rev_balance(&self, address: &str) -> Result<i64, BlockApiException> {
        let term = format!(
            r#"new return, vault(`rho:rchain:revVault`), ret in {{ vault!("getBalance", "{address}", *ret) | for (@b <- ret) {{ return!(b) }} }}"#
        );
        let (reply, _) = self
            .block_api
            .exploratory_deploy(&term, None, false)
            .await
            .map_err(BlockApiException)?;

        reply
            .data
            .iter()
            .flat_map(|par| par.exprs.iter())
            .find_map(|expr| match expr {
                Expr::GInt(balance) => Some(*balance),
                _ => None,
            })
            .ok_or_else(|| {
                BlockApiException(format!(
                    "faucet: finalized balance query for {address} returned no integer"
                ))
            })
    }

    /// Remove one exact reservation. A replay/submission failure refunds the total allocation; a
    /// delivered drip only drops the in-process marker because its budget was genuinely spent.
    fn clear_faucet_reservation(&self, address: &str, expected: Option<&[u8]>, refund: bool) {
        let mut ledger = self.faucet_ledger.lock().unwrap_or_else(|p| p.into_inner());
        let matches = match (ledger.pending.get(address), expected) {
            (Some(None), None) => true,
            (Some(Some(actual)), Some(expected)) => actual.as_slice() == expected,
            _ => false,
        };
        if matches {
            ledger.pending.remove(address);
            if refund {
                ledger.spent = ledger.spent.saturating_sub(faucet::FAUCET_AMOUNT);
            }
        }
    }
}

#[derive(Default)]
struct FaucetLedger {
    /// One in-process reservation per recipient. `None` means the submit is still in progress;
    /// `Some(sig)` means it was accepted and later requests must reconcile its chain outcome.
    pending: HashMap<String, Option<Vec<u8>>>,
    spent: i64,
}

fn invalid_deploy_id() -> BlockApiException {
    BlockApiException("Deploy id is not valid base16 format.".to_string())
}

/// Public-testnet faucet allocation: 10,000 REV. A drip is reserved before submission and refunded
/// when submission or replay proves that nothing was delivered.
const FAUCET_TOTAL_BUDGET: i64 = 10_000 * 100_000_000;

#[cfg(test)]
mod tests {
    use super::*;
    use rchain_casper::runtime_manager::{CapturedReply, ReplySource};
    use std::sync::Mutex as StdMutex;

    use rchain_block_storage::dag::dag_storage::DeployId;
    use rchain_casper::api::block_api::{ApiErr, Capabilities};
    use rchain_models::ast::{Expr, Par};
    use rchain_models::block_metadata::BlockMetadata;
    use rchain_models::casper::protocol::casper_message::DeployData;
    use rchain_models::casper::protocol::deploy_service::{
        BlockInfo, ContinuationsWithBlockInfo, DataWithBlockInfo,
        DeployExecStatus as DomainExecStatus, LightBlockInfo, Status, VersionInfo,
    };

    /// A block API that answers from its fields and records what it was asked — the `StubBlockApi`
    /// pattern `deploy_grpc_service_v1.rs` and `tonic.rs`'s tests use. The methods this file never
    /// calls are `unreachable!`: a stub that returns a plausible value there would hide a call the
    /// implementation is not supposed to make.
    struct StubBlockApi {
        caps: Capabilities,
        pooled: Vec<SignedDeployData>,
        deploy_status: Arc<StdMutex<ApiErr<DomainExecStatus>>>,
        finalized_balance: Arc<StdMutex<i64>>,
        finalized_block_number: Arc<StdMutex<i64>>,
        /// What `deploy` was asked to pool, shared by handle so a test can read it after the stub
        /// has been boxed behind `Arc<dyn BlockApi>`.
        deployed: Arc<StdMutex<Vec<SignedDeployData>>>,
        /// Optional deploy refusal injected by tests; shared so a test can clear it and retry.
        deploy_error: Arc<StdMutex<Option<String>>>,
    }

    impl Default for StubBlockApi {
        fn default() -> Self {
            StubBlockApi {
                caps: Capabilities {
                    autopropose: false,
                    propose_on_deploy: false,
                    manual_propose: true,
                    admin_http: false,
                    dev_mode: true,
                },
                pooled: Vec::new(),
                deploy_status: Arc::new(StdMutex::new(Ok(DomainExecStatus::NotProcessed {
                    status: "pending".to_string(),
                }))),
                finalized_balance: Arc::new(StdMutex::new(0)),
                finalized_block_number: Arc::new(StdMutex::new(0)),
                deployed: Arc::new(StdMutex::new(Vec::new())),
                deploy_error: Arc::new(StdMutex::new(None)),
            }
        }
    }

    fn block_status() -> Status {
        Status {
            version: VersionInfo {
                api: "1".to_string(),
                node: "2".to_string(),
            },
            address: "addr".to_string(),
            network_id: "net".to_string(),
            shard_id: "root".to_string(),
            peers: 3,
            nodes: 4,
            min_phlo_price: 5,
            latest_block_number: 42,
        }
    }

    fn deploy_with(sig: u8, timestamp: i64) -> SignedDeployData {
        SignedDeployData {
            data: DeployData {
                attachments: Vec::new(),
                term: format!("term-{sig}"),
                timestamp,
                phlo_price: 1,
                phlo_limit: 2,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
            },
            deployer: vec![sig; 65],
            sig: vec![sig],
            sig_algorithm: "secp256k1".to_string(),
        }
    }

    fn light_block_info(block_number: i64) -> LightBlockInfo {
        LightBlockInfo {
            version: 1,
            shard_id: "root".to_string(),
            block_hash: format!("block-{block_number}"),
            block_number,
            sender: "sender".to_string(),
            seq_num: block_number,
            pre_state_hash: "pre".to_string(),
            post_state_hash: "post".to_string(),
            justifications: Vec::new(),
            bonds: Vec::new(),
            sig_algorithm: "secp256k1".to_string(),
            sig: "sig".to_string(),
            block_size: "0".to_string(),
            deploy_count: 0,
            rejected_deploys: Vec::new(),
            timestamp: 0,
        }
    }

    /// The trait's method list, copied signature-for-signature: every method this file does not
    /// call is `unreachable!`, so a call the implementation is not supposed to make fails loudly
    /// instead of being answered by a plausible stub value.
    #[async_trait]
    impl BlockApi for StubBlockApi {
        async fn status(&self) -> Status {
            block_status()
        }

        async fn deploy(&self, deploy: &SignedDeployData) -> ApiErr<String> {
            if let Some(error) = self.deploy_error.lock().unwrap().clone() {
                return Err(error);
            }
            self.deployed.lock().unwrap().push(deploy.clone());
            Ok(base16::encode(&deploy.sig))
        }

        async fn deploy_status(&self, _: &DeployId) -> ApiErr<DomainExecStatus> {
            self.deploy_status.lock().unwrap().clone()
        }

        async fn pooled_deploys(&self) -> ApiErr<Vec<SignedDeployData>> {
            Ok(self.pooled.clone())
        }

        async fn capabilities(&self) -> Capabilities {
            self.caps.clone()
        }

        async fn create_block(&self, _: bool) -> ApiErr<String> {
            unreachable!("WebApiImpl does not create blocks")
        }
        async fn get_propose_result(&self) -> ApiErr<String> {
            unreachable!("WebApiImpl does not read propose results")
        }
        async fn get_listening_name_data_response(
            &self,
            _: i32,
            _: &Par,
        ) -> ApiErr<(Vec<DataWithBlockInfo>, i32)> {
            unreachable!("not exercised here")
        }
        async fn get_listening_name_continuation_response(
            &self,
            _: i32,
            _: &[Par],
        ) -> ApiErr<(Vec<ContinuationsWithBlockInfo>, i32)> {
            unreachable!("not exercised here")
        }
        async fn get_blocks_by_heights(&self, _: i64, _: i64) -> ApiErr<Vec<LightBlockInfo>> {
            unreachable!("not exercised here")
        }
        async fn visualize_dag(&self, _: i32, _: i32, _: bool) -> ApiErr<Vec<String>> {
            unreachable!("not exercised here")
        }
        async fn machine_verifiable_dag(&self, _: i32) -> ApiErr<String> {
            unreachable!("not exercised here")
        }
        async fn get_blocks(&self, _: i32) -> ApiErr<Vec<LightBlockInfo>> {
            unreachable!("not exercised here")
        }
        async fn find_deploy(&self, _: &DeployId) -> ApiErr<LightBlockInfo> {
            unreachable!("not exercised here")
        }
        async fn get_block(&self, _: &str) -> ApiErr<BlockInfo> {
            unreachable!("not exercised here")
        }
        async fn bond_status(&self, _: &[u8]) -> ApiErr<bool> {
            unreachable!("not exercised here")
        }
        async fn exploratory_deploy(
            &self,
            _: &str,
            _: Option<&str>,
            _: bool,
        ) -> ApiErr<(CapturedReply, LightBlockInfo)> {
            let balance = *self.finalized_balance.lock().unwrap();
            Ok((
                CapturedReply {
                    source: ReplySource::FirstPrivateName,
                    data: vec![Par {
                        exprs: vec![Expr::GInt(balance)],
                        ..Default::default()
                    }],
                },
                light_block_info(*self.finalized_block_number.lock().unwrap()),
            ))
        }
        async fn get_data_at_par(
            &self,
            _: &Par,
            _: &str,
            _: bool,
        ) -> ApiErr<(Vec<Par>, LightBlockInfo)> {
            unreachable!("not exercised here")
        }
        async fn last_finalized_block(&self) -> ApiErr<BlockInfo> {
            Ok(BlockInfo {
                block_info: light_block_info(*self.finalized_block_number.lock().unwrap()),
                deploys: Vec::new(),
            })
        }
        async fn is_finalized(&self, _: &str) -> ApiErr<bool> {
            unreachable!("not exercised here")
        }
        async fn get_latest_message(&self) -> ApiErr<BlockMetadata> {
            unreachable!("not exercised here")
        }
    }

    struct StubTransactionApi;
    #[async_trait]
    impl TransactionApi for StubTransactionApi {
        async fn get_transaction(
            &self,
            _: &Blake2b256Hash,
        ) -> Result<Vec<crate::web::transaction::TransactionInfo>, String> {
            unreachable!("WebApiImpl's get_transaction is not exercised here")
        }
    }

    /// A deploy-mode key pair and a REV address derived from it — the address the faucet will
    /// accept, since a REV address is checked for its checksum and prefix, not merely its length.
    fn key_and_address() -> (PrivateKey, String) {
        let alg = rchain_crypto::signatures::signatures_alg::from_algorithm("secp256k1")
            .expect("secp256k1 is registered");
        let (sk, pk) = alg.new_key_pair();
        let address = RevAddress::from_public_key(&pk).expect("an address from the key");
        (sk, address.to_base58())
    }

    /// A REV address that is **not** the deployer's own.
    ///
    /// `key_and_address` derives both halves from one key, so its address *is* the deployer's — and
    /// every faucet test in this module was dripping to it. That is AUDIT R33 exactly: the drip is a
    /// transfer from the node's account to the node's account, which moves nothing, and nothing
    /// refused it. With the refusal in place those tests would all fail for the wrong reason, so a
    /// test that wants to exercise a real drip names somebody else.
    fn faucet_target() -> String {
        key_and_address().1
    }

    fn api(block_api: StubBlockApi, deployer_key: Option<PrivateKey>) -> WebApiImpl {
        WebApiImpl::new(
            Arc::new(block_api),
            Arc::new(StubTransactionApi),
            deployer_key,
            "root".to_string(),
        )
    }

    /// `capabilities` reports the faucet as available only when **both** dev mode and a deployer key
    /// are configured: advertising it with no key would send a wallet at a 500.
    #[tokio::test]
    async fn the_faucet_is_advertised_only_with_dev_mode_and_a_key() {
        let (sk, _) = key_and_address();

        for (dev_mode, has_key, expected) in [
            (true, true, true),
            (true, false, false),
            (false, true, false),
            (false, false, false),
        ] {
            let mut block_api = StubBlockApi::default();
            block_api.caps.dev_mode = dev_mode;
            let key = if has_key { Some(sk.clone()) } else { None };
            let caps = api(block_api, key)
                .capabilities()
                .await
                .expect("capabilities");
            assert_eq!(
                caps.faucet, expected,
                "dev_mode = {dev_mode}, key = {has_key}"
            );
        }
    }

    /// A non-hex deploy id is refused **by name** before the block API is consulted — the id comes
    /// from a URL path, so it is untrusted input.
    #[tokio::test]
    async fn a_non_hex_deploy_id_is_refused_by_name() {
        let err = api(StubBlockApi::default(), None)
            .deploy_status("not-hex!")
            .await
            .expect_err("not base16");
        assert_eq!(
            err,
            BlockApiException("Deploy id is not valid base16 format.".to_string())
        );

        // Odd-length hex is not base16 either.
        assert!(api(StubBlockApi::default(), None)
            .deploy_status("abc")
            .await
            .is_err());
    }

    /// A hex id reaches the block API, and the returned status is converted — the stub's status
    /// comes back as the API's own `NotProcessed` variant with its message intact.
    #[tokio::test]
    async fn a_valid_deploy_id_returns_the_converted_status() {
        let status = api(StubBlockApi::default(), None)
            .deploy_status("aabb")
            .await
            .expect("a valid id");
        match status {
            DeployExecStatus::NotProcessed { status } => assert_eq!(status, "pending"),
            other => panic!("expected NotProcessed, got {other:?}"),
        }
    }

    /// Pooled deploys are returned **most-recent-first**, by the deploy's own timestamp: the pool's
    /// key order is the signature bytes, so without the sort the API's order would look random to a
    /// client (and change between calls).
    #[tokio::test]
    async fn pooled_deploys_come_back_most_recent_first() {
        let block_api = StubBlockApi {
            pooled: vec![
                deploy_with(1, 500),
                deploy_with(2, 900),
                deploy_with(3, 100),
            ],
            ..StubBlockApi::default()
        };
        let pooled = api(block_api, None).pooled_deploys().await.expect("pooled");

        let timestamps: Vec<i64> = pooled.deploys.iter().map(|d| d.timestamp).collect();
        assert_eq!(timestamps, vec![900, 500, 100]);
        assert_eq!(pooled.deploys[0].deploy_id, base16::encode(&[2u8]));
        assert_eq!(pooled.deploys[1].term, "term-1");
    }

    /// The faucet validates the REV address **before** spending anything: an invalid address is an
    /// error naming it (a valid REV address has a checksum and a coin prefix, so a plausible-looking
    /// string is not enough).
    #[tokio::test]
    async fn the_faucet_refuses_an_invalid_address_by_name() {
        let (sk, _) = key_and_address();
        let err = api(StubBlockApi::default(), Some(sk))
            .faucet("rBdXnotARealAddress")
            .await
            .expect_err("invalid");
        assert_eq!(
            err,
            BlockApiException("Invalid REV address: rBdXnotARealAddress".to_string())
        );
    }

    /// **A drip to the deployer's own address is refused** (AUDIT R33).
    ///
    /// `build_transfer_term` signs a transfer from the deployer's account to whatever address is
    /// asked for, and the genesis faucet's deployer **is** the funded account — so asking for the
    /// deployer's own address builds a transfer that moves nothing between two accounts the node
    /// already holds. It still spent one of that address's drips and submitted a deploy that pays
    /// phlo to do it, and nothing refused it. Every test in this module was dripping to exactly that
    /// address without noticing, which is what R33 means by a no-op that is not free.
    ///
    /// Two arms, so a refusal cannot pass for a fix on a faucet that refuses everything: the
    /// self-drip is refused **by name and reason**, and a drip to anybody else still succeeds.
    #[tokio::test]
    async fn the_faucet_refuses_a_drip_to_the_deployers_own_address() {
        let (sk, own) = key_and_address();
        let web = api(StubBlockApi::default(), Some(sk));

        let err = web
            .faucet(&own)
            .await
            .expect_err("the deployer's own address is not a target");
        assert!(
            err.0.contains(&own) && err.0.contains("deployer's own address"),
            "the refusal names the address and what is wrong with it: {}",
            err.0
        );

        let target = faucet_target();
        assert!(target != own, "the two keys differ");
        web.faucet(&target)
            .await
            .expect("somebody else is a target");
    }

    /// One delivered drip is enforced by finalized chain state rather than a process-local count.
    /// The first request reserves and submits; once that deploy is successful and finalised, the
    /// recipient balance itself makes a second request ineligible.
    #[tokio::test]
    async fn finalized_balance_makes_the_faucet_idempotent() {
        let (sk, _) = key_and_address();
        let address = faucet_target();
        let stub = StubBlockApi::default();
        let balance = stub.finalized_balance.clone();
        let finalized = stub.finalized_block_number.clone();
        let deploy_status = stub.deploy_status.clone();
        let deployed = stub.deployed.clone();
        let web = api(stub, Some(sk));

        let first = web.faucet(&address).await.expect("first drip submits");
        assert_eq!(first.amount, faucet::FAUCET_AMOUNT);

        *deploy_status.lock().unwrap() = Ok(DomainExecStatus::ProcessedWithSuccess {
            deploy_result: Vec::new(),
            block: light_block_info(7),
        });
        *finalized.lock().unwrap() = 7;
        *balance.lock().unwrap() = faucet::FAUCET_AMOUNT;

        let err = web
            .faucet(&address)
            .await
            .expect_err("finalized delivery makes the account ineligible");
        assert!(
            err.0.contains("already funded in finalized state"),
            "the refusal is derived from chain state: {}",
            err.0
        );
        assert_eq!(
            deployed.lock().unwrap().len(),
            1,
            "no second drip was submitted"
        );
    }

    /// A submitted drip remains reserved until its outcome is known, closing the concurrent/retry
    /// window before finality. The chain state becomes authoritative once delivery finalises.
    #[tokio::test]
    async fn a_pending_drip_keeps_the_address_reserved() {
        let (sk, _) = key_and_address();
        let address = faucet_target();
        let web = api(StubBlockApi::default(), Some(sk));

        web.faucet(&address).await.expect("first drip submits");
        let err = web
            .faucet(&address)
            .await
            .expect_err("the first deploy is still pending");
        assert!(
            err.0.contains("still pending"),
            "pending reservation is named: {}",
            err.0
        );
    }

    /// With no deployer key the faucet refuses before any chain read or reservation.
    #[tokio::test]
    async fn the_faucet_without_a_key_names_the_flags_it_needs() {
        let (_, address) = key_and_address();
        let err = api(StubBlockApi::default(), None)
            .faucet(&address)
            .await
            .expect_err("no key");
        assert_eq!(
            err,
            BlockApiException("faucet requires --dev-mode --deployer-private-key".to_string())
        );
    }

    /// A refused deploy consumes neither the address allowance nor the total faucet budget.
    #[tokio::test]
    async fn a_failed_submit_does_not_consume_faucet_budget() {
        let (sk, _) = key_and_address();
        let address = faucet_target();
        let stub = StubBlockApi::default();
        let deploy_error = stub.deploy_error.clone();
        *deploy_error.lock().unwrap() = Some("pool refused".to_string());
        let web = api(stub, Some(sk));

        let before = web.capabilities().await.expect("capabilities");
        assert_eq!(before.faucet_remaining, FAUCET_TOTAL_BUDGET);
        assert!(
            web.faucet(&address).await.is_err(),
            "the injected refusal reaches the caller"
        );
        let after_refusal = web.capabilities().await.expect("capabilities");
        assert_eq!(after_refusal.faucet_remaining, FAUCET_TOTAL_BUDGET);

        *deploy_error.lock().unwrap() = None;
        web.faucet(&address)
            .await
            .expect("the same address still owns its one successful drip");
        let after_success = web.capabilities().await.expect("capabilities");
        assert_eq!(
            after_success.faucet_remaining,
            FAUCET_TOTAL_BUDGET - faucet::FAUCET_AMOUNT
        );
    }

    /// A successful replay is not delivery until the containing block is finalized. While finality
    /// is behind, the recipient reservation stays closed; once finality reaches the block and the
    /// finalized balance is still below one drip, the account can retry.
    #[tokio::test]
    async fn processed_success_waits_for_finality_before_reconciling_delivery() {
        let (sk, _) = key_and_address();
        let address = faucet_target();
        let stub = StubBlockApi::default();
        let deploy_status = stub.deploy_status.clone();
        let finalized = stub.finalized_block_number.clone();
        let deployed = stub.deployed.clone();
        let web = api(stub, Some(sk));

        web.faucet(&address)
            .await
            .expect("first submit is accepted");
        *deploy_status.lock().unwrap() = Ok(DomainExecStatus::ProcessedWithSuccess {
            deploy_result: Vec::new(),
            block: light_block_info(7),
        });
        *finalized.lock().unwrap() = 6;

        let waiting = web
            .faucet(&address)
            .await
            .expect_err("processed is not finalized delivery");
        assert!(
            waiting.0.contains("processed in block 7 but finality is 6"),
            "the finality boundary is explicit: {}",
            waiting.0
        );
        assert_eq!(
            deployed.lock().unwrap().len(),
            1,
            "no retry before finality"
        );

        *finalized.lock().unwrap() = 7;
        web.faucet(&address)
            .await
            .expect("finalized success with no delivered balance releases the retry");
        assert_eq!(
            deployed.lock().unwrap().len(),
            2,
            "retry happens after reconciliation"
        );
    }

    /// A replay-time failure is different from a submission refusal: the deploy was accepted, but
    /// the chain later proved that nothing was delivered. The next request must refund that stale
    /// reservation and be allowed to submit again.
    #[tokio::test]
    async fn a_replay_failure_does_not_consume_recipient_eligibility() {
        let (sk, _) = key_and_address();
        let address = faucet_target();
        let stub = StubBlockApi::default();
        let deploy_status = stub.deploy_status.clone();
        let deployed = stub.deployed.clone();
        let web = api(stub, Some(sk));

        web.faucet(&address)
            .await
            .expect("first submit is accepted");
        assert_eq!(
            web.capabilities().await.unwrap().faucet_remaining,
            FAUCET_TOTAL_BUDGET - faucet::FAUCET_AMOUNT
        );

        *deploy_status.lock().unwrap() = Ok(DomainExecStatus::ProcessedWithError {
            deploy_error: "preCharge: insufficient funds (0 < 1000000)".to_string(),
            block: light_block_info(5),
        });

        let retry = web
            .faucet(&address)
            .await
            .expect("replay failure releases eligibility and retries");
        assert_eq!(retry.amount, faucet::FAUCET_AMOUNT);
        assert_eq!(
            deployed.lock().unwrap().len(),
            2,
            "the retry reached the block API"
        );
        assert_eq!(
            web.capabilities().await.unwrap().faucet_remaining,
            FAUCET_TOTAL_BUDGET - faucet::FAUCET_AMOUNT,
            "the failed replay was refunded before the retry reservation"
        );
    }

    /// The advertised capability carries the remaining total budget and closes when it is dry.
    #[tokio::test]
    async fn a_dry_faucet_is_not_advertised() {
        let (sk, _) = key_and_address();
        let web = api(StubBlockApi::default(), Some(sk));
        web.faucet_ledger.lock().unwrap().spent = FAUCET_TOTAL_BUDGET;

        let caps = web.capabilities().await.expect("capabilities");
        assert_eq!(caps.faucet_remaining, 0);
        assert!(!caps.faucet, "a client must not offer a dry faucet");
    }

    /// A successful drip signs a transfer and hands it to the block API. The deploy the API
    /// received carries the **public key derived from the deployer's secret** (the node's `deployer`
    /// field is what the vault's `transfer` derives `from` from), the configured shard, and a
    /// non-empty signature — so the receipt's deploy id is the signature's hex.
    #[tokio::test]
    async fn a_drip_signs_a_transfer_and_pools_it() {
        let (sk, _) = key_and_address();
        let address = faucet_target();
        let stub = StubBlockApi::default();
        let deployed = stub.deployed.clone();
        let web = api(stub, Some(sk.clone()));

        let response = web.faucet(&address).await.expect("a drip");
        assert_eq!(response.amount, faucet::FAUCET_AMOUNT);
        assert_eq!(response.to, address);

        let recorded = deployed.lock().unwrap();
        assert_eq!(recorded.len(), 1, "the deploy reached the block API");
        assert_eq!(recorded[0].sig_algorithm, "secp256k1");
        assert!(!recorded[0].sig.is_empty(), "it is signed");
        assert_eq!(recorded[0].data.shard_id, "root", "the node's shard");
        assert_eq!(
            recorded[0].data.valid_after_block_number, 42,
            "anchored to the chain height"
        );

        let alg = rchain_crypto::signatures::signatures_alg::from_algorithm("secp256k1")
            .expect("registered");
        let expected_deployer = alg.to_public(&sk).expect("public key").bytes().to_vec();
        assert_eq!(
            recorded[0].deployer, expected_deployer,
            "the deployer is the public key of the signing key"
        );
        assert_eq!(
            response.deploy_id,
            base16::encode(&recorded[0].sig),
            "the receipt's id is the deploy's signature"
        );

        // The transfer term names the recipient, so the drip actually pays the address asked for.
        assert!(
            recorded[0].data.term.contains(&address),
            "the term is a transfer to {address}: {}",
            recorded[0].data.term
        );
    }
}

#[async_trait]
impl WebApi for WebApiImpl {
    async fn status(&self) -> Result<ApiStatus, BlockApiException> {
        let status = self.block_api.status().await;
        let caps = self.block_api.capabilities().await;
        let health = self.block_api.proposer_health().await;
        Ok(to_api_status(&status, &caps, &health))
    }

    async fn deploy(&self, request: &DeployRequest) -> Result<String, BlockApiException> {
        // `Signed<DeployData>` holds a `&dyn SignaturesAlg` (not `Sync`), so keep it in a block
        // that ends before the `.await`.
        let deploy = {
            let signed = to_signed_deploy(request).map_err(|e| BlockApiException(e.0))?;
            SignedDeployData {
                data: signed.data.clone(),
                deployer: signed.pk.bytes().to_vec(),
                sig: signed.sig.clone(),
                sig_algorithm: signed.sig_algorithm.name().to_string(),
            }
        };
        self.block_api
            .deploy(&deploy)
            .await
            .map_err(BlockApiException)
    }

    async fn pooled_deploys(&self) -> Result<PooledDeploys, BlockApiException> {
        let mut pooled = self
            .block_api
            .pooled_deploys()
            .await
            .map_err(BlockApiException)?;
        // Most-recent-first: the pool's key order is the deploy signature bytes, not insertion time.
        pooled.sort_by_key(|d| std::cmp::Reverse(d.data.timestamp));
        let deploys = pooled.iter().map(to_pooled_deploy).collect();
        Ok(PooledDeploys { deploys })
    }

    async fn capabilities(&self) -> Result<NodeCapabilities, BlockApiException> {
        let caps = self.block_api.capabilities().await;
        let ledger = self.faucet_ledger.lock().unwrap_or_else(|p| p.into_inner());
        let remaining = FAUCET_TOTAL_BUDGET.saturating_sub(ledger.spent);
        // A dry faucet is not advertised: r-wallet can stop offering it without probing a write.
        let faucet =
            caps.dev_mode && self.deployer_key.is_some() && remaining >= faucet::FAUCET_AMOUNT;
        Ok(to_node_capabilities(&caps, faucet, remaining))
    }

    async fn deploy_status(&self, deploy_id: &str) -> Result<DeployExecStatus, BlockApiException> {
        let id = base16::decode(deploy_id).ok_or_else(invalid_deploy_id)?;
        let status = self
            .block_api
            .deploy_status(&id)
            .await
            .map_err(BlockApiException)?;
        to_deploy_exec_status(&status)
            .ok_or_else(|| BlockApiException("Deploy status protobuf message error".to_string()))
    }

    async fn faucet(&self, address: &str) -> Result<FaucetResponse, BlockApiException> {
        if !RevAddress::is_valid(address) {
            return Err(BlockApiException(format!("Invalid REV address: {address}")));
        }

        let sk = self.deployer_key.as_ref().ok_or_else(|| {
            BlockApiException("faucet requires --dev-mode --deployer-private-key".to_string())
        })?;
        let own = faucet::deployer_rev_address(sk).map_err(BlockApiException)?;
        if address == own {
            return Err(BlockApiException(format!(
                "faucet: {address} is the deployer's own address — the drip would move nothing between \
                 two accounts the node already holds, and would still spend the budget"
            )));
        }

        // Reconcile an earlier reservation before consulting durable eligibility. Submission is not
        // delivery: a replay-time failure refunds both the recipient reservation and the total budget.
        let pending = {
            let ledger = self.faucet_ledger.lock().unwrap_or_else(|p| p.into_inner());
            ledger.pending.get(address).cloned()
        };
        if let Some(pending) = pending {
            let deploy_id = match pending {
                None => {
                    return Err(BlockApiException(format!(
                        "faucet: address {address} already has a drip submission in progress"
                    )))
                }
                Some(deploy_id) => deploy_id,
            };

            match self
                .block_api
                .deploy_status(&deploy_id)
                .await
                .map_err(BlockApiException)?
            {
                CasperDeployExecStatus::ProcessedWithError { .. } => {
                    self.clear_faucet_reservation(address, Some(&deploy_id), true);
                }
                CasperDeployExecStatus::NotProcessed { status } => {
                    return Err(BlockApiException(format!(
                        "faucet: previous drip for {address} is still pending ({status})"
                    )))
                }
                CasperDeployExecStatus::ProcessedWithSuccess { block, .. } => {
                    let finalized = self
                        .block_api
                        .last_finalized_block()
                        .await
                        .map_err(BlockApiException)?;
                    if finalized.block_info.block_number < block.block_number {
                        return Err(BlockApiException(format!(
                            "faucet: previous drip for {address} is processed in block {} but finality is {}",
                            block.block_number, finalized.block_info.block_number
                        )));
                    }

                    let balance = self.finalized_rev_balance(address).await?;
                    if balance >= faucet::FAUCET_AMOUNT {
                        self.clear_faucet_reservation(address, Some(&deploy_id), false);
                        return Err(BlockApiException(format!(
                            "faucet: address {address} is already funded in finalized state ({balance} drops)"
                        )));
                    }

                    // Replay succeeded, finality passed, but the recipient still did not receive a
                    // drip. Delivery is the invariant, so release the reservation and let it retry.
                    self.clear_faucet_reservation(address, Some(&deploy_id), true);
                }
            }
        }

        // The chain, not a process-local counter, owns one-grant-per-account. explore-deploy without
        // a block hash is anchored to last-finalized-block, so this survives process restarts and
        // cannot lock out an account that received nothing.
        let balance = self.finalized_rev_balance(address).await?;
        if balance >= faucet::FAUCET_AMOUNT {
            return Err(BlockApiException(format!(
                "faucet: address {address} is already funded in finalized state ({balance} drops)"
            )));
        }

        // Build the deploy before taking the reservation; a signing error must not consume budget.
        let vabn = self.block_api.status().await.latest_block_number;
        let signed =
            faucet::sign_faucet_deploy(sk, address, faucet::FAUCET_AMOUNT, &self.shard_id, vabn)
                .map_err(BlockApiException)?;

        // Reserve atomically across the per-address in-flight gate and total allocation. This closes
        // the concurrent-request window while the chain outcome is still unknown.
        {
            let mut ledger = self.faucet_ledger.lock().unwrap_or_else(|p| p.into_inner());
            if ledger.pending.contains_key(address) {
                return Err(BlockApiException(format!(
                    "faucet: address {address} already has a drip reservation"
                )));
            }
            if FAUCET_TOTAL_BUDGET.saturating_sub(ledger.spent) < faucet::FAUCET_AMOUNT {
                return Err(BlockApiException(
                    "faucet: total budget exhausted".to_string(),
                ));
            }
            ledger.pending.insert(address.to_string(), None);
            ledger.spent = ledger.spent.saturating_add(faucet::FAUCET_AMOUNT);
        }

        if let Err(err) = self.block_api.deploy(&signed).await {
            self.clear_faucet_reservation(address, None, true);
            return Err(BlockApiException(err));
        }

        {
            let mut ledger = self.faucet_ledger.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(slot) = ledger.pending.get_mut(address) {
                *slot = Some(signed.sig.clone());
            }
        }

        Ok(FaucetResponse {
            deploy_id: base16::encode(&signed.sig),
            amount: faucet::FAUCET_AMOUNT,
            to: address.to_string(),
        })
    }

    async fn listen_for_data_at_name(
        &self,
        request: &DataAtNameRequest,
    ) -> Result<DataAtNameResponse, BlockApiException> {
        let par = unforg_to_par(&request.name).map_err(BlockApiException)?;
        let (dbs, length) = self
            .block_api
            .get_listening_name_data_response(request.depth, &par)
            .await
            .map_err(BlockApiException)?;
        Ok(to_data_at_name_response(&dbs, length))
    }

    async fn get_data_at_par(
        &self,
        request: &DataAtNameByBlockHashRequest,
    ) -> Result<RhoDataResponse, BlockApiException> {
        let par = rho_expr_to_par(&request.name).map_err(BlockApiException)?;
        let (pars, block) = self
            .block_api
            .get_data_at_par(&par, &request.block_hash, request.use_pre_state_hash)
            .await
            .map_err(BlockApiException)?;
        Ok(to_rho_data_response(&pars, &block))
    }

    async fn last_finalized_block(&self) -> Result<BlockInfo, BlockApiException> {
        self.block_api
            .last_finalized_block()
            .await
            .map_err(BlockApiException)
    }

    async fn get_block(&self, hash: &str) -> Result<BlockInfo, BlockApiException> {
        self.block_api
            .get_block(hash)
            .await
            .map_err(BlockApiException)
    }

    async fn get_blocks(&self, depth: i32) -> Result<Vec<LightBlockInfo>, BlockApiException> {
        self.block_api
            .get_blocks(depth)
            .await
            .map_err(BlockApiException)
    }

    async fn find_deploy(&self, deploy_id: &str) -> Result<LightBlockInfo, BlockApiException> {
        let id = base16::decode(deploy_id).ok_or_else(invalid_deploy_id)?;
        self.block_api
            .find_deploy(&id)
            .await
            .map_err(BlockApiException)
    }

    async fn exploratory_deploy(
        &self,
        term: &str,
        block_hash: Option<&str>,
        use_pre_state_hash: bool,
    ) -> Result<ExploratoryDeployResponse, BlockApiException> {
        let (reply, block) = self
            .block_api
            .exploratory_deploy(term, block_hash, use_pre_state_hash)
            .await
            .map_err(BlockApiException)?;
        Ok(to_exploratory_deploy_response(&reply, &block))
    }

    async fn get_blocks_by_heights(
        &self,
        start_block_number: i64,
        end_block_number: i64,
    ) -> Result<Vec<LightBlockInfo>, BlockApiException> {
        self.block_api
            .get_blocks_by_heights(start_block_number, end_block_number)
            .await
            .map_err(BlockApiException)
    }

    async fn is_finalized(&self, hash: &str) -> Result<bool, BlockApiException> {
        self.block_api
            .is_finalized(hash)
            .await
            .map_err(BlockApiException)
    }

    async fn get_transaction(&self, hash: &str) -> Result<TransactionResponse, BlockApiException> {
        if hash.is_empty() {
            return Err(BlockApiException("Block hash cannot be empty.".to_string()));
        }
        let blake =
            Blake2b256Hash::from_hex_either(hash).map_err(|e| BlockApiException(e.to_string()))?;
        let data = self
            .transaction_api
            .get_transaction(&blake)
            .await
            .map_err(BlockApiException)?;
        Ok(TransactionResponse { data })
    }
}
