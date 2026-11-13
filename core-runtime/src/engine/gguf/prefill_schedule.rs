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
//! chosen, and a bulk chunk is never below `MIN_CHUNK` (or `cap`). Chunks below
//! 64 tokens leave llama.cpp's batched path and cost about twice as much per
//! token, so they are used only for a final remainder, split into near-equal
//! pieces whose predicted cost stays within `max(BUDGET_MS, T64)` (plan
//! Revision 4, batches `mcal` and `shape`; the sub-64 bound is measured at
//! positions 2300 to 2600 only). Model-bytes scaling is measured
//! for Q4_K_M models of 0.49 to 2.4 GB on one host. Other quantizations, GPU
//! offload and contexts beyond 16k are unqualified.

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
/// Sub-64 piece cost bound `(p + PIECE_OFFSET) / PIECE_DENOM` of a 64-token
/// chunk at the same position (batch `shape`: worst of 2 runs at 1/2/8
/// threads, best of 2 at 4 threads; minimum margin 1.20).
const PIECE_OFFSET: usize = 6;
const PIECE_DENOM: usize = 24;

/// `[T0 × 2, T16 × 2]` in ms per thread bucket (rows: 1, 2, 4, 8 threads)
/// and chunk size (columns: `SIZES`). Doubling keeps the two-run mean
/// integral; printed from c184 by
/// `scripts/qualification/n7_prefill_schedule_model.py --integer`.
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

/// Tokens to decode in the chunk that starts at `pos` with `remaining`
/// prompt tokens left (`1..=remaining`). `cap` is the largest chunk the
/// context accepts. A final remainder below `MIN_CHUNK` is split into
/// near-equal pieces of at most `piece_limit` tokens (plan Revision 4).
pub(super) fn chunk(
    pos: usize,
    remaining: usize,
    threads: u32,
    model_bytes: u64,
    cap: usize,
) -> usize {
    debug_assert!(remaining > 0, "no tokens remain");
    let cap = cap.clamp(1, MAX_PREFILL_CHUNK);
    let row = bucket(threads);
    let bulk = SIZES
        .iter()
        .enumerate()
        .find(|&(col, &size)| size <= cap && fits(row, col, pos, model_bytes))
        .map_or(MIN_CHUNK.min(cap), |(_, &size)| size)
        .min(remaining);
    if remaining >= MIN_CHUNK.min(cap) {
        return bulk;
    }
    let piece = piece_limit(row, pos, model_bytes).clamp(1, bulk.max(1));
    let pieces = remaining.div_ceil(piece);
    remaining.div_ceil(pieces.max(1))
}

/// Largest sub-64 piece predicted to cost at most `max(BUDGET, T64)`, where
/// `T64` is the predicted 64-token chunk at `pos`. Measured bound (batch
/// `shape`): a `p`-token piece costs at most `(p + PIECE_OFFSET) /
/// PIECE_DENOM` of `T64`. Integer form: with `N = T64 · 2·SPAN·REF` scaled
/// as in `fits`, `p_max = 18` when `N ≥ BUDGET·D` and otherwise
/// `24·BUDGET·D / N − 6` (the quotient is then ≥ 24, so no underflow).
pub(super) fn piece_limit(row: usize, pos: usize, model_bytes: u64) -> usize {
    let [t0, t16] = TABLE[row][SIZES.len() - 1];
    let slope = t16.saturating_sub(t0);
    let at_pos = (t0 * SPAN).saturating_add(slope.saturating_mul(pos as u128));
    let n = at_pos.saturating_mul(u128::from(model_bytes));
    let budget = 2 * BUDGET_MS * SPAN * REF_MODEL_BYTES;
    if n == 0 {
        return usize::MAX;
    }
    if n >= budget {
        return PIECE_DENOM - PIECE_OFFSET;
    }
    let quotient = (PIECE_DENOM as u128 * budget) / n;
    usize::try_from(quotient).unwrap_or(usize::MAX) - PIECE_OFFSET
}

#[cfg(test)]
#[path = "prefill_schedule_tests.rs"]
mod tests;
