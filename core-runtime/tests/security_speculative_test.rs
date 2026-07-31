//! Security tests for speculative decoding — ADR-007 / Issue #67.
//!
//! Threat model binding (docs/security/THREAT_MODEL.md §12):
//!   T2 – target verification bypass (rejected suffix committed)
//!   T3 – telemetry / config PII leakage
//!   T4 – incompatible tokenizer pairing falls back to single-model
//!   T5 – auto-disable fires when acceptance rate below threshold;
//!         fallback plan has zero window

// ── T2 ───────────────────────────────────────────────────────────────────────
// Rejected draft suffix cannot be emitted.
//
// Uses the `advanced`-gated `VerificationResult::into_tokens` which is the
// single code path responsible for assembling output tokens from a
// speculation step.  Any token beyond `accepted_count` must never appear.

#[cfg(feature = "advanced")]
mod t2_verification_bypass {
    use gg_core::engine::adaptive_speculative::VerificationResult;

    /// Partial acceptance (2 of 4) + correction: output is exactly
    /// [draft[0], draft[1], correction].  draft[2] and draft[3] must be absent.
    #[test]
    fn t2_rejected_suffix_never_emitted() {
        let draft = vec![10u32, 20, 30, 40];
        let result = VerificationResult::reject_at(2, 99);

        let out = result.into_tokens(&draft);

        // Accepted prefix
        assert_eq!(out[0], 10, "first accepted token must be draft[0]");
        assert_eq!(out[1], 20, "second accepted token must be draft[1]");
        // Correction at rejection boundary
        assert_eq!(out[2], 99, "correction token must follow accepted prefix");
        // Total length: accepted_count + 1 correction; rejected suffix absent
        assert_eq!(out.len(), 3, "no tokens beyond accepted_count + correction");
        // Explicit absence of rejected tokens
        assert!(
            !out.contains(&30),
            "draft[2] (rejected) must not appear in output"
        );
        assert!(
            !out.contains(&40),
            "draft[3] (rejected) must not appear in output"
        );
    }

    /// Full acceptance: output equals the entire draft; no spurious tokens.
    #[test]
    fn t2_full_accept_emits_exactly_draft() {
        let draft = vec![1u32, 2, 3];
        let result = VerificationResult::accept_all(3);

        let out = result.into_tokens(&draft);

        assert_eq!(out, draft, "full acceptance must reproduce draft verbatim");
    }

    /// Zero acceptance with correction: only the correction token is emitted.
    #[test]
    fn t2_zero_accept_emits_only_correction() {
        let draft = vec![5u32, 6, 7];
        let result = VerificationResult::reject_at(0, 42);

        let out = result.into_tokens(&draft);

        assert_eq!(
            out,
            vec![42],
            "zero accepted tokens: only correction is emitted"
        );
        assert!(
            !out.contains(&5),
            "draft[0] must not appear when accepted_count=0"
        );
    }

    /// accepted_count larger than draft length saturates at draft.len().
    /// This guards against a corrupt VerificationResult with an inflated count.
    #[test]
    fn t2_accepted_count_overflow_saturates_at_draft_len() {
        let draft = vec![10u32, 20];
        // Simulate a corrupt result claiming more accepted tokens than exist.
        let result = VerificationResult {
            accepted_count: 999,
            correction_token: None,
            target_log_probs: vec![],
        };

        let out = result.into_tokens(&draft);

        // .take(999) on a 2-element slice must yield at most 2 elements.
        assert_eq!(
            out.len(),
            2,
            "output bounded by draft length regardless of accepted_count"
        );
        assert_eq!(out, draft);
    }
}

// ── T3 ───────────────────────────────────────────────────────────────────────
// Telemetry content safety: AdaptiveSpeculativeConfig must not carry fields
// that could store prompt text or PII.
//
// The test enumerates every field of the struct and asserts its type is one
// of the permitted numeric/boolean primitives.  Any future `String`,
// `Vec<u8>`, `Box<dyn Any>`, or opaque handle field will fail to match
// and the developer is forced to revisit PII policy.

#[cfg(feature = "advanced")]
mod t3_telemetry_pii {
    use gg_core::models::AdaptiveSpeculativeConfig;

    /// Verify that every public field of AdaptiveSpeculativeConfig has a type
    /// that cannot carry arbitrary text or opaque byte sequences.
    ///
    /// Permitted types: bool, usize, f32.  Any String or Vec field is a
    /// PII risk and must trigger a policy review before merge.
    #[test]
    fn t3_config_fields_contain_no_pii_types() {
        let cfg = AdaptiveSpeculativeConfig::default();

        // Exhaustive field access — if a field is added/renamed the
        // destructure will fail to compile, forcing a policy review.
        let AdaptiveSpeculativeConfig {
            enabled,
            mode: _,
            max_draft_tokens,
            min_verification_tokens,
            max_verification_tokens,
            confidence_floor,
            acceptance_floor,
            auto_disable,
            auto_disable_threshold,
            telemetry_enabled,
            cost_profiling,
            tier_aware,
            prompt_lookup_ngram,
        } = cfg;

        // All bool fields: no text content
        let _: bool = enabled;
        let _: bool = auto_disable;
        let _: bool = telemetry_enabled;
        let _: bool = cost_profiling;
        let _: bool = tier_aware;

        // All usize fields: numeric counts, cannot store text
        let _: usize = max_draft_tokens;
        let _: usize = min_verification_tokens;
        let _: usize = max_verification_tokens;
        let _: usize = prompt_lookup_ngram;

        // All f32 fields: numeric thresholds, cannot store text
        let _: f32 = confidence_floor;
        let _: f32 = acceptance_floor;
        let _: f32 = auto_disable_threshold;

        // If we reach here, the struct has no String/Vec/opaque fields.
        // The `mode` field is AdaptiveMode (enum of unit variants) — safe.
    }

    /// AdaptiveSpeculativeConfig default must have telemetry enabled
    /// (counters only, no content) and no cost profiling by default
    /// (extra overhead disabled unless explicitly opted in).
    #[test]
    fn t3_default_config_telemetry_safe_defaults() {
        let cfg = AdaptiveSpeculativeConfig::default();

        assert!(
            cfg.telemetry_enabled,
            "telemetry counters should be on by default"
        );
        assert!(
            !cfg.cost_profiling,
            "per-step cost profiling is off by default (overhead risk)"
        );
    }
}

// ── T4 ───────────────────────────────────────────────────────────────────────
// Incompatible tokenizer pairing falls back to single-model.
//
// When `config.enabled = false` (or `is_active()` returns false) the
// TierSpeculativePlan must return `is_speculative = false`, meaning the
// execution path uses only one model and tokenizer — eliminating any
// vocabulary-alignment risk.

#[cfg(feature = "advanced")]
mod t4_incompatible_pairing {
    use gg_core::models::SmartModelTier as ModelTier;
    use gg_core::models::{
        AdaptiveMode, AdaptiveSpeculativeConfig, HardwareProfile, TierSpeculativePlan,
    };

    fn disabled_config() -> AdaptiveSpeculativeConfig {
        AdaptiveSpeculativeConfig {
            enabled: false,
            ..Default::default()
        }
    }

    fn active_config() -> AdaptiveSpeculativeConfig {
        AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Balanced,
            acceptance_floor: 0.60,
            ..Default::default()
        }
    }

    /// When speculation is disabled via config, even a valid Light+Quality
    /// pairing must resolve to a single-model plan.
    #[test]
    fn t4_disabled_config_yields_single_model_plan() {
        let plan = TierSpeculativePlan::select(
            &[ModelTier::Light, ModelTier::Quality],
            None,
            HardwareProfile::SingleGpu,
            0.99,
            &disabled_config(),
        );

        assert!(
            !plan.is_speculative,
            "disabled config must yield single-model plan"
        );
        assert!(
            plan.draft_tier.is_none(),
            "single-model plan must not carry a draft tier"
        );
        assert!(
            plan.fallback_reason.is_some(),
            "single-model plan must carry a fallback reason"
        );
    }

    /// When mode is Disabled (even with enabled=true), plan must fall back.
    #[test]
    fn t4_disabled_mode_yields_single_model_plan() {
        let cfg = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Disabled,
            ..Default::default()
        };

        let plan = TierSpeculativePlan::select(
            &[ModelTier::Light, ModelTier::Quality],
            None,
            HardwareProfile::SingleGpu,
            0.99,
            &cfg,
        );

        assert!(
            !plan.is_speculative,
            "Disabled mode must yield single-model plan"
        );
    }

    /// When acceptance rate is below the configured floor, pairing falls back.
    /// This ensures a mismatch-induced acceptance collapse cannot persist.
    #[test]
    fn t4_low_acceptance_rate_yields_single_model_plan() {
        let cfg = active_config(); // acceptance_floor = 0.60
        let plan = TierSpeculativePlan::select(
            &[ModelTier::Light, ModelTier::Quality],
            None,
            HardwareProfile::SingleGpu,
            0.30, // well below 0.60 floor — as if tokenizer mismatch degraded acceptance
            &cfg,
        );

        assert!(
            !plan.is_speculative,
            "acceptance below floor must fall back to single-model"
        );
    }

    /// When no compatible tier pair is available, plan is single-model.
    #[test]
    fn t4_no_compatible_tier_yields_single_model_plan() {
        let cfg = active_config();
        let plan = TierSpeculativePlan::select(
            &[ModelTier::Quality], // only one tier — no pair possible
            None,
            HardwareProfile::SingleGpu,
            0.99,
            &cfg,
        );

        assert!(
            !plan.is_speculative,
            "single available tier must yield single-model plan"
        );
    }
}

// ── T5 ───────────────────────────────────────────────────────────────────────
// Auto-disable fires when rolling acceptance history falls below threshold,
// and VerificationPlan::fallback() always produces window=0.

#[cfg(feature = "advanced")]
mod t5_auto_disable {
    use gg_core::engine::adaptive_speculative::heuristic::AdaptiveVerificationScheduler;
    use gg_core::engine::adaptive_speculative::{
        DraftBlock, SurvivalProfile, VerificationPlan, VerificationScheduler,
    };
    use gg_core::models::{AdaptiveMode, AdaptiveSpeculativeConfig};

    fn scheduler_with_threshold(threshold: f32) -> AdaptiveVerificationScheduler {
        let config = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Balanced,
            auto_disable: true,
            auto_disable_threshold: threshold,
            max_draft_tokens: 4,
            min_verification_tokens: 1,
            max_verification_tokens: 8,
            ..Default::default()
        };
        AdaptiveVerificationScheduler::new(config)
    }

    fn sample_draft() -> DraftBlock {
        DraftBlock::from_tokens(vec![1u32, 2, 3, 4])
    }

    fn uniform_profile(n: usize) -> SurvivalProfile {
        SurvivalProfile::uniform(n)
    }

    /// Auto-disable fires when acceptance history is consistently low.
    ///
    /// The scheduler records repeated 0.0 acceptance fractions (all tokens
    /// rejected every step).  After enough samples the rolling mean plus 1.0
    /// falls below `auto_disable_threshold`, and `plan()` must return fallback.
    #[test]
    fn t5_auto_disable_fires_below_threshold() {
        // threshold = 1.5 means (1.0 + mean_acceptance) must be >= 1.5,
        // i.e. mean_acceptance >= 0.5.  We will record 0.0 repeatedly.
        let scheduler = scheduler_with_threshold(1.5);

        // Prime history with all-rejection results.
        for _ in 0..32 {
            scheduler.record_result(0, 4); // 0 accepted out of 4
        }

        let plan = scheduler.plan(&sample_draft(), &uniform_profile(4));

        assert!(
            plan.is_fallback(),
            "auto-disable must fire and return fallback after sustained low acceptance"
        );
        assert!(
            scheduler.auto_disable_fired(),
            "auto_disable_fired() must return true after threshold breach"
        );
    }

    /// Auto-disable does NOT fire when acceptance is healthy.
    #[test]
    fn t5_auto_disable_does_not_fire_above_threshold() {
        let scheduler = scheduler_with_threshold(1.05); // requires only 5% speedup
                                                        // Record high-acceptance history.
        for _ in 0..32 {
            scheduler.record_result(4, 4); // all 4 tokens accepted
        }

        let plan = scheduler.plan(&sample_draft(), &uniform_profile(4));

        assert!(
            !scheduler.auto_disable_fired(),
            "auto-disable must not fire when acceptance is healthy"
        );
        // Plan should not be fallback when acceptance is high.
        // (May still be fallback if window computation rounds to 0, but
        //  auto_disable_fired must be false.)
        let _ = plan; // result used; avoid unused-variable warning
    }

    /// VerificationPlan::fallback() always has window=0.
    ///
    /// A window of zero means the executor cannot pass any draft tokens to
    /// the target model — the only path forward is generate_one().
    #[test]
    fn t5_fallback_plan_has_zero_window() {
        let plan = VerificationPlan::fallback();

        assert_eq!(plan.window, 0, "fallback plan window must be zero");
        assert!(
            plan.is_fallback(),
            "is_fallback() must return true for fallback plan"
        );
        assert!(
            !plan.emit_correction,
            "fallback plan must not request a correction token"
        );
    }

    /// Scheduler with Disabled mode always returns fallback (window=0).
    #[test]
    fn t5_disabled_mode_always_returns_fallback() {
        let config = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Disabled,
            auto_disable: false, // isolate: ensure it's the mode causing fallback
            ..Default::default()
        };
        let scheduler = AdaptiveVerificationScheduler::new(config);

        let plan = scheduler.plan(&sample_draft(), &uniform_profile(4));

        assert!(
            plan.is_fallback(),
            "Disabled mode must return fallback regardless of history"
        );
        assert_eq!(plan.window, 0, "Disabled mode fallback window must be zero");
    }
}
