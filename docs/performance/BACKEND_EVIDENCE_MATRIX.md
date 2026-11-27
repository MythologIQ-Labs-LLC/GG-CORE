# Backend evidence matrix (experimental)

This is a **model-free presentation step** for GG-CORE issue #196. It does not run any inference, attest models, or choose a winning backend. The executable source is in scripts/summarize_backend_bench.py.

## Read validated receipts

Run from the repository root, using Python 3.10+ and a JSONL receipt file satisfying scripts/validate_backend_bench.py:

    python3 scripts/validate_backend_bench.py /approved/results/backend-runs.jsonl
    python3 scripts/summarize_backend_bench.py /approved/results/backend-runs.jsonl > /approved/results/backend-matrix.json

The command emits one row for each of the nine backend/task combinations (GGUF, ONNX, Hologram × generation, embedding, classification). Each supplied run stays separate even if another run has the same backend. For each measured run, the summary includes the **reported** number of raw timing samples, their minimum, median and maximum, and generation decode throughput only for generation. All unavailable cells are left empty rather than fabricated. Unsupported/blocked/unmeasured rows retain their declared status and the fact that a reason was supplied, without copying free-text content.

**No cross-backend ranking is computed.** Matching task names, input fixture hashes or source metadata alone cannot demonstrate equal weights, tokenizer, quantization, sampling, hardware, operating conditions, output correctness, security configuration, or runtime boundary. Any cross-backend comparison requires independently reproduced equivalence and correctness receipts under #196, not just schema-valid JSON.

Validation refuses malformed JSON, duplicate keys, incorrect enum types, missing required provenance, invalid numeric values, or any invalid row. The assembler fails closed and writes no matrix on stdout if any input is invalid. Its bounds are 2 MiB per line, 32 MiB total, and 2,000 receipts. It does not echo prompt text, generated text, tokens or other unexpected input properties.

**Important:** The standalone packaged IPC latency tool at scripts/bench_packaged_ipc.py produces a different schema. Its first-sanitized-text latency is not native TTFT, and it has no independently verified generation tokens/s or model/output correctness. Do not relabel it as a verified three-backend receipt or silently convert it for this matrix. Preserve the original measured artifact unchanged.

## Local checks

    python3 -W error::ResourceWarning -m unittest discover -s scripts -p 'test_summarize_backend_bench.py' -v

The tests are model-free and use synthetic timing numbers **solely for regression tests**, never as runtime benchmark results. Real CPU/GPU results, provenance, and v1 approval remain separately gated.
