//! Devnet faucet: sign a REV transfer from the funded deployer wallet to a caller's address.
//!
//! Dev/testnet-only. The transfer is an ordinary signed rholang deploy calling the **native**
//! `rho:rchain:revVault` system process (`rholang/src/system_processes.rs`), whose `transfer` derives
//! the source vault from the caller's unforgeable `deployerId`. It is only reachable when the node
//! runs with `--dev-mode --deployer-private-key`.

use std::time::{SystemTime, UNIX_EPOCH};

use rchain_crypto::private_key::PrivateKey;
use rchain_crypto::public_key::PublicKey;
use rchain_crypto::signatures::secp256k1::Secp256k1;
use rchain_crypto::signatures::signed::Signed;
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_rholang::util::rev_address::RevAddress;

/// One faucet drip, in the smallest REV unit ("drops"). `10 REV = 1_000_000_000` drops
/// (1 REV = 10^8 drops).
///
/// The drip is sized for **developing an application**, not for a single deploy: a deploy on the
/// testnet costs on the order of 0.001 REV, so one drip is thousands of them.
pub const FAUCET_AMOUNT: i64 = 1_000_000_000;

/// The faucet's whole budget, in drops: **10,000 REV**, i.e. 1,000 grants of [`FAUCET_AMOUNT`].
/// One of the genesis dev wallets holds exactly this, so the budget is dedicated rather than improvised.
pub const FAUCET_TOTAL_BUDGET: i64 = 1_000_000_000_000;

/// Phlo budget for a faucet transfer deploy (matches the devnet `deploy` helper).
const FAUCET_PHLO_LIMIT: i64 = 1_000_000;

/// Why a grant was refused. The caller of the endpoint gets the string; nothing is signed.
#[derive(Debug, PartialEq, Eq)]
pub enum FaucetRefusal {
    /// This address has already had its one drip. An address gets one, ever.
    AlreadyFunded,
    /// The budget is spent — the operator tops the wallet up and resets the ledger to continue.
    BudgetExhausted { spent: i64, budget: i64 },
    /// The ledger could not be written. The grant is *not* made: a drip that cannot be recorded is a
    /// drip that could be claimed again.
    LedgerUnwritable(String),
}

impl FaucetRefusal {
    pub fn message(&self) -> String {
        match self {
            FaucetRefusal::AlreadyFunded => {
                format!("this address has already been funded ({} REV once per account)", FAUCET_AMOUNT / 100_000_000)
            }
            FaucetRefusal::BudgetExhausted { spent, budget } => format!(
                "faucet budget exhausted ({} of {} REV granted)",
                spent / 100_000_000,
                budget / 100_000_000
            ),
            FaucetRefusal::LedgerUnwritable(e) => format!("faucet ledger unwritable: {e}"),
        }
    }
}

/// **The faucet's policy, and the only place it lives**: one drip per address, and a total budget.
///
/// Persisted as a plain text ledger — one `<address> <amount>` per line — so the promise survives a
/// restart. A faucet whose ledger is in memory only is a faucet anyone can reset by waiting for the
/// operator's next restart, which is not "once per account".
#[derive(Debug)]
pub struct FaucetLedger {
    path: std::path::PathBuf,
    granted: std::collections::BTreeSet<String>,
    spent: i64,
    budget: i64,
}

impl FaucetLedger {
    /// Open the ledger at `path`, reading whatever is already granted there.
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        Self::with_budget(path, FAUCET_TOTAL_BUDGET)
    }

    /// Open with an explicit budget (the budget is a parameter so the policy can be tested without
    /// granting 1,000 drips).
    pub fn with_budget(path: impl Into<std::path::PathBuf>, budget: i64) -> Result<Self, String> {
        let path = path.into();
        let mut granted = std::collections::BTreeSet::new();
        let mut spent: i64 = 0;
        if path.exists() {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            for line in text.lines() {
                let mut it = line.split_whitespace();
                if let (Some(addr), Some(amount)) = (it.next(), it.next()) {
                    if let Ok(amount) = amount.parse::<i64>() {
                        granted.insert(addr.to_string());
                        spent += amount;
                    }
                }
            }
        }
        Ok(Self { path, granted, spent, budget })
    }

    /// Grant one drip to `address`, or refuse. On success the ledger is appended before returning,
    /// so a crash cannot lose a grant the caller was told about.
    pub fn grant(&mut self, address: &str) -> Result<i64, FaucetRefusal> {
        if self.granted.contains(address) {
            return Err(FaucetRefusal::AlreadyFunded);
        }
        if self.spent + FAUCET_AMOUNT > self.budget {
            return Err(FaucetRefusal::BudgetExhausted { spent: self.spent, budget: self.budget });
        }
        let line = format!("{address} {FAUCET_AMOUNT}\n");
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .map_err(|e| FaucetRefusal::LedgerUnwritable(e.to_string()))?;
            f.write_all(line.as_bytes())
                .map_err(|e| FaucetRefusal::LedgerUnwritable(e.to_string()))?;
            f.sync_all()
                .map_err(|e| FaucetRefusal::LedgerUnwritable(e.to_string()))?;
        }
        self.granted.insert(address.to_string());
        self.spent += FAUCET_AMOUNT;
        Ok(FAUCET_AMOUNT)
    }

    /// Drops still available under the budget.
    pub fn remaining(&self) -> i64 {
        (self.budget - self.spent).max(0)
    }

    /// Drops granted so far.
    pub fn spent(&self) -> i64 {
        self.spent
    }

    /// Addresses that have had their drip.
    pub fn granted_count(&self) -> usize {
        self.granted.len()
    }
}


/// The native `revVault` transfer term: `transfer(*deployerId, to, amount, ret)` derives the `from`
/// vault from the caller's unforgeable `deployerId` and creates the `to` vault implicitly
/// (`rholang/src/system_processes.rs:1383-1422`). `__TO__` (REV address string) and `__AMOUNT__`
/// (integer drops) are substituted. Kept as a `.replace` template rather than a `format!` so the
/// rholang `{ … }` blocks don't collide with `format!` braces.
const TRANSFER_TEMPLATE: &str = r#"new revVault(`rho:rchain:revVault`), deployerId(`rho:rchain:deployerId`), resultCh in {
  revVault!("transfer", *deployerId, "__TO__", __AMOUNT__, *resultCh) |
  for (_ <- resultCh) { Nil }
}"#;

/// Render the native `revVault` transfer term that moves `amount` drops to `to`.
pub fn build_transfer_term(to: &str, amount: i64) -> String {
    TRANSFER_TEMPLATE
        .replace("__TO__", to)
        .replace("__AMOUNT__", &amount.to_string())
}

/// Derive the deployer's REV address from its private key (secp256k1 pubkey → REV address).
pub fn deployer_rev_address(sk: &PrivateKey) -> Result<String, String> {
    let pk_bytes = Secp256k1::to_public_bytes(sk.bytes()).map_err(|e| e.to_string())?;
    let pk = PublicKey::new(pk_bytes);
    RevAddress::from_public_key(&pk)
        .map(|a| a.to_base58())
        .ok_or_else(|| "failed to derive REV address from deployer key".to_string())
}

/// Build and sign a faucet transfer deploy to `to` (port of the devnet `deploy` path; the signing
/// pattern mirrors `casper/src/blocks/proposer/proposer.rs`'s dummy deploy).
///
/// `valid_after_block_number` must be the current chain height (not `-1`): a deploy is expired once
/// `latest_block_number - valid_after_block_number > DEPLOY_LIFESPAN` (50), so `-1` is dropped from
/// the pool as soon as the node passes block 49.
pub fn sign_faucet_deploy(
    sk: &PrivateKey,
    to: &str,
    amount: i64,
    shard_id: &str,
    valid_after_block_number: i64,
) -> Result<SignedDeployData, String> {
    let data = DeployData {
        attachments: Vec::new(),
        term: build_transfer_term(to, amount),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        phlo_price: 1,
        phlo_limit: FAUCET_PHLO_LIMIT,
        valid_after_block_number,
        shard_id: shard_id.to_string(),
    };
    // `Signed` holds a `&'static dyn SignaturesAlg` (not `Sync`); convert it into an owned
    // `SignedDeployData` in this block before any `.await`.
    let signed = Signed::new(data, &Secp256k1, sk).map_err(|e| e.to_string())?;
    Ok(SignedDeployData {
        data: signed.data,
        deployer: signed.pk.bytes().to_vec(),
        sig: signed.sig,
        sig_algorithm: signed.sig_algorithm.name().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_term_embeds_address_and_amount() {
        let term = build_transfer_term("toAddr", FAUCET_AMOUNT);
        assert!(term.contains("revVault!(\"transfer\""));
        assert!(term.contains("\"toAddr\""));
        assert!(term.contains(&FAUCET_AMOUNT.to_string()));
        assert!(!term.contains("__TO__") && !term.contains("__AMOUNT__"));
        // The native transfer derives `from` from the deployerId; there must be no string-address
        // `findOrCreate`/`deployerAuthKey` Scala-API remnants.
        assert!(!term.contains("findOrCreate") && !term.contains("deployerAuthKey"));
    }

    #[test]
    fn the_drip_is_ten_rev_and_the_budget_is_a_thousand_drips() {
        assert_eq!(FAUCET_AMOUNT, 10 * 100_000_000, "10 REV in drops");
        assert_eq!(FAUCET_TOTAL_BUDGET / FAUCET_AMOUNT, 1_000, "10,000 REV = 1,000 drips");
    }

    fn tmp_ledger(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("faucet-ledger-{name}-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn a_grant_is_once_per_address_and_survives_a_reopen() {
        let p = tmp_ledger("once");
        let mut l = FaucetLedger::open(&p).unwrap();
        assert_eq!(l.grant("addr-a").unwrap(), FAUCET_AMOUNT);
        assert_eq!(l.grant("addr-b").unwrap(), FAUCET_AMOUNT);
        assert_eq!(l.grant("addr-a"), Err(FaucetRefusal::AlreadyFunded));
        // and again after a restart, which is what makes it "once per account" rather than "once per process"
        let mut reopened = FaucetLedger::open(&p).unwrap();
        assert_eq!(reopened.grant("addr-a"), Err(FaucetRefusal::AlreadyFunded));
        assert_eq!(reopened.granted_count(), 2);
        assert_eq!(reopened.spent(), 2 * FAUCET_AMOUNT);
        assert_eq!(reopened.grant("addr-c").unwrap(), FAUCET_AMOUNT);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn the_budget_stops_the_drips_and_is_reported_as_remaining() {
        let p = tmp_ledger("budget");
        let mut l = FaucetLedger::with_budget(&p, 2 * FAUCET_AMOUNT).unwrap();
        assert_eq!(l.remaining(), 2 * FAUCET_AMOUNT);
        assert!(l.grant("a1").is_ok());
        assert_eq!(l.remaining(), FAUCET_AMOUNT);
        assert!(l.grant("a2").is_ok());
        assert_eq!(l.remaining(), 0);
        assert_eq!(
            l.grant("a3"),
            Err(FaucetRefusal::BudgetExhausted { spent: 2 * FAUCET_AMOUNT, budget: 2 * FAUCET_AMOUNT })
        );
        // the spent budget survives a restart, so topping up is a deliberate act
        let mut reopened = FaucetLedger::with_budget(&p, 2 * FAUCET_AMOUNT).unwrap();
        assert_eq!(reopened.remaining(), 0);
        assert!(reopened.grant("a4").is_err());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_refusal_names_the_reason_for_the_caller() {
        assert!(FaucetRefusal::AlreadyFunded.message().contains("already been funded"));
        assert!(FaucetRefusal::AlreadyFunded.message().contains("10 REV"));
        assert!(FaucetRefusal::BudgetExhausted { spent: 0, budget: FAUCET_TOTAL_BUDGET }
            .message()
            .contains("budget exhausted"));
    }

    #[test]
    fn deployer_rev_address_derives_from_key() {
        // The devnet deployer key; the derived address must match the genesis wallet address.
        let sk = PrivateKey::new(
            rchain_shared::base16::decode(
                "a68a6e6cca30f81bd24a719f3145d20e8424bd7b396309b0708a16c7d8000b76",
            )
            .unwrap(),
        );
        let addr = deployer_rev_address(&sk).unwrap();
        assert_eq!(
            addr,
            "11112VYAt8rUGNRRZX3eJdgagaAhtWTK8Js7F7X5iqddMVqyDTtYau"
        );
    }
}
