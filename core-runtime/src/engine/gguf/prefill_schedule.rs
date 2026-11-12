//! Deterministic prompt-prefill chunk schedule (N7 successor, #184).
//!
//! A stop request (cancel or deadline) is observed only between prefill
//! decode chunks, so a chunk's duration is the stop tail. The former
//! `PrefillPacer` sized chunks from measured wall time. llama.cpp CPU logits
//! depend on how a prompt is split into decode batches, so the split, and
//! with it greedy output, then changed with host load (review experiment
//! `d184`).
//!
//! This schedule is a pure function of the chunk's start position, the
//! thread bucket, the model's tensor bytes and the decode cap. It takes no
//! time input. The same model file, thread bucket, prompt and sampling give
//! the same split, and so the same output, under any load and concurrency,
//! with or without a stop check.
//!
//! Output can still differ between thread buckets. It can also differ
//! between the embedded and daemon paths on one host: embedded auto threads
//! are `min(logical, 16)`, while the daemon divides that by its worker
//! count. A different CPU instruction set is not covered either.
//!
//! The cost model comes from calibration batch `c184`
//! (`docs/integration/N7_PREFILL_CALIBRATION_c184.txt`, Qwen2.5-0.5B Q4_K_M,
//! 12-vCPU VM). It measures one chunk's time at position 0 (`T0`, the mean of
//! two runs) and at position 13 926 (`T16`), per chunk size and thread
//! bucket. Time is assumed linear in the start position and proportional to
//! model bytes. The largest chunk predicted to take at most `BUDGET_MS` is
//! chosen, and the result is never below `MIN_CHUNK` (or `cap`). Chunks below
//! 64 tokens cost 1.6 to 3.6 times as much per token at late positions, so
//! they are not candidates. Scaling to other model sizes, quantizations, GPU
//! offload or contexts beyond 16k is unqualified (see the plan, Revision 3).

/// Largest chunk the schedule will choose (also the decode cap it applies).
pub(super) const MAX_PREFILL_CHUNK: usize = 512;
/// Smallest chunk the schedule chooses unless the decode cap is smaller.
pub(super) const MIN_CHUNK: usize = 64;
/// Predicted duration target for one chunk, in milliseconds.
const BUDGET_MS: u128 = 1000;
/// Tensor bytes (`LlamaModel::size`) of the calibration model.
const REF_MODEL_BYTES: u128 = 485_452_288;
/// Calibration position of `T16`.
const SPAN: u128 = 13_926;
/// Candidate chunk sizes, largest first; `TABLE` columns follow this order.
const SIZES: [usize; 4] = [512, 256, 128, 64];

/// `[T0 × 2, T16 × 2]` in ms per thread bucket (rows: 1, 2, 4, 8 threads)
/// and chunk size (columns: `SIZES`). Doubling keeps the two-run mean
/// integral; generated from c184 by the plan's model script.
const TABLE: [[[u128; 2]; 4]; 4] = [
    [[9906, 37294], [4907, 16798], [2669, 7996], [1322, 3918]],
    [[5319, 16384], [2613, 7830], [1354, 4246], [714, 2434]],
    [[3030, 9870], [1539, 4564], [748, 2584], [404, 1702]],
    [[2422, 6018], [1186, 3194], [639, 1802], [387, 1458]],
];

/// Table row for a resolved thread count, rounded down to the nearest
/// calibrated count (1, 2, 4, 8) so predictions err slow.
pub(super) fn bucket(threads: u32) -> usize {
    match threads {
        0..=1 => 0,
        2..=3 => 1,
        4..=7 => 2,
        _ => 3,
    }
}

/// Whether a chunk of `SIZES[col]` starting at `pos` is predicted within
/// budget. Exact integer comparison (no division): `(T0·SPAN + (T16−T0)·pos)
/// · bytes ≤ 2·BUDGET·SPAN·REF`, saturating so huge inputs never fit.
fn fits(row: usize, col: usize, pos: usize, model_bytes: u64) -> bool {
    let [t0, t16] = TABLE[row][col];
    let slope = t16.saturating_sub(t0);
    let at_pos = (t0 * SPAN).saturating_add(slope.saturating_mul(pos as u128));
    let predicted = at_pos.saturating_mul(u128::from(model_bytes));
    predicted <= 2 * BUDGET_MS * SPAN * REF_MODEL_BYTES
}

/// Tokens to decode in the chunk that starts at `pos`, before clamping to
/// the tokens that remain. `cap` is the largest chunk the context accepts.
pub(super) fn chunk(pos: usize, threads: u32, model_bytes: u64, cap: usize) -> usize {
    let cap = cap.clamp(1, MAX_PREFILL_CHUNK);
    let row = bucket(threads);
    SIZES
        .iter()
        .enumerate()
        .find(|&(col, &size)| size <= cap && fits(row, col, pos, model_bytes))
        .map_or(MIN_CHUNK.min(cap), |(_, &size)| size)
}

#[cfg(test)]
#[path = "prefill_schedule_tests.rs"]
mod tests;
