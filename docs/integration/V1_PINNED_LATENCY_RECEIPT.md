# #50: independently pinned packaged latency receipt check

Baseline: 13faee4f480889c7bee98a6a097c9bc02d76bf13. Risk L2/L3 evidence boundary, isolated draft only.

## Why
Existing packaged IPC benchmark schema 2 is a measurement artifact, not an attestation. A report's own hashes and summary cannot be used as independent approval. Bind exact report bytes to a separately approved digest, exact producer/model/binary/prompt hashes and request cohort, derive descriptive p95 from raw observations, and reject invalid or incomplete samples.

## Scoped preimplementation adversarial review
- Security: read one bounded local receipt, require separately supplied trusted pin; no credentials, networking, process execution, authority flags, automatic dispatch or registry mutation. Digest agreement does not prove that a daemon actually served the pinned model or that hardware/security/quality was qualified. Never source expected digest from the same untrusted receipt. No client input grants authority.
- Razor: small module and focused tests; existing strict JSON helpers and quantile implementation reused, no dependencies.
- Architecture: offline benchmark evidence processing only, no runtime API/schema changes. All raw timing fields used for p95 must be finite, positive and present; exact one requested cohort, count = waves * concurrency, at least 20 samples, consistent requested auth and streaming mode. Reject stale/mismatched pins, duplicate JSON keys, oversized inputs, malformed sample counts and invalid timing. Output explicitly unqualified, no raw prompt, paths or model output.
- Independent review and exact-head tests required before integration. Existing host/runtime evidence and dispatch revalidation remain separate. Do not modify Claude's branches or claim #50 closure.
