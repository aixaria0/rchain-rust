//! **The ERTP object API** — `casper/src/genesis/resources/ERTP.rho` over the native issuer ledger
//! (`rho:rchain:ertp:ledger`, W2.1). Issue #249, condition 3.
//!
//! The contract is installed here the way any deploy is: `compute_genesis` plays the `.rho` file as
//! an ordinary deploy signed by a fixed key, so the URI the file registers itself under is derivable
//! (`registry_insert_signed` derives it from the deployer's public key) and a second deploy can look
//! the contract up and call it. **Nothing in this file consults the blessing** — the alias tier
//! (`rho:rchain:ertp`) is W2.3's, and the fresh-chain gate that uses it is W2.4's. That is the
//! separation the two tests exist for: this one proves the contract, the other proves the blessing.
//!
//! Every assertion is read back out of the **block path**, not out of the contract: the driver
//! deploy reports each observation on its own `@"tag"` channel, and a missing tag is a failed
//! assertion with a name. That matters because an unmatched `for` in Rholang is silent — the failure
//! mode this port keeps meeting is a call that reaches nothing and looks like a client bug.
//!
//! The load-bearing clause is [`a_payment_of_one_brand_is_refused_by_another_brands_purse_and_survives_it`]:
//! a refused cross-brand deposit must leave the payment **live**, which is what distinguishes a
//! brand check from a burn.

mod common;

use rchain_casper::genesis::contracts::{ProofOfStake, Registry};
use rchain_casper::genesis::default_blessed_terms;
use rchain_casper::runtime_manager::RuntimeManager;
use rchain_crypto::hash::blake2b256;
use rchain_crypto::hash::blake2b512_random::Blake2b512Random;
use rchain_crypto::private_key::PrivateKey;
use rchain_crypto::signatures::secp256k1::Secp256k1;
use rchain_crypto::signatures::signatures_alg::SignaturesAlg;
use rchain_models::ast::{Expr, Par};
use rchain_models::casper::protocol::casper_message::{DeployData, SignedDeployData};
use rchain_models::par_ops::from_expr;
use rchain_models::rholang::RhoType::{RhoBoolean, RhoNumber, RhoString, RhoTupleN};
use rchain_models::sorted::SortedProc;
use rchain_rholang::native_state::PosGenesis;
use rchain_rholang::system_processes::BlockData;

use common::build_runtime_manager;

/// The contract, exactly as genesis will embed it.
const ERTP_RHO: &str = include_str!("../src/genesis/resources/ERTP.rho");

/// The key the contract is installed under. Its public half determines the `rho:id` the file
/// registers itself under, so a driver can name the contract without the blessing.
const ERTP_SEED: u8 = 21;

fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::from_init(&[11u8; 32])
}

/// A deploy signed by a **real** key pair, so `rho:rchain:deployerId` is a genuine public key and
/// the registry URI is the one `registry_insert_signed` derives from it.
fn deploy_signed_by(term: &str, seed: u8) -> SignedDeployData {
    let sk = PrivateKey::new(vec![seed; 32]);
    let pk = Secp256k1
        .to_public(&sk)
        .expect("a fixed 32-byte scalar is a valid secp256k1 key");
    SignedDeployData {
        data: DeployData {
            attachments: Vec::new(),
            term: term.to_string(),
            timestamp: 0,
            phlo_price: 1,
            phlo_limit: 5_000_000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
        },
        deployer: pk.bytes().to_vec(),
        sig: Vec::new(),
        sig_algorithm: "secp256k1".to_string(),
    }
}

/// The `rho:id` the ERTP deploy registers itself under — the same derivation
/// `registry_insert_signed` performs, so the two cannot drift without this test failing.
fn ertp_uri() -> String {
    let sk = PrivateKey::new(vec![ERTP_SEED; 32]);
    let pk = Secp256k1
        .to_public(&sk)
        .expect("a fixed 32-byte scalar is a valid secp256k1 key");
    rchain_rholang::registry::build_uri(&blake2b256::hash(pk.bytes()))
}

/// A driver: look `target` up, then run `body`. The lookup is the consumer's own idiom
/// (`rho:registry:lookup` → `for (@(_, C) <- ch) { @C!(…) }`), so a contract that registered nothing
/// fails here rather than silently.
///
/// `target` is either the contract's own `rho:id` — what W2.2 uses, so the gate does not depend on
/// the blessing — or the shorthand `rho:rchain:ertp`, which is what W2.4 uses. Both are URI
/// literals to the lookup, which is exactly why one helper serves both and why the two tests differ
/// only in what they are evidence *about*.
fn driver(target: &str, body: &str) -> SignedDeployData {
    let term = format!(
        r#"new rl(`rho:registry:lookup`), ch in {{
             rl!(`{target}`, *ch) |
             for (@(_, ERTP) <- ch) {{
               {body}
             }}
           }}"#
    );
    deploy_signed_by(&term, ERTP_SEED + 1)
}

/// Play a list of deploys through the genesis path and return the manager at the post-state, so the
/// tags can be read. A deploy that did not succeed is reported by index, because that is the one
/// failure that makes every assertion below vacuous at once.
async fn play_terms(terms: &[SignedDeployData]) -> RuntimeManager {
    let rm = build_runtime_manager().await;
    let (_, _, results) = rm
        .compute_genesis(
            terms,
            &fixed_rand(),
            BlockData::empty(),
            &PosGenesis::default(),
            &[],
        )
        .await
        .expect("compute_genesis");
    for (i, r) in results.iter().enumerate() {
        assert!(
            r.eval_result.succeeded(),
            "deploy #{i} failed: {:?}",
            r.eval_result.errors
        );
    }
    rm
}

/// **W2.2's vehicle**: the contract installed as an ordinary deploy, then each driver reaching it by
/// its own `rho:id`. Nothing here consults the blessing.
async fn play(bodies: &[&str]) -> RuntimeManager {
    let mut terms = vec![deploy_signed_by(ERTP_RHO, ERTP_SEED)];
    terms.extend(bodies.iter().map(|b| driver(&ertp_uri(), b)));
    play_terms(&terms).await
}

/// **W2.4's vehicle**: a real chain's blessed set, then each driver reaching the contract through
/// the shorthand a consumer hardcodes.
async fn play_blessed(bodies: &[&str]) -> RuntimeManager {
    let mut terms = default_blessed_terms(
        &proof_of_stake(),
        &Registry {
            system_contract_pub_key: String::new(),
        },
        &[],
        "root",
        &ceremony_identity(),
    )
    .expect("the blessed term list builds");
    terms.extend(bodies.iter().map(|b| driver("rho:rchain:ertp", b)));
    play_terms(&terms).await
}

/// The PoS parameters a test genesis needs (the same shape `genesis_registry.rs` uses).
fn proof_of_stake() -> ProofOfStake {
    ProofOfStake {
        minimum_bond: rchain_shared::refined::NonNegI64::try_from(1).unwrap(),
        maximum_bond: rchain_shared::refined::NonNegI64::try_from(100).unwrap(),
        validators: Vec::new(),
        epoch_length: 1,
        quarantine_length: 1,
        number_of_active_validators: 1,
        executor_share: rchain_shared::refined::NonNegI64::try_from(0).unwrap(),
        absence_slack: rchain_shared::refined::NonNegI64::try_from(0).unwrap(),
        participation_grace: rchain_shared::refined::NonNegI64::try_from(0).unwrap(),
        pos_multi_sig_public_keys: Vec::new(),
        pos_multi_sig_quorum: 1,
        pos_vault_pub_key: String::new(),
    }
}

/// The genesis ceremony's identity, fixed so a test genesis is deterministic.
fn ceremony_identity() -> rchain_casper::validator_identity::ValidatorIdentity {
    let sk = PrivateKey::new(vec![7u8; 32]);
    let public_key = Secp256k1
        .to_public(&sk)
        .expect("a fixed 32-byte scalar is a valid key");
    rchain_casper::validator_identity::ValidatorIdentity {
        public_key,
        private_key: sk,
        sig_algorithm: "secp256k1".to_string(),
    }
}

/// Everything sent to `@"tag"`. An absent tag is a failed assertion, not an error, so every caller
/// checks the length.
async fn read(rm: &RuntimeManager, tag: &str) -> Vec<Par> {
    rm.runtime()
        .get_data_par(&SortedProc::new(from_expr(Expr::GString(tag.to_string()))))
        .await
        .expect("read the driver's output channel")
}

/// The one value sent to `@"tag"`.
async fn one(rm: &RuntimeManager, tag: &str) -> Par {
    let v = read(rm, tag).await;
    assert_eq!(
        v.len(),
        1,
        "`{tag}` must have been sent exactly once: {v:?}"
    );
    v[0].clone()
}

/// The `(true, value)` / `(false, reason)` pair every ERTP op replies in.
async fn pair(rm: &RuntimeManager, tag: &str) -> (bool, Par) {
    let p = one(rm, tag).await;
    let parts = RhoTupleN::unapply(&p).unwrap_or_else(|| panic!("`{tag}` must be a pair: {p:?}"));
    assert_eq!(parts.len(), 2, "`{tag}` must be a pair: {p:?}");
    let ok = RhoBoolean::unapply(&parts[0])
        .unwrap_or_else(|| panic!("`{tag}`'s first element must be a boolean"));
    (ok, parts[1].clone())
}

/// A refusal and its reason — the assertion that an op *declined* rather than answered.
async fn refused(rm: &RuntimeManager, tag: &str) -> String {
    let (ok, reason) = pair(rm, tag).await;
    assert!(!ok, "`{tag}` must be refused, and it was answered instead");
    RhoString::unapply(&reason)
        .unwrap_or_else(|| panic!("`{tag}`'s reason must be a string: {reason:?}"))
        .to_string()
}

/// A reply that must be an answer, not a refusal.
async fn value(rm: &RuntimeManager, tag: &str) -> Par {
    let (ok, v) = pair(rm, tag).await;
    assert!(ok, "`{tag}` must be answered, and it was refused: {v:?}");
    v
}

/// The Int an answered op replied — `(true, n)`, which is the shape every op but `makeIssuerKit`
/// uses.
async fn answered_number(rm: &RuntimeManager, tag: &str) -> i64 {
    let v = value(rm, tag).await;
    RhoNumber::unapply(&v).unwrap_or_else(|| panic!("`{tag}` must answer an Int: {v:?}"))
}

/// The **amount** an answered op replied — `(true, (brand, n))`, the shape `amountMath`'s
/// constructors and arithmetic use.
async fn answered_amount(rm: &RuntimeManager, tag: &str) -> (Par, i64) {
    let v = value(rm, tag).await;
    let parts =
        RhoTupleN::unapply(&v).unwrap_or_else(|| panic!("`{tag}` must answer an amount: {v:?}"));
    assert_eq!(parts.len(), 2, "`{tag}` must answer an amount: {v:?}");
    let n = RhoNumber::unapply(&parts[1])
        .unwrap_or_else(|| panic!("`{tag}`'s amount must hold an Int: {v:?}"));
    (parts[0].clone(), n)
}

async fn boolean(rm: &RuntimeManager, tag: &str) -> bool {
    let v = one(rm, tag).await;
    RhoBoolean::unapply(&v).unwrap_or_else(|| panic!("`{tag}` must be a boolean: {v:?}"))
}

/// The boolean an answered op replied — `(true, true)`, the shape `isEqual` and `isEmpty` use.
async fn answered_boolean(rm: &RuntimeManager, tag: &str) -> bool {
    let v = value(rm, tag).await;
    RhoBoolean::unapply(&v).unwrap_or_else(|| panic!("`{tag}` must answer a boolean: {v:?}"))
}

/// A capability on the wire is `bundle+{*name}` — the shape every ERTP handle takes (the kit, a
/// purse, a payment, the facet). Asserting *that* rather than "not a number" is the difference
/// between checking the reply is a handle and checking the reply is not the wrong thing.
fn assert_capability(p: &Par, what: &str) {
    assert_eq!(
        p.bundles.len(),
        1,
        "{what} must be a bundle over a name: {p:?}"
    );
    assert_eq!(
        p.bundles[0].body.unforgeables.len(),
        1,
        "{what} must be a bundle over an unforgeable name: {p:?}"
    );
}

/// The three parts of a reported kit — `(brand, mint, issuer)`, Agoric's shape.
async fn kit(rm: &RuntimeManager, tag: &str) -> (Par, Par, Par) {
    let v = one(rm, tag).await;
    let parts =
        RhoTupleN::unapply(&v).unwrap_or_else(|| panic!("`{tag}` must be a 3-tuple: {v:?}"));
    assert_eq!(parts.len(), 3, "`{tag}` must be a 3-tuple: {v:?}");
    (parts[0].clone(), parts[1].clone(), parts[2].clone())
}

/// Run an async body on a worker thread with the node's 32 MiB stack: the blessed contracts recurse
/// deeper than the default 2 MiB test stack allows when they parse and normalize.
fn with_big_stack<F>(body: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    const STACK: usize = 32 * 1024 * 1024;
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .thread_stack_size(STACK)
                .enable_all()
                .build()
                .expect("a tokio runtime")
                .block_on(body)
        })
        .expect("spawn the test thread")
        .join()
        .expect("the test thread panicked");
}

/// **A kit is `(brand, mint, issuer)`, the issuer knows its brand, and a fresh purse is empty.**
///
/// The three are pairwise distinct: a kit whose brand *is* its mint would make "who may mint this?"
/// answerable by anyone holding an amount, which is the whole distinction ERTP exists to draw.
#[test]
fn a_kit_makes_a_brand_a_mint_and_an_issuer() {
    with_big_stack(async {
        let rm = play(&[r#"
new kitCh in {
  @ERTP!("makeIssuerKit", *kitCh) |
  for (@(brand, mint, issuer) <- kitCh) {
    @"kit"!((brand, mint, issuer)) |
    new bc, pc in {
      // One boolean, so a collision names itself rather than three separate reads.
      @"kit-collides"!((brand == mint) || (brand == issuer) || (mint == issuer)) |
      @issuer!("getBrand", *bc) |
      for (@r <- bc) { @"issuer-brand"!(r) } |
      @issuer!("makeEmptyPurse", *pc) |
      for (@r <- pc) {
        match r {
          (true, purse) => {
            new cc in {
              @purse!("getCurrentAmount", *cc) |
              for (@b <- cc) { @"empty-purse"!(b) }
            }
          }
          _ => { @"empty-purse"!(r) }
        }
      }
    }
  }
}
"#])
        .await;

        let (brand, _, _) = kit(&rm, "kit").await;
        assert!(
            !boolean(&rm, "kit-collides").await,
            "the brand, the mint and the issuer must be three different names"
        );
        assert_eq!(
            value(&rm, "issuer-brand").await,
            brand,
            "the issuer's `getBrand` must be the kit's brand, by structural equality"
        );
        assert_eq!(
            answered_number(&rm, "empty-purse").await,
            0,
            "a purse from `makeEmptyPurse` must read zero"
        );
    });
}

/// **A minted payment carries its brand and its amount, a deposit credits the purse, and the second
/// deposit of the same payment is refused with the balance unmoved.** That last clause is ERTP's
/// double-spend guard: a payment that could be deposited twice is a purse.
#[test]
fn a_payment_is_spent_by_its_first_deposit() {
    with_big_stack(async {
        let rm = play(&[r#"
new kitCh in {
  @ERTP!("makeIssuerKit", *kitCh) |
  for (@(brand, mint, issuer) <- kitCh) {
    @"kit"!((brand, mint, issuer)) |
    new amCh in {
      @issuer!("getAmountMath", *amCh) |
      for (@(true, am) <- amCh) {
        new amtCh in {
          @am!("make", brand, 10, *amtCh) |
          for (@(true, amount) <- amtCh) {
            new payCh in {
              @mint!("mintPayment", amount, *payCh) |
              for (@(true, payment) <- payCh) {
                new abCh in {
                  @payment!("getAllegedBrand", *abCh) |
                  for (@r <- abCh) { @"payment-brand"!(r) }
                } |
                new aoCh in {
                  @issuer!("getAmountOf", payment, *aoCh) |
                  for (@r <- aoCh) { @"payment-amount"!(r) }
                } |
                new pc in {
                  @issuer!("makeEmptyPurse", *pc) |
                  for (@(true, purse) <- pc) {
                    new d1 in {
                      @purse!("deposit", payment, *d1) |
                      for (@r1 <- d1) {
                        @"first-deposit"!(r1) |
                        new d2 in {
                          @purse!("deposit", payment, *d2) |
                          for (@r2 <- d2) {
                            @"second-deposit"!(r2) |
                            new cc in {
                              @purse!("getCurrentAmount", *cc) |
                              for (@b <- cc) { @"balance-after"!(b) }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#])
        .await;

        let (brand, _, _) = kit(&rm, "kit").await;
        assert_eq!(
            value(&rm, "payment-brand").await,
            brand,
            "a minted payment must report the brand it was minted under"
        );
        assert_eq!(answered_number(&rm, "payment-amount").await, 10);
        assert_eq!(answered_number(&rm, "first-deposit").await, 10);
        assert_eq!(
            refused(&rm, "second-deposit").await,
            "that payment has already been deposited",
            "a payment is consumed by its first deposit"
        );
        assert_eq!(
            answered_number(&rm, "balance-after").await,
            10,
            "the refused second deposit must not have moved the balance"
        );
    });
}

/// **The acceptance clause: a payment of one brand is refused by another brand's purse — and
/// survives it.** The refusal must leave the payment *live*, so the deposit into its own brand's
/// purse afterwards succeeds. A purse that burned a foreign payment would pass a weaker test.
///
/// The order is the point: B's refusal finishes, **then** A's deposit runs.
#[test]
fn a_payment_of_one_brand_is_refused_by_another_brands_purse_and_survives_it() {
    with_big_stack(async {
        let rm = play(&[r#"
new kitA, kitB in {
  @ERTP!("makeIssuerKit", *kitA) |
  @ERTP!("makeIssuerKit", *kitB) |
  for (@(brandA, mintA, issuerA) <- kitA & @(brandB, mintB, issuerB) <- kitB) {
    @"kitA"!((brandA, mintA, issuerA)) |
    @"kitB"!((brandB, mintB, issuerB)) |
    new amCh in {
      @issuerA!("getAmountMath", *amCh) |
      for (@(true, am) <- amCh) {
        new amtCh in {
          @am!("make", brandA, 10, *amtCh) |
          for (@(true, amount) <- amtCh) {
            new payCh in {
              @mintA!("mintPayment", amount, *payCh) |
              for (@(true, paymentA) <- payCh) {
                new pbc in {
                  @issuerB!("makeEmptyPurse", *pbc) |
                  for (@(true, purseB) <- pbc) {
                    new d1 in {
                      // B's purse, A's payment, and B's brand stated by B.
                      @purseB!("deposit", paymentA, *d1) |
                      for (@r1 <- d1) {
                        @"b-deposit-a"!(r1) |
                        new cb in {
                          @purseB!("getCurrentAmount", *cb) |
                          for (@b <- cb) { @"b-balance"!(b) }
                        } |
                        // Only now: the same payment, its own brand's purse.
                        new pac in {
                          @issuerA!("makeEmptyPurse", *pac) |
                          for (@(true, purseA) <- pac) {
                            new d2 in {
                              @purseA!("deposit", paymentA, *d2) |
                              for (@r2 <- d2) { @"a-deposit-a"!(r2) }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#])
        .await;

        let (brand_a, _, _) = kit(&rm, "kitA").await;
        let (brand_b, _, _) = kit(&rm, "kitB").await;
        assert_ne!(brand_a, brand_b, "two kits must not share a brand");

        assert_eq!(
            refused(&rm, "b-deposit-a").await,
            "no such payment",
            "another brand's purse must refuse the payment structurally: it names a token the \
             ledger does not know under this brand"
        );
        assert_eq!(
            answered_number(&rm, "b-balance").await,
            0,
            "the refused deposit must not have credited B's purse"
        );
        assert_eq!(
            answered_number(&rm, "a-deposit-a").await,
            10,
            "**the refused payment must stay live**: A's own purse must still be able to take it"
        );
    });
}

/// **A deposit facet can add and cannot withdraw.** `getDepositFacet` hands back a bundle of a
/// forwarder with one arm, so a facet holder never sees the purse's ledger token. The test sends
/// `withdraw` to the facet and asserts nothing ever answers it — which is the only observable form
/// of "the arm is not there", since an unmatched `for` in Rholang is silent.
#[test]
fn the_deposit_facet_can_add_and_cannot_withdraw() {
    with_big_stack(async {
        let rm = play(&[r#"
new kitCh in {
  @ERTP!("makeIssuerKit", *kitCh) |
  for (@(brand, mint, issuer) <- kitCh) {
    new pc in {
      @issuer!("makeEmptyPurse", *pc) |
      for (@(true, purse) <- pc) {
        new fc, wr in {
          @purse!("getDepositFacet", *fc) |
          for (@(true, facet) <- fc) {
            // Sent, but never answered: a reply here would be the defect.
            @facet!("withdraw", (brand, 1), *wr) |
            @"facet-probe-sent"!(true) |
            for (@_ <- wr) { @"facet-answered-withdraw"!(true) } |
            // …and the facet's one arm does work.
            new amCh in {
              @issuer!("getAmountMath", *amCh) |
              for (@(true, am) <- amCh) {
                new amtCh in {
                  @am!("make", brand, 10, *amtCh) |
                  for (@(true, amount) <- amtCh) {
                    new payCh in {
                      @mint!("mintPayment", amount, *payCh) |
                      for (@(true, payment) <- payCh) {
                        new dr in {
                          @facet!("deposit", payment, *dr) |
                          for (@r <- dr) {
                            @"facet-deposit"!(r) |
                            new cc in {
                              @purse!("getCurrentAmount", *cc) |
                              for (@b <- cc) { @"purse-after-facet-deposit"!(b) }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#])
        .await;

        assert!(
            boolean(&rm, "facet-probe-sent").await,
            "the probe must have run, or the absence below proves nothing"
        );
        assert!(
            read(&rm, "facet-answered-withdraw").await.is_empty(),
            "a deposit facet must not answer `withdraw`"
        );
        assert_eq!(
            answered_number(&rm, "facet-deposit").await,
            10,
            "the facet's `deposit` must be the purse's `deposit`"
        );
        assert_eq!(
            answered_number(&rm, "purse-after-facet-deposit").await,
            10,
            "the facet must have credited the purse it came from"
        );
    });
}

/// **`amountMath`: the pair, the arithmetic, and every refusal that keeps an amount depositable.**
///
/// A value the ledger cannot hold must never come out of `amountMath`, because an amount that
/// exists and cannot be deposited is the silent kind of broken. `make(brand, -1)`, `make(brand,
/// BigInt(2))`, another brand's amount, a subtraction that would go negative and an addition that
/// leaves `i64` are all refusals — never truncations, never wraps.
#[test]
fn amount_math_refuses_what_the_ledger_could_not_hold() {
    with_big_stack(async {
        let rm = play(&[r#"
new kitA, kitB in {
  @ERTP!("makeIssuerKit", *kitA) |
  @ERTP!("makeIssuerKit", *kitB) |
  for (@(brandA, mintA, issuerA) <- kitA & @(brandB, mintB, issuerB) <- kitB) {
    @"kitA"!((brandA, mintA, issuerA)) |
    @"kitB"!((brandB, mintB, issuerB)) |
    new amCh in {
      @issuerA!("getAmountMath", *amCh) |
      for (@(true, am) <- amCh) {
        // --- make
        new r in { @am!("make", brandA, 5, *r) | for (@x <- r) { @"make-ok"!(x) } } |
        new r in { @am!("make", brandA, -1, *r) | for (@x <- r) { @"make-negative"!(x) } } |
        new r in { @am!("make", brandA, BigInt(2), *r) | for (@x <- r) { @"make-bigint"!(x) } } |
        new r in { @am!("make", brandB, 5, *r) | for (@x <- r) { @"make-foreign"!(x) } } |
        // --- getValue, and the pair it insists on
        new r in { @am!("getValue", (brandA, 5), *r) | for (@x <- r) { @"get-value"!(x) } } |
        new r in { @am!("getValue", (brandB, 5), *r) | for (@x <- r) { @"get-value-foreign"!(x) } } |
        new r in { @am!("getValue", 5, *r) | for (@x <- r) { @"get-value-bare"!(x) } } |
        // --- add and subtract, on amounts made here
        new ma in {
          @am!("make", brandA, 5, *ma) |
          for (@(true, five) <- ma) {
            new mb in {
              @am!("make", brandA, 7, *mb) |
              for (@(true, seven) <- mb) {
                new r in { @am!("add", five, seven, *r) | for (@x <- r) { @"add-ok"!(x) } } |
                new r in { @am!("subtract", seven, five, *r) | for (@x <- r) { @"subtract-ok"!(x) } } |
                new r in { @am!("subtract", five, seven, *r) | for (@x <- r) { @"subtract-negative"!(x) } } |
                new r in { @am!("add", five, (brandB, 5), *r) | for (@x <- r) { @"add-foreign"!(x) } } |
                new r in { @am!("isEqual", five, (brandA, 5), *r) | for (@x <- r) { @"is-equal"!(x) } } |
                new r in { @am!("isEmpty", (brandA, 0), *r) | for (@x <- r) { @"is-empty"!(x) } } |
                new r in { @am!("isEmpty", five, *r) | for (@x <- r) { @"is-not-empty"!(x) } }
              }
            }
          }
        } |
        // --- the i64 bound
        new mm in {
          @am!("make", brandA, 9223372036854775807, *mm) |
          for (@(true, max) <- mm) {
            new mo in {
              @am!("make", brandA, 1, *mo) |
              for (@(true, one) <- mo) {
                new r in { @am!("add", max, one, *r) | for (@x <- r) { @"add-overflow"!(x) } } |
                new r in { @am!("subtract", max, max, *r) | for (@x <- r) { @"subtract-to-zero"!(x) } }
              }
            }
          }
        } |
        // --- the brand the math is bound to
        new r in { @am!("getBrand", *r) | for (@x <- r) { @"amount-math-brand"!(x) } }
      }
    }
  }
}
"#])
        .await;

        let (brand_a, _, _) = kit(&rm, "kitA").await;
        let (brand_b, _, _) = kit(&rm, "kitB").await;

        // The pair itself, and the two values `make` refuses.
        let made = value(&rm, "make-ok").await;
        let parts = RhoTupleN::unapply(&made).expect("an amount is a pair");
        assert_eq!(
            parts[0].clone(),
            brand_a,
            "an amount must carry the brand it was made of"
        );
        assert_eq!(RhoNumber::unapply(&parts[1]), Some(5));
        assert_eq!(
            refused(&rm, "make-negative").await,
            "an amount is not negative"
        );
        assert_eq!(
            refused(&rm, "make-bigint").await,
            "an amount's value is an Int",
            "a BigInt is not an `Int` the ledger could hold — refused, never truncated"
        );
        assert_eq!(
            refused(&rm, "make-foreign").await,
            "that amount is of another brand"
        );

        // Reading, and the shapes it refuses.
        assert_eq!(answered_number(&rm, "get-value").await, 5);
        assert_eq!(
            refused(&rm, "get-value-foreign").await,
            "that amount is of another brand"
        );
        assert_eq!(
            refused(&rm, "get-value-bare").await,
            "an amount is a (brand, value) pair"
        );

        // Arithmetic. `add` and `subtract` answer *amounts*, so the brand must be carried through
        // as well as the number.
        let (sum_brand, sum) = answered_amount(&rm, "add-ok").await;
        assert_eq!(sum, 12);
        assert_eq!(sum_brand, brand_a);
        let (difference_brand, difference) = answered_amount(&rm, "subtract-ok").await;
        assert_eq!(difference, 2);
        assert_eq!(difference_brand, brand_a);
        assert_eq!(
            refused(&rm, "subtract-negative").await,
            "the subtraction would be negative"
        );
        assert_eq!(
            refused(&rm, "add-foreign").await,
            "that amount is of another brand",
            "two brands cannot be added"
        );
        assert!(answered_boolean(&rm, "is-equal").await);
        assert!(answered_boolean(&rm, "is-empty").await);
        assert!(!answered_boolean(&rm, "is-not-empty").await);

        // The bound, at the boundary and one past it.
        assert_eq!(
            answered_amount(&rm, "subtract-to-zero").await.1,
            0,
            "`i64::MAX - i64::MAX` must be an amount"
        );
        assert_eq!(
            refused(&rm, "add-overflow").await,
            "the sum is larger than an amount can hold",
            "`i64::MAX + 1` promotes to a BigInt, which no purse could hold"
        );

        // And the math is bound to one brand.
        assert_eq!(value(&rm, "amount-math-brand").await, brand_a);
        assert_ne!(brand_a, brand_b);
    });
}

/// **A purse gives a payment back out of itself, and refuses to give more than it holds.**
///
/// `withdraw` is the arm the issuer's authority reaches through, so it is the one that must not be
/// tricked into a negative balance: the amount is validated by the *purse* before the ledger sees
/// it, because the ledger's own amount parser refuses a negative by failing the deploy rather than
/// answering — and a caller deserves a refusal it can branch on.
#[test]
fn a_purse_withdraws_a_payment_and_refuses_more_than_it_holds() {
    with_big_stack(async {
        let rm = play(&[r#"
new kitCh in {
  @ERTP!("makeIssuerKit", *kitCh) |
  for (@(brand, mint, issuer) <- kitCh) {
    @"kit"!((brand, mint, issuer)) |
    new amCh in {
      @issuer!("getAmountMath", *amCh) |
      for (@(true, am) <- amCh) {
        new pc in {
          @issuer!("makeEmptyPurse", *pc) |
          for (@(true, purse) <- pc) {
            // Fund the purse with a minted 10.
            new mc in {
              @am!("make", brand, 10, *mc) |
              for (@(true, ten) <- mc) {
                new mp in {
                  @mint!("mintPayment", ten, *mp) |
                  for (@(true, minted) <- mp) {
                    new fd in {
                      @purse!("deposit", minted, *fd) |
                      for (@_ <- fd) {
                        // Withdraw 4 of it.
                        new wc in {
                          @am!("make", brand, 4, *wc) |
                          for (@(true, four) <- wc) {
                            new wp in {
                              @purse!("withdraw", four, *wp) |
                              for (@rw <- wp) {
                                @"withdrawn"!(rw) |
                                match rw {
                                  (true, payment) => {
                                    new abCh in {
                                      @payment!("getAllegedBrand", *abCh) |
                                      for (@r <- abCh) { @"withdrawn-brand"!(r) }
                                    } |
                                    new aoCh in {
                                      @issuer!("getAmountOf", payment, *aoCh) |
                                      for (@r <- aoCh) { @"withdrawn-amount"!(r) }
                                    } |
                                    // Read the balance here — before the payment goes back in, so
                                    // "the purse was debited" is observed at the moment it was.
                                    new bc in {
                                      @purse!("getCurrentAmount", *bc) |
                                      for (@b <- bc) {
                                        @"balance-after-withdraw"!(b) |
                                        // **And the payment is live**: a withdrawn payment is an
                                        // ordinary payment, so the purse takes it back.
                                        new rd in {
                                          @purse!("deposit", payment, *rd) |
                                          for (@r <- rd) { @"redeposited"!(r) }
                                        }
                                      }
                                    }
                                  }
                                  _ => { Nil }
                                }
                              }
                            }
                          }
                        } |
                        // More than the purse holds, and each way of asking for it.
                        new oc in {
                          @am!("make", brand, 11, *oc) |
                          for (@(true, eleven) <- oc) {
                            new op in {
                              @purse!("withdraw", eleven, *op) |
                              for (@r <- op) { @"over-withdraw"!(r) }
                            }
                          }
                        } |
                        new np in {
                          @am!("make", brand, -1, *np) |
                          for (@r <- np) { @"negative-amount"!(r) }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#])
        .await;

        let (brand, _, _) = kit(&rm, "kit").await;
        // The withdrawal answered a **payment** — a capability, not a number — and the payment is
        // what the brand and amount reads below go through.
        assert_capability(&value(&rm, "withdrawn").await, "withdraw's payment");
        assert_eq!(value(&rm, "withdrawn-brand").await, brand);
        assert_eq!(answered_number(&rm, "withdrawn-amount").await, 4);
        assert_eq!(
            answered_number(&rm, "balance-after-withdraw").await,
            6,
            "the purse must have been debited by the withdrawal"
        );
        assert_eq!(
            answered_number(&rm, "redeposited").await,
            10,
            "**a withdrawn payment is an ordinary payment**: the purse must take it back, which is \
             what makes `withdraw` a transfer rather than a sink"
        );
        assert_eq!(
            refused(&rm, "over-withdraw").await,
            "insufficient funds",
            "the ledger refuses more than the purse holds, and the purse reports it"
        );
        assert_eq!(
            refused(&rm, "negative-amount").await,
            "an amount is not negative",
            "a negative amount must be refused by `amountMath`, never passed to the ledger, whose \
             own parser fails the deploy instead of answering"
        );
    });
}

/// **W2.4: the same contract, reached the way a consumer reaches it.** On a chain that ran the
/// genesis ceremony, `lookup!(\`rho:rchain:ertp\`, *ch)` must resolve to the ERTP contract and it
/// must behave — so this test reruns the load-bearing clause through the *shorthand*.
///
/// **Why one script and not all six.** The blessing copies one registry entry onto a name; it cannot
/// change the value. That the copied value *is* this contract's is pinned mechanically
/// (`standard_deploys::tests::aliased_contract_uris_are_pinned` asserts the `rho:id` the key
/// derives), and the contract's semantics are W2.2's, so what remains to prove here is the one thing
/// neither of those can: that a fresh chain seeds the alias and that the value behind it answers a
/// real multi-step call. The acceptance clause is the script that exercises the most of the contract
/// per unit of ceremony.
///
/// **Two gates because they fail differently**: W2.2 fails when the contract is wrong, and this one
/// fails when the *blessing* is wrong — and the blessing's failure mode is the one this repository
/// keeps meeting: a deploy that reports success, registers nothing, and leaves `lookup!` answering
/// `Nil` for ever, which is silent.
#[test]
fn a_fresh_chain_serves_ertp_through_the_shorthand_and_the_brand_check_still_holds() {
    with_big_stack(async {
        let rm = play_blessed(&[r#"
new kitA, kitB in {
  @ERTP!("makeIssuerKit", *kitA) |
  @ERTP!("makeIssuerKit", *kitB) |
  for (@(brandA, mintA, issuerA) <- kitA & @(brandB, mintB, issuerB) <- kitB) {
    @"kitA"!((brandA, mintA, issuerA)) |
    new amCh in {
      @issuerA!("getAmountMath", *amCh) |
      for (@(true, am) <- amCh) {
        new mc in {
          @am!("make", brandA, 10, *mc) |
          for (@(true, ten) <- mc) {
            new mp in {
              @mintA!("mintPayment", ten, *mp) |
              for (@(true, paymentA) <- mp) {
                @"payment"!(paymentA) |
                new pbc in {
                  @issuerB!("makeEmptyPurse", *pbc) |
                  for (@(true, purseB) <- pbc) {
                    new d1 in {
                      @purseB!("deposit", paymentA, *d1) |
                      for (@r1 <- d1) {
                        @"b-deposit-a"!(r1) |
                        new pac in {
                          @issuerA!("makeEmptyPurse", *pac) |
                          for (@(true, purseA) <- pac) {
                            new d2 in {
                              @purseA!("deposit", paymentA, *d2) |
                              for (@r2 <- d2) { @"a-deposit-a"!(r2) }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#])
        .await;

        // The kit arrived whole through the shorthand — a 3-tuple of three distinct names.
        let (brand_a, mint_a, issuer_a) = kit(&rm, "kitA").await;
        assert_ne!(brand_a, mint_a);
        assert_ne!(brand_a, issuer_a);
        assert!(
            mint_a != issuer_a,
            "the blessed contract must be the kit-making contract"
        );
        assert_capability(&one(&rm, "payment").await, "the minted payment");
        assert_eq!(
            refused(&rm, "b-deposit-a").await,
            "no such payment",
            "the structural refusal must hold through the alias tier too"
        );
        assert_eq!(
            answered_number(&rm, "a-deposit-a").await,
            10,
            "**and the refused payment must still be live** — the acceptance clause, on a real chain"
        );
    });
}
