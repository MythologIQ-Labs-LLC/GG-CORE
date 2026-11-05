# GG-CORE packaged IPC latency baseline (experimental)

This is the first **measurement-tool** component for [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136). It is *not* a completed performance qualification or release gate.

The tool measures end-to-end **request response time** and, in streaming mode, time until the first nonempty **sanitized text** arrives. It makes authenticated V1 requests to a daemon that the operator has already started with the shipping model and security settings. It never installs, loads, unloads, or changes a model, disables a security control, or starts the daemon.

## Run against an approved installed package

Use Python 3.10+ on the test host. Create a non-sensitive UTF-8 prompt fixture and record the shipping model and executable paths. Set `CORE_AUTH_TOKEN` in your environment without displaying or logging it. **The script will not accept a token on the command line.**

```bash
python3 scripts/bench_packaged_ipc.py \
  --socket "$GG_CORE_SOCKET_PATH" \
  --model-id qwen \
  --model-file /approved/models/qwen.gguf \
  --daemon-binary /approved/bin/gg-core-cli \
  --producer-sha FULL_40_CHARACTER_COMMIT_SHA \
  --prompt-file /approved/fixtures/ordinary-prompt.txt \
  --stream --max-tokens 64 --timeout-ms 120000 \
  --concurrent 1 4 8 --waves 10 \
  --output /approved/measurements/gg-core-stream.json
```

Use the Windows named-pipe path with `--socket`, e.g. `\\.\pipe\gg-core-...`, and the installed `gg-core-cli.exe`. No external Python packages are required. A Windows runner must verify the named-pipe path; local fixture tests execute Unix sockets only.

Each concurrency count receives the specified number of *waves* (10 waves × 4 requests = 40 samples in the concurrency-4 cohort). Each client authenticates independently. Concurrent requests in a wave wait on a barrier **after handshake** so the recorded request start times are close. Reports store p50/p95/p99 of request latency, p50/p95/p99 of first sanitized text latency in streaming mode, every sample's timings, and SHA-256 hashes of the supplied executable, GGUF and prompt fixture. They never store token secrets, prompt text, or generated text. Streaming raw-token-ID exposure fails closed.

The output JSON names the model, hardware/OS, declared producer SHA, and benchmark options. Schema v2 separately records the client handshake duration (`handshake_ms`), measured warmup request/response duration (`warmup_request_ms`), and daemon-reported warmup inference duration (`daemon_reported_warmup_ms`). These are distinct timings and do not include model load time. **The script does not attest that the running daemon is identical to the supplied executable or that the loaded model is identical to the supplied GGUF file.** Record the actual process artifact, installation manifests, sandbox configuration, model-load receipt, auth mode, thread affinity, `GG_CORE_N_THREADS` and `GG_CORE_N_CTX` alongside the JSON. Only use comparable results when those facts match.

## Metrics this script does NOT supply

- **Native TTFT, generation tokens/s and token-by-token inter-token latency**: GG-CORE exposes sanitized text over IPC and intentionally does not expose raw token IDs. First-sanitized-text latency includes sanitization holdback and must not be renamed TTFT. Character counts cannot be converted reliably into token counts.
- **Model loading time, RSS and memory reclamation**: instrument separately on the actual process.
- **Security parity, egress redaction validation or sandbox evidence**: use dedicated verified fixtures, negative tests and OS receipts. Running with security disabled is diagnostic only and cannot establish a shipping result.
- **Cross-runtime superiority**: no `llama.cpp`/`mistral.rs` like-for-like comparison is implemented here.
- **Production performance gate**: no acceptance thresholds should be enabled before variance has been measured across enough host runs.

This intentionally avoids inventing benchmark numbers. The historical throughput and comparative multipliers in `docs/BENCHMARKS.md` are not substitutes for a reproducible real-model measurement under the shipping configuration.

## Developer validation

```bash
python3 -m unittest discover -s scripts -p 'test_bench_packaged_ipc.py' -v
```

The mock IPC server tests framing, concurrent requests, authentication failures, raw token ID refusal, provenance hashing and the distinction between non-stream latency and first-sanitized-text latency. They do **not** qualify Windows named pipes or model speed. Real performance claims require completed archived JSON, exact SHA, approved model, and recorded security configuration.