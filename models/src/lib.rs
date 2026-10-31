#![forbid(unsafe_code)]
//! Faithful Rust port of the RChain `models` module (the rholang term structure and its
//! canonicalizing sorter).
//!
//! Mirrors `models/src/main/protobuf/RhoTypes.proto` and
//! `models/src/main/scala/coop/rchain/models/rholang/sorter/`. Implements Law 1 (canonicalization):
//! `sort` is the total order that makes `Par` an order-insensitive container (`sort(sort p) =
//! sort p`, `sort(p | q) = sort(q | p)`).
//!
//! The protobuf AST is hand-written rather than derived with `prost`, because the load-bearing
//! contract is in-memory `Eq`/`Ord`/`Hash` + canonicalization (with `locallyFree` excluded from
//! equality via `AlwaysEqual`), not wire bytes. Wire serialization is deferred until
//! `comm`/`casper` block-hashing (Law 16) needs it.

pub mod ast;
pub mod block;
pub mod block_hash;
pub mod block_metadata;
pub mod block_version;
pub mod casper;
pub mod comm;
// **`decimal` is no longer a module** (issue #527, 2026-10-31): the exact decimal value type was
// removed before mainnet and `models/src/decimal.rs` is archived in place — full source as comments,
// not compiled. See `docs/src/formal/decimal-removed.md`. Wire tag 35 and sort rank 14 are retired.
pub mod errors;
pub mod murmur_hash3;
pub mod normalizer_env;
pub mod par_ops;
pub mod pretty;
pub mod proto;
pub mod rholang;
pub mod runtime;
pub mod sorted;
pub mod sorter;
pub mod string_syntax;
pub mod types;
pub mod validator;
pub mod wire;

#[cfg(test)]
mod property_tests;
