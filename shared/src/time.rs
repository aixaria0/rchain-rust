//! Time utilities (port of the sync half of `shared/Time.scala`).
//!
//! The cats-effect `Time[F]`/`Timer[F]` abstraction is simplified to plain functions.

use std::sync::OnceLock;
use std::time::Duration;

// `std::time::{Instant, SystemTime}` compile for `wasm32-unknown-unknown` but panic at runtime — the
// target has no clock in `std` — so the wasm build takes the host's clock (`web-time`, backed by
// `performance.now`/`Date`). No production path here calls `std`'s clock directly on that target
// (`candidate:host-supplied-clock`, issue #98).
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
use web_time::{Instant, SystemTime, UNIX_EPOCH};

/// Current epoch time in milliseconds (port of `Time.currentMillis`).
pub fn current_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Monotonic time in nanoseconds since process start (port of `Time.nanoTime`).
pub fn nano_time() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as i64
}

/// Sleep for the given duration (port of `Time.sleep`).
///
/// **No-op on `wasm32-unknown-unknown`**, which has neither a blocking sleep in `std` nor threads to
/// block on: a wasm host drives time asynchronously, so a synchronous sleep cannot be honoured. The
/// reducer must not depend on `sleep` for pacing; the wasm arm is kept only so the signature holds.
#[cfg(not(target_arch = "wasm32"))]
pub fn sleep(duration: Duration) {
    std::thread::sleep(duration);
}

#[cfg(target_arch = "wasm32")]
pub fn sleep(_duration: Duration) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_millis_is_epoch_based() {
        // Sanity: any post-2017 epoch-millis value.
        assert!(current_millis() > 1_500_000_000_000);
    }

    #[test]
    fn nano_time_is_monotonic() {
        let a = nano_time();
        let b = nano_time();
        assert!(b >= a);
    }

    #[test]
    fn sleep_blocks_for_at_least_the_duration() {
        let start = Instant::now();
        sleep(Duration::from_millis(5));
        assert!(start.elapsed() >= Duration::from_millis(5));
    }
}
