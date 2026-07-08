//! Speculative decoding overhead benchmarks (ADR-007).
//!
//! Measures pure construction and selection costs for types in the adaptive
//! speculative decoding path. No GGUF models are required; all inputs are
//! synthetic. Results are CPU-only and should be interpreted as overhead
//! baselines, not end-to-end speedup figures.
//!
//! Run with `advanced` feature to exercise all groups:
//! ```text
//! cargo bench --bench speculative_matrix --features advanced
//! ```
//! Without `advanced`, a trivial `bench_noop` group is compiled instead.

#[cfg(not(feature = "advanced"))]
use criterion::Criterion;
use criterion::{criterion_group, criterion_main};

// ── Without `advanced`: trivial stub so the binary always compiles ────────────

#[cfg(not(feature = "advanced"))]
fn bench_noop(c: &mut Criterion) {
    let mut group = c.benchmark_group("speculative_noop");
    group.bench_function("no_op", |b| b.iter(|| 0u64));
    group.finish();
}

#[cfg(not(feature = "advanced"))]
criterion_group!(benches, bench_noop);

// ── With `advanced`: full speculative matrix ───────────────────────────────────

#[cfg(feature = "advanced")]
mod advanced_benches {
    use criterion::{black_box, BenchmarkId, Criterion};

    use gg_core::engine::adaptive_speculative::{DraftBlock, SurvivalProfile, VerificationPlan};
    use gg_core::models::SmartModelTier as ModelTier;
    use gg_core::models::{
        AdaptiveMode, AdaptiveSpeculativeConfig, HardwareProfile, TierSpeculativePlan,
    };

    // ── bench_speculative_config_creation ─────────────────────────────────────

    pub(super) fn bench_speculative_config_creation(c: &mut Criterion) {
        let mut group = c.benchmark_group("speculative_config_creation");

        group.bench_function("default_disabled", |b| {
            b.iter(|| black_box(AdaptiveSpeculativeConfig::default()))
        });

        group.bench_function("enabled_balanced", |b| {
            b.iter(|| {
                black_box(AdaptiveSpeculativeConfig {
                    enabled: true,
                    mode: AdaptiveMode::Balanced,
                    max_draft_tokens: 6,
                    ..AdaptiveSpeculativeConfig::default()
                })
            })
        });

        group.bench_function("enabled_aggressive", |b| {
            b.iter(|| {
                black_box(AdaptiveSpeculativeConfig {
                    enabled: true,
                    mode: AdaptiveMode::Aggressive,
                    max_draft_tokens: 8,
                    confidence_floor: 0.50,
                    ..AdaptiveSpeculativeConfig::default()
                })
            })
        });

        group.finish();
    }

    // ── bench_tier_plan_selection ─────────────────────────────────────────────

    pub(super) fn bench_tier_plan_selection(c: &mut Criterion) {
        let mut group = c.benchmark_group("tier_plan_selection");

        let tiers = &[ModelTier::Light, ModelTier::Balanced, ModelTier::Quality];
        let config = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Balanced,
            ..AdaptiveSpeculativeConfig::default()
        };

        for hw in [
            HardwareProfile::NoGpu,
            HardwareProfile::SingleGpu,
            HardwareProfile::MultiGpu,
        ] {
            let label = format!("{hw:?}");
            group.bench_with_input(BenchmarkId::new("hardware", &label), &hw, |b, &hw| {
                b.iter(|| {
                    black_box(TierSpeculativePlan::select(
                        black_box(tiers),
                        None,
                        hw,
                        black_box(0.75_f32),
                        &config,
                    ))
                })
            });
        }

        group.finish();
    }

    // ── bench_verification_plan_fallback ──────────────────────────────────────

    pub(super) fn bench_verification_plan_fallback(c: &mut Criterion) {
        let mut group = c.benchmark_group("verification_plan");

        group.bench_function("fallback", |b| {
            b.iter(|| black_box(VerificationPlan::fallback()))
        });

        group.bench_function("active_window_4", |b| {
            b.iter(|| {
                black_box(VerificationPlan {
                    window: black_box(4),
                    emit_correction: true,
                })
            })
        });

        group.bench_function("active_window_8", |b| {
            b.iter(|| {
                black_box(VerificationPlan {
                    window: black_box(8),
                    emit_correction: true,
                })
            })
        });

        group.finish();
    }

    // ── bench_survival_profile_uniform ────────────────────────────────────────

    pub(super) fn bench_survival_profile_uniform(c: &mut Criterion) {
        let mut group = c.benchmark_group("survival_profile_uniform");

        for &n in &[4_usize, 8, 16] {
            group.bench_with_input(BenchmarkId::new("tokens", n), &n, |b, &len| {
                b.iter(|| black_box(SurvivalProfile::uniform(black_box(len))))
            });
        }

        group.finish();
    }

    // ── bench_draft_block_construction ────────────────────────────────────────

    pub(super) fn bench_draft_block_construction(c: &mut Criterion) {
        let mut group = c.benchmark_group("draft_block_from_tokens");

        for &n in &[4_usize, 8, 16, 32] {
            let tokens: Vec<u32> = (0..n as u32).collect();
            group.bench_with_input(BenchmarkId::new("draft_len", n), &tokens, |b, toks| {
                b.iter(|| black_box(DraftBlock::from_tokens(black_box(toks.clone()))))
            });
        }

        group.finish();
    }
}

#[cfg(feature = "advanced")]
criterion_group!(
    benches,
    advanced_benches::bench_speculative_config_creation,
    advanced_benches::bench_tier_plan_selection,
    advanced_benches::bench_verification_plan_fallback,
    advanced_benches::bench_survival_profile_uniform,
    advanced_benches::bench_draft_block_construction,
);

criterion_main!(benches);
