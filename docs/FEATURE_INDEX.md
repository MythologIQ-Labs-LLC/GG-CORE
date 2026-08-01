# Feature Index

Reconstructed governance artifact (required, non-scaffold). Repaired via
`/qor-remediate` on 2026-07-08 because the Phase 109 governance-health schema
requires it and it had never been created since bootstrap. Content is grounded
in the actual `core-runtime/` source tree and `core-runtime/tests/` suite — not
a scaffold placeholder.

## Purpose

One row per shipped GG-CORE subsystem, mapping each feature to its source of
truth, a design citation, its test binding, and a verification status. This is
the surface the substantiation seal diffs against to catch outside-scope
`verified -> unverified` regressions (`qor.scripts.feature_index_verify`).

## Verification status semantics

- **verified** — a direct, named test binding exists for the subsystem and is
  wired into the workspace test suite.
- **unverified** — coverage is absent, indirect only, or the subsystem has a
  known open defect (cross-referenced in `docs/BACKLOG.md`).
- **n/a** — not a testable runtime feature (documentation or scaffolding).

Per SG-035, `verified` remains subject to operator deep-verification (does the
test actually exercise the feature?); this index records the binding, not that
judgment.

## Index

| ID | Name | Source-of-truth | Doc citation | Test path | Verification status |
|----|------|-----------------|--------------|-----------|---------------------|
| F-01 | IPC transport & protocol | core-runtime/src/ipc/ | docs/IPC_PROTOCOL_SCHEMA.md | core-runtime/tests/ipc_server_test.rs | verified |
| F-02 | IPC protocol versioning | core-runtime/src/ipc/ | docs/IPC_PROTOCOL_SCHEMA.md | core-runtime/tests/protocol_version_test.rs | verified |
| F-03 | IPC caller authentication | core-runtime/src/ipc/ | docs/CONCEPT.md | core-runtime/tests/auth_test.rs | verified |
| F-04 | Connection management | core-runtime/src/ipc/ | docs/CONCEPT.md | core-runtime/tests/connections_test.rs | verified |
| F-05 | Scheduler (queue/priority/batching) | core-runtime/src/scheduler/ | docs/CONCEPT.md | core-runtime/tests/scheduler_test.rs | verified |
| F-06 | Graceful drain & shutdown | core-runtime/src/shutdown.rs | docs/CONCEPT.md | core-runtime/tests/shutdown_test.rs | verified |
| F-07 | Request timeout & cancellation | core-runtime/src/scheduler/ | docs/CONCEPT.md | core-runtime/tests/timeout_cancel_test.rs | verified |
| F-08 | Inference engine core | core-runtime/src/engine/inference.rs | docs/CONCEPT.md | core-runtime/src/engine/inference_tests.rs | verified |
| F-09 | Tokenizer & encoding | core-runtime/src/engine/tokenizer.rs | docs/CONCEPT.md | core-runtime/tests/tokenizer_test.rs | verified |
| F-10 | Streaming output | core-runtime/src/engine/streaming.rs | docs/CONCEPT.md | core-runtime/tests/streaming_test.rs | verified |
| F-11 | GGUF backend (llama-cpp-2) | core-runtime/src/engine/gguf/ | docs/RECOMMENDED_MODELS.md | core-runtime/tests/integration_gguf_test.rs | verified |
| F-12 | ONNX backend (embed/classify) | core-runtime/src/engine/onnx/ | docs/RECOMMENDED_MODELS.md | core-runtime/tests/integration_onnx_test.rs | verified |
| F-13 | ONNX classification (real candle-onnx inference via Runtime/engine) | core-runtime/src/engine/onnx/classifier.rs | docs/RECOMMENDED_MODELS.md | core-runtime/src/engine/onnx/classifier.rs (logits_to_classification unit tests) | verified |
| F-14 | Mixture-of-Experts routing | core-runtime/src/engine/moe/ | docs/CONCEPT.md | core-runtime/tests/moe_test.rs | verified |
| F-15 | GPU allocation & management | core-runtime/src/engine/gpu_allocator.rs | docs/architecture/SCALABILITY_REMEDIATION_UPGRADE_PATH.md | core-runtime/tests/gpu_v2_test.rs | verified |
| F-16 | Multi-GPU exec/partition/pipeline | core-runtime/src/engine/multi_gpu_exec.rs | docs/architecture/SCALABILITY_REMEDIATION_UPGRADE_PATH.md | core-runtime/src/engine/multi_gpu_tests.rs | verified |
| F-17 | Flash attention | core-runtime/src/engine/flash_attn.rs | docs/architecture/V0.6.0_TRADE_OFFS.md | core-runtime/tests/flash_attn_test.rs | verified |
| F-19 | Quantization & KV quant | core-runtime/src/engine/quantize.rs | docs/architecture/V0.6.0_TRADE_OFFS.md | core-runtime/tests/kv_quant_test.rs | verified |
| F-20 | SIMD matmul / NEON | core-runtime/src/engine/simd_matmul.rs | docs/architecture/V0.6.0_TRADE_OFFS.md | core-runtime/tests/simd_matmul_test.rs | verified |
| F-21 | KV cache & paged/continuous | core-runtime/src/memory/ | docs/architecture/SCALABILITY_REMEDIATION_UPGRADE_PATH.md | core-runtime/tests/kv_cache_test.rs | verified |
| F-22 | Memory pool | core-runtime/src/memory/ | docs/CONCEPT.md | core-runtime/tests/memory_test.rs | verified |
| F-23 | Prompt cache | core-runtime/src/engine/ | docs/CONCEPT.md | core-runtime/tests/prompt_cache_test.rs | verified |
| F-24 | Model loader/registry/hot-swap | core-runtime/src/models/ | docs/CONCEPT.md | core-runtime/tests/integration_gguf_test.rs | verified |
| F-25 | Model routing | core-runtime/src/models/ | docs/CONCEPT.md | core-runtime/tests/model_router_test.rs | verified |
| F-26 | Model preload & warmup | core-runtime/src/models/ | docs/CONCEPT.md | core-runtime/tests/warmup_test.rs | verified |
| F-27 | Telemetry (Prometheus/OTel) | core-runtime/src/telemetry/ | docs/CONCEPT.md | core-runtime/tests/telemetry_test.rs | verified |
| F-28 | Metrics export | core-runtime/src/telemetry/ | docs/CONCEPT.md | core-runtime/tests/metrics_export_test.rs | verified |
| F-29 | A/B testing (traffic/metrics) | core-runtime/src/ab_testing/ | docs/CONCEPT.md | core-runtime/tests/ab_testing_test.rs | verified |
| F-30 | Canary deployment & rollback | core-runtime/src/deployment/ | docs/architecture/ADR-006-DEPLOYMENT-STRATEGIES.md | core-runtime/tests/canary_deployment_test.rs | verified |
| F-31 | Blue-green deployment & rollback | core-runtime/src/deployment/ | docs/architecture/ADR-006-DEPLOYMENT-STRATEGIES.md | core-runtime/tests/bluegreen_deployment_test.rs | verified |
| F-32 | Security: input validation | core-runtime/src/security/ | docs/security | core-runtime/tests/security_input_validation_test.rs | verified |
| F-33 | Security: path-traversal defense | core-runtime/src/security/ | docs/security | core-runtime/tests/security_path_traversal_test.rs | verified |
| F-34 | Security: output filter | core-runtime/src/engine/filter.rs | docs/security | core-runtime/tests/filter_test.rs | verified |
| F-35 | Security: encryption & FIPS | core-runtime/src/security/encryption.rs | docs/security | core-runtime/src/security/fips_tests.rs | verified |
| F-36 | Security: PII detection | core-runtime/src/security/pii_detector.rs | docs/security | core-runtime/src/security/pii_tests.rs; core-runtime/src/scheduler/worker_security_tests.rs | verified |
| F-37 | Security: prompt-injection guard | core-runtime/src/security/prompt_injection.rs | docs/security | core-runtime/src/security/sanitizer_tests.rs; core-runtime/tests/security_pipeline_wiring_test.rs | verified |
| F-38 | Sandbox isolation (unix-gated; CI-verified) | core-runtime/src/sandbox/ | docs/CONCEPT.md | core-runtime/tests/sandbox_test.rs | verified |
| F-39 | C FFI bindings (inference routes through secure façade Runtime::infer; deadlock fixed, security-enforced) | core-runtime/src/ffi/ | docs/USAGE_GUIDE.md | core-runtime/tests/ffi_test.rs (ffi acceptance un-ignored + injection→SecurityRejected; + CI ffi leg: .github/workflows/rust.yml features/ffi — clippy -D warnings + build + test) | verified |
| F-40 | Python (PyO3 0.29 / pyo3-async-runtimes 0.29) bindings (Session/AsyncSession::infer route through secure façade Runtime::infer; deadlock fixed, security-enforced; RUSTSEC-2026-0176/0177/2025-0020 cleared) | core-runtime/src/python/ | docs/USAGE_GUIDE.md | core-runtime/tests/python_binding_test.rs (CI python leg: .github/workflows/rust.yml features/python — cargo test --features python) | verified |
| F-41 | CLI (health/status/config/models) | core-runtime/src/cli/ | docs/USAGE_GUIDE.md | core-runtime/tests/cli_test.rs | verified |
| F-42 | Health probe | core-runtime/src/health.rs | docs/USAGE_GUIDE.md | core-runtime/tests/health_test.rs | verified |
| F-43 | Config & resource limits | core-runtime/src/config.rs | docs/CONCEPT.md | core-runtime/tests/limits_test.rs | verified |
| F-44 | K8s hardware profiles | core-runtime/src/k8s/ | docs/CONCEPT.md | core-runtime/src/k8s/profiles_tests.rs | verified |
| F-45 | Veritas shim (rate-limit/service-tier) | core-runtime/src/shim/ | docs/META_LEDGER.md | core-runtime/src/shim/rate_limiter.rs; core-runtime/src/shim/service_tier.rs; core-runtime/src/shim/mod.rs | verified |
| F-46 | Chaos resilience harness | core-runtime/src/deployment/ | docs/architecture/ADR-006-DEPLOYMENT-STRATEGIES.md | core-runtime/tests/chaos_resilience_test.rs | verified |
| F-47 | Benchmark suite | core-runtime/benches/ | docs/BENCHMARKS.md | core-runtime/tests/bench_fixtures_test.rs | verified |
| F-48 | Adaptive speculative config (AdaptiveSpeculativeConfig) | core-runtime/src/models/speculative_config.rs | docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md | core-runtime/src/models/speculative_config.rs (inline tests) | verified |
| F-49 | Adaptive speculative decoder interfaces (traits + types) | core-runtime/src/engine/adaptive_speculative/ | docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md | core-runtime/src/engine/adaptive_speculative/tests.rs | verified |
| F-50 | Heuristic confidence estimator + adaptive verification scheduler | core-runtime/src/engine/adaptive_speculative/heuristic/ | docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md | core-runtime/src/engine/adaptive_speculative/heuristic/tests.rs | verified |
| F-51 | TierSynergy speculative execution plan (TierSpeculativePlan) | core-runtime/src/models/tier_synergy_speculative.rs | docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md | core-runtime/src/models/tier_synergy_speculative_tests.rs | verified |
| F-52 | Speculative decoding threat model + security oracles | docs/security/THREAT_MODEL.md | docs/security/THREAT_MODEL.md | core-runtime/tests/security_speculative_test.rs | verified |
| F-53 | Speculative telemetry + auto-disable + CLI surface (SpeculativeTelemetry) | core-runtime/src/engine/adaptive_speculative/telemetry.rs | docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md | core-runtime/src/engine/adaptive_speculative/telemetry_tests.rs | verified |
| F-54 | Speculative benchmark matrix | core-runtime/benches/speculative_matrix.rs | docs/BENCHMARKS.md | core-runtime/benches/speculative_matrix.rs | verified |
| F-55 | Security: pipeline wiring (ingress scan + egress sanitize) | core-runtime/src/security/pipeline.rs | docs/security | core-runtime/tests/security_pipeline_wiring_test.rs | verified |
| F-56 | Secure inference façade (Runtime::infer/infer_stream) | core-runtime/src/runtime_facade.rs | docs/security | core-runtime/tests/secure_facade_test.rs | verified |
| F-57 | Manifest-driven model-load dispatch (GGUF/ONNX by sibling manifest.json; prod FFI+Python) | core-runtime/src/models/backend_dispatch.rs | docs/research-brief-b29b2-manifest-dispatch-2026-07-28.md | core-runtime/src/models/backend_dispatch_tests.rs | verified |
| F-58 | Degraded-mode policy (intentional, explained degradation under resource pressure; context reduction before hard-fail) | core-runtime/src/engine/degraded_mode.rs | docs/research-brief-b07-degraded-mode-2026-07-28.md | core-runtime/src/engine/degraded_mode_tests.rs | verified |
| F-59 | Streaming egress PII sanitizer (token-by-token redaction; O(n) cached-stable-prefix; gguf-gated) | core-runtime/src/security/stream_sanitizer.rs | docs/research-brief-b36-incremental-stream-sanitize-2026-07-30.md | core-runtime/src/security/stream_sanitizer_diff_tests.rs | verified |
| F-60 | Prompt KV cache (exact + longest-prefix match; O(n) find_prefix) | core-runtime/src/memory/prompt_cache.rs | docs/research-brief-b38-profile-memory-2026-07-30.md | core-runtime/tests/prompt_cache_test.rs | verified |
| F-61 | Adaptive speculative decoding — LIVE on the inference path (config-gated in Runtime::infer; rejected suffix never committed; single-model fallback; advanced-gated) | core-runtime/src/engine/adaptive_speculative/executor.rs | docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md | core-runtime/src/engine/adaptive_speculative/executor_tests.rs | verified |
| F-62 | Speculative KV-cache reuse — persistent GGUF session (self_cell; delta-decode + draft rollback; token-equivalent to fresh context; advanced-gated) | core-runtime/src/engine/gguf/speculative_session.rs | docs/research-brief-b21f-kv-cache-reuse-2026-07-31.md | core-runtime/src/engine/gguf/speculative_session_tests.rs | verified |
| F-63 | Prompt-lookup draft — model-free n-gram speculative draft (BlockDraftModel; advanced-gated) | core-runtime/src/engine/adaptive_speculative/prompt_lookup.rs | docs/research-brief-b21f-kv-cache-reuse-2026-07-31.md | core-runtime/src/engine/adaptive_speculative/prompt_lookup_tests.rs | verified |

## Open coverage gaps

Tracked as backlog items in `docs/BACKLOG.md`:

- **F-38 Sandbox** — `unverified`: lint-only fix for issue #54 landed on
  `chore/hardening-ci-sandbox-lints` (cycle 1, session 2026-07-08T1556-3b7852);
  flip to `verified` is gated on a green Linux/macOS CI run after operator push
  (`.github/workflows/rust.yml`). Canonical: GitHub issue #54.
- **F-40 Python bindings** — resolved to `verified` (session
  2026-07-26T0030-b25ffi, ledger Entry #105): the `python` feature is now built,
  linted (`clippy -D warnings`), and tested in CI via the `features` matrix job
  (`.github/workflows/rust.yml` features/python), so `python_binding_test.rs`
  executes with the feature enabled. F-39 (FFI) is likewise CI-covered via
  features/ffi. The FFI/Python inference reroute onto `Runtime::infer`
  (deadlock fix + security enforcement) shipped in B-25b (session
  2026-07-26T1850-b25b, ledger Entry #107); the ffi acceptance test is
  un-ignored and an injection→`SecurityRejected` test passes. Real per-token
  FFI streaming remains pending (needs detokenization; `docs/BACKLOG.md` B-24).
- **F-45 Veritas shim** — `unverified`: sealed at ledger Entry #79 but without a
  standalone test binding in the integration suite.
- **F-13 ONNX classification** — remains `verified` but the test binding was
  re-pointed (session 2026-07-26T1930-onnxcls, ledger Entry #109, L2 audit
  PASS): `OnnxClassifier` now performs real candle-onnx inference (tokenize →
  `candle_onnx::simple_eval` → deterministic logits selection → softmax/argmax),
  replacing the fail-loud stub. The real behavior-asserting coverage is the
  `classifier.rs` `logits_to_classification` unit tests (3 CI-runnable, synthetic
  logits, no fixture) plus one fixture-gated e2e that skips when the model is
  absent. `tests/tier2_onnx_classification_test.rs` is simulation-only and does
  NOT provide the classification assurance, so the Test-path column now cites
  `classifier.rs`. Follow-ups: real subword tokenizer (`docs/BACKLOG.md`) and
  manifest-driven embedder-vs-classifier auto-dispatch.
