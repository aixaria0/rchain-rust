//! Poison-aware lock accessors — **re-exported from `rchain-shared`**.
//!
//! The accessors used to live here, and `ocapn`/`comm` were the reason they could not stay: both
//! crates have poison-recovery sites and neither depends on `rspace`. `rchain-shared` is a
//! dependency of every crate with a site, so the module moved there (`C253` E2) and this file is the
//! re-export that keeps `rchain_rspace::lock::poison_recoveries()` — read by
//! `node/src/api/web_api_impl.rs` to publish `/api/status` — resolving unchanged.

pub use rchain_shared::lock::*;
