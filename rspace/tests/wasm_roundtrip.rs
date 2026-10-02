//! RSpace over a host-provided in-memory store, on `wasm32-unknown-unknown` — the "deploy against a
//! state → new state" shape at the storage layer (issue #98).
//!
//! This is the test the store seam exists for. Before it, `KeyValueTypedStoreCodec` routed every
//! operation through `tokio::task::spawn_blocking`, so an RSpace built on wasm *compiled* — that is
//! why the check in #187 passed — and then panicked on the first operation, because the target has no
//! threads to run the blocking pool on. `create_rspace` → `create_checkpoint` is exactly that path.
//!
//! Run: `cargo test -p rchain-rspace --target wasm32-unknown-unknown`.

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use rchain_rspace::errors::RSpaceError;
use rchain_rspace::factory::create_rspace;
use rchain_rspace::i_space::ISpace;
use rchain_rspace::match_::Match;
use rchain_rspace::tuple_space::Tuplespace;
use rchain_shared::store_manager::InMemoryStoreManager;
use wasm_bindgen_test::wasm_bindgen_test;

/// The same trivial matcher the in-crate tests use — this pins the store, not the matcher.
struct StrMatch;

impl Match<String, String> for StrMatch {
    fn get(&self, _p: &String, a: &String) -> Result<Option<String>, RSpaceError> {
        Ok(Some(a.clone()))
    }
}

#[wasm_bindgen_test]
async fn an_in_memory_rspace_round_trips_on_wasm() {
    let manager = InMemoryStoreManager::default();
    let space = create_rspace::<String, String, String, String>(&manager, Arc::new(StrMatch))
        .await
        .expect("create the space over the in-memory store");

    let before = space.create_checkpoint().await.expect("checkpoint").root;

    space
        .produce("c".to_string(), "data".to_string(), false)
        .await
        .expect("produce");

    let after = space.create_checkpoint().await.expect("checkpoint").root;
    assert_ne!(before, after, "a produce must advance the state root");

    let data = space.get_data(&"c".to_string()).await.expect("get_data");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].a, "data");
}
