# AUDIT REPORT — Gate Tribunal (ONNX classifier #72)

**Target**: docs/plan-onnx-classifier-2026-07-26.md (iteration 1)
**Date**: 2026-07-26
**Session**: 2026-07-26T1930-onnxcls
**Risk Grade**: L2
**Mode**: adversarial (independent fresh-context Judge subagent)

## VERDICT: PASS

Infrastructure claims all grep-verified; Razor holds; feature-gating preserved;
the pure `logits_to_classification` helper is genuinely CI-testable with a
synthetic tensor (no fixture). No blocking findings. L2 correct (engine compute,
onnx-gated, no security surface).

### Must-honor advisories (baked into implementation)
1. **Deterministic classifier output selection.** Do NOT copy the embedder's
   `outputs.values().next()` (HashMap order is nondeterministic; unsound for
   multi-output exports). Use `outputs.get("logits")`; if absent, accept a
   single-output model, else `Err(ModelError("ambiguous classifier outputs"))`.
   Fail loud, don't silently misclassify. (The `labels.len()!=width` guard is
   only a partial backstop.)
2. **Non-onnx dead-code.** Removing `#[allow(dead_code)]` on `labels` (LD-1)
   would warn on the non-onnx build; keep it cfg-conditional so `-D warnings`
   stays clean without the feature.
3. **Razor ceiling.** classifier.rs is 94 lines; if the additions push it toward
   250, extract `logits_to_classification` to `onnx/common.rs` (already
   contemplated).
4. **FEATURE_INDEX credit.** Foreground the classifier.rs unit tests as the real
   coverage; the tier2 file is simulation-only.

### Next action
`/qor-implement` authorized. Commit locally; push/PR at operator direction.
