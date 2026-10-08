//! A token-bucket rate limiter.
//!
//! Shared by the unauthenticated deploy gRPC/HTTP servers and the Kademlia discovery RPC to bound
//! request rate (documented Scala deviations: those surfaces are unlimited in Scala), and by the
//! OCapN listener's audit lines.
//!
//! **It was a fixed window until 2026-10-07** (AUDIT C232). A window admits its whole allowance at
//! once and then resets on the wall clock, so a client that keeps asking is granted a fresh allowance
//! at each boundary — up to **twice** the configured rate over a sliding second, indefinitely, on a
//! surface whose whole purpose is to bound a rate. A bucket refills only what was spent, so the same
//! client is held to the rate it was given; the burst a fixed window allowed after an *idle* period
//! is preserved, because the bucket starts full.

use std::sync::Mutex;

// The wasm build takes the host's clock (`web-time`); `std`'s panics there (issue #98).
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// A token bucket holding at most one second's allowance, refilling continuously at the configured
/// rate.
pub struct RateLimiter {
    max_per_sec: u64,
    /// The tokens in hand, and when they were last accounted for. One lock rather than an `AtomicU64`
    /// beside a `Mutex`: the count and the instant are two halves of one fact, and a fixed window
    /// could get away with splitting them only because its reset was a store.
    state: Mutex<(f64, Instant)>,
}

impl RateLimiter {
    pub fn new(max_per_sec: u64) -> Self {
        RateLimiter {
            max_per_sec,
            // **Full at construction**, so a surface that has been quiet grants its whole allowance at
            // once — the burst the fixed window's first tick gave, and the reason a limiter sized for
            // an interactive caller does not make the first call wait.
            state: Mutex::new((max_per_sec as f64, Instant::now())),
        }
    }

    /// Admit a request if a token is available, refilling at the configured rate.
    pub fn allow(&self) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let (tokens, last) = &mut *state;
        let refill = now.duration_since(*last).as_secs_f64() * self.max_per_sec as f64;
        *tokens = (*tokens + refill).min(self.max_per_sec as f64);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tests are the only code that sleeps; the limiter itself reads the clock through `Instant`.
    use std::time::Duration;

    /// The bucket admits exactly `max_per_sec` requests: one more and the surface is closed until a
    /// token refills. This is the DoS bound the unauthenticated deploy/faucet routes rest on, and
    /// nothing tested it.
    #[test]
    fn admits_exactly_max_per_window_then_refuses() {
        let limiter = RateLimiter::new(3);
        assert!(limiter.allow(), "first");
        assert!(limiter.allow(), "second");
        assert!(limiter.allow(), "third");
        assert!(!limiter.allow(), "the fourth must be refused");
        assert!(!limiter.allow(), "and so must the fifth");
    }

    /// Zero means the surface is closed, not unlimited: the bucket holds nothing and refills at a
    /// rate of zero, so even the first request is refused. A limiter configured to zero protecting
    /// *less* than one configured to one would be exactly backwards.
    #[test]
    fn zero_never_admits() {
        let limiter = RateLimiter::new(0);
        for _ in 0..5 {
            assert!(!limiter.allow());
        }
    }

    /// The bucket refills, so a client that was refused is not refused forever.
    /// (The single sleep is the cost of testing a wall clock without injecting one.)
    #[test]
    fn refills_after_its_period() {
        let limiter = RateLimiter::new(1);
        assert!(limiter.allow(), "the first token");
        assert!(!limiter.allow(), "the second has nothing to spend");
        std::thread::sleep(Duration::from_millis(1100));
        assert!(limiter.allow(), "a second has refilled one");
    }

    /// **The allowance is never granted twice** (AUDIT C232). Under the fixed window this limiter
    /// used to be, a client that kept asking was handed a whole fresh allowance at each window
    /// boundary — twice the configured rate over a sliding second, on the surfaces whose entire
    /// purpose is to bound a rate. A bucket refills only what was spent, so a fraction of the period
    /// buys a fraction of a token and the whole allowance is never handed out twice. **Measured
    /// failing against the fixed window**: half a second into a one-second window it refused, because
    /// nothing refills until the boundary.
    #[test]
    fn the_allowance_is_never_granted_twice() {
        let limiter = RateLimiter::new(2);
        assert!(limiter.allow() && limiter.allow(), "the whole allowance");
        assert!(!limiter.allow(), "spent");
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            limiter.allow(),
            "half the period at two per second buys one token"
        );
        assert!(
            !limiter.allow(),
            "and one only: the allowance was refilled, not re-granted"
        );
    }
}
