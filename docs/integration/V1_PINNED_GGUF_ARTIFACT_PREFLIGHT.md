# #50: operator-pinned GGUF artifact preflight

Status: isolated source candidate. Risk L2/L3 (artifact evidence; no live admission or auth change).
Baseline: f53bb329237ecbd4789227001135cfb62388af70.

## Why
The canonical dispatch verifies bytes against a sibling manifest when present. A substituted file and matching substituted manifest can still be internally consistent. Qualification must bind an independently approved artifact/manifest to the local observed file, not infer approval from consistency.

## Scope
Add an opt-in Rust library preflight in models/artifact_preflight.rs. The trusted local host supplies a previously reviewed ModelManifest, never a request payload or a manifest read from the model directory as its approval source. Require GGUF text-generation declaration, validated allowed model path, regular file, a present sibling manifest matching every parsed approved field, existing streaming hash/size verification, and existing file-identity recheck. Return an opaque non-serializable snapshot with a path-identity recheck; it is not a load or admission permit. Reuse ModelLoader, read_sibling_manifest, verify_manifest_hash and check_identity_unchanged. No new dependencies, no changes to canonical loader or IPC, no claim of license approval, benchmark quality, signed receipt, actual loaded-model identity or hardware qualification. ONNX external data/tokenizer custody and exact installed receipt/host identity are separate unresolved work. Existing path containment and writable-directory limitations remain.

## Adversarial preimplementation review
- Security: no new auth, secrets, trust flags or bypass. An independently pinned manifest is required; matching attacker-controlled file/manifest does not establish approval. Never call this with client-controlled approval input. Existing canonical admission remains mandatory. A same-inode rewrite and path races outside existing custody assumptions are not eliminated. No raw receipt deserialization or new runtime permission.
- Ghost UI: none. No new wire or UI surface.
- Razor: small dedicated module, individual functions under 40 lines, module/tests each under 250 lines, no new deep nesting.
- Dependencies: existing Rust std, existing manifest/loader/integrity code and tempfile test dependency only.
- Architecture: a narrow preflight helper on existing model module; no duplicate registry, backend or loader. Manifest equality is based on parsed known fields, not a raw sidecar byte digest. No new policy engine.
- Build path: models/mod.rs exports the helper; tests linked by cfg(test), feature index updated. Default and feature matrix run existing Rust test gates.
- Negative controls: absent manifest, substituted bytes+matching unapproved manifest, unchanged weights with altered metadata, corrupted bytes, wrong declared size, wrong architecture, disallowed path, nonregular file, replacement after verification. Positive control uses synthetic bytes only.

Scoped source design review: acceptable for isolated draft authoring, not a formal independent security verdict or promotion. Follow the existing per-PR plan/audit record convention (ledger #206); obtain required repository governance review before merge and do not edit the shared ledger concurrently. Native installed qualification, receipt authentication and canonical dispatch binding remain HOLD.
