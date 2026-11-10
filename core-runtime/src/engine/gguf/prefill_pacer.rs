//! Time-budgeted prompt-prefill chunk sizing (N7, #184).
//!
//! A stop request (cancel or deadline) is only observed between prefill
//! decode chunks: llama.cpp cannot be interrupted inside one `decode` through
//! the safe wrapper. A fixed 512-token chunk took 1.2 to 12.5 s on the
//! qualification VM depending on the thread count, which is how long a
//! stopped request kept computing. The pacer sizes each chunk from the
//! measured speed of the previous one so a chunk aims at `BUDGET`.
//!
//! This is a target, not a hard bound: one chunk (at least `MIN_CHUNK`
//! tokens) and one generated-token decode always run to completion, and a
//! chunk can overrun when the host slows down after it was sized.
//!
//! Budget choice (Qwen2.5-0.5B Q4, 2,516-token prompt, 1/2/4 threads, both
//! qualified contexts, batch `ggn7m3`): smaller chunks decode fewer tokens
//! per second, so the budget trades stop latency against prefill time.
//!
//! | budget | longest chunk | prefill time vs fixed 512 |
//! |--------|---------------|---------------------------|
//! | 250 ms | 0.30-0.86 s   | +65% to +110%             |
//! | 500 ms | 0.56-0.84 s   | +7% to +95%               |
//! | 1 s    | 1.05-1.81 s   | +3% to +33% (one 67% run) |
//! | 2 s    | 1.79-2.47 s   | -16% to +10% (one 47% run)|

use std::time::Duration;

/// Wall-clock target for one cancellable prefill chunk.
pub(super) const BUDGET: Duration = Duration::from_millis(1000);
/// First chunk, before any speed is known.
pub(super) const START_CHUNK: usize = 32;
/// Smallest chunk the pacer will choose; bounds the throughput cost on a
/// slow host instead of degrading to token-at-a-time prefill.
pub(super) const MIN_CHUNK: usize = 16;

/// Chooses cancellable prefill chunk sizes within `[MIN_CHUNK, max]`.
#[derive(Debug, Clone, Copy)]
pub(super) struct PrefillPacer {
    chunk: usize,
    max: usize,
    budget: Duration,
}

impl PrefillPacer {
    /// `max` is the largest chunk the context and caller allow.
    pub(super) fn new(max: usize) -> Self {
        Self::with_budget(max, BUDGET)
    }

    /// As `new`, with an explicit per-chunk target (characterization only).
    pub(super) fn with_budget(max: usize, budget: Duration) -> Self {
        let max = max.max(1);
        Self {
            chunk: START_CHUNK.min(max),
            max,
            budget,
        }
    }

    /// Tokens to decode next, given how many remain.
    pub(super) fn next_len(&self, remaining: usize) -> usize {
        self.chunk.min(remaining)
    }

    /// Resize from the chunk just decoded. Growth is limited to doubling so
    /// one unusually fast chunk cannot schedule a long uninterruptible one.
    pub(super) fn record(&mut self, len: usize, elapsed: Duration) {
        if len == 0 {
            return;
        }
        let micros = elapsed.as_micros().max(1);
        let fit = (len as u128).saturating_mul(self.budget.as_micros()) / micros;
        let fit = usize::try_from(fit).unwrap_or(usize::MAX);
        let floor = MIN_CHUNK.min(self.max);
        self.chunk = fit.min(self.chunk.saturating_mul(2)).clamp(floor, self.max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A 500 ms pacer, so the arithmetic below does not move with `BUDGET`.
    fn half_second(max: usize) -> PrefillPacer {
        PrefillPacer::with_budget(max, ms(500))
    }

    #[test]
    fn default_budget_is_the_measured_choice() {
        let mut p = PrefillPacer::new(512);
        // 20 ms per token: the 1 s default fits 50 tokens.
        p.record(32, ms(640));
        assert_eq!(p.next_len(usize::MAX), 50);
    }

    #[test]
    fn starts_small_and_never_exceeds_max() {
        assert_eq!(PrefillPacer::new(512).next_len(10_000), START_CHUNK);
        assert_eq!(PrefillPacer::new(8).next_len(10_000), 8);
        assert_eq!(PrefillPacer::new(512).next_len(5), 5);
        assert_eq!(PrefillPacer::new(0).next_len(5), 1);
    }

    #[test]
    fn fast_host_grows_by_doubling_up_to_max() {
        let mut p = half_second(512);
        let mut seen = Vec::new();
        for _ in 0..6 {
            let n = p.next_len(usize::MAX);
            seen.push(n);
            p.record(n, ms(1));
        }
        assert_eq!(seen, [32, 64, 128, 256, 512, 512]);
    }

    #[test]
    fn slow_host_shrinks_to_budget_and_stops_at_min() {
        // 20 ms per token: 500 ms fits 25 tokens.
        let mut p = half_second(512);
        p.record(32, ms(640));
        assert_eq!(p.next_len(usize::MAX), 25);
        // 100 ms per token: the budget fits 5, the floor holds.
        p.record(25, ms(2500));
        assert_eq!(p.next_len(usize::MAX), MIN_CHUNK);
    }

    #[test]
    fn steady_host_converges_on_the_budget() {
        // 4 ms per token: 500 ms fits 125 tokens.
        let mut p = half_second(512);
        for _ in 0..8 {
            let n = p.next_len(usize::MAX);
            p.record(n, ms(4 * n as u64));
        }
        assert_eq!(p.next_len(usize::MAX), 125);
    }

    #[test]
    fn zero_elapsed_and_empty_chunk_are_safe() {
        let mut p = PrefillPacer::new(512);
        p.record(0, ms(0));
        assert_eq!(p.next_len(usize::MAX), START_CHUNK);
        p.record(32, Duration::ZERO);
        assert_eq!(p.next_len(usize::MAX), 64);
    }
}
