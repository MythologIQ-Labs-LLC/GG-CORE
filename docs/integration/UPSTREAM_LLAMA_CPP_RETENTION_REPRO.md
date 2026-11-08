# Draft upstream report: committed memory retained after `llama_init_from_model` fails at compute-buffer reservation

**Status: DRAFT, not filed.** Filing upstream (`ggml-org/llama.cpp`, and/or `utilityai/llama-cpp-rs` for the vendored copy) is an outward-facing action that awaits owner approval.

**Scope of the GG-CORE mitigation:** GG-CORE #176 prevents GG-CORE from *reaching* this path by refusing requests whose metadata-derived working memory does not fit, before any native allocation. **That does not fix the upstream behaviour**, which still occurs whenever a caller lets llama.cpp hit an allocation limit at the same stage.

The report is redacted: it contains no prompts, tokens, hostnames, user paths or credentials. The prompt content is irrelevant to the failure; any prompt reaches it.

## Affected build
- **Crate:** `llama-cpp-sys-2` **0.1.161** from crates.io (`.crate` sha256 `e9bea77fde41bbc15a278891cd5edd2ba07c5260d162614b41080ba1f100b39b`), consumed through `llama-cpp-2` 0.1.161.
- **Vendored llama.cpp:**
  - `BUILD_NUMBER` is 0 in the vendored tree (no `.git` metadata).
  - Source identity: `src/llama-context.cpp` sha256 `904c41cb688900936db135159f37c1b9803bdb79ce04a25c326400078a82532f`; `src/llama-kv-cache.cpp` `4ac534956aa0b384a9a707c778527a4e7cd4ebfbea9c895e531b0935485de3c8`.
- **Platform:**
  - Windows Server 2025 x64 (offline Hyper-V guest, 8 vCPU, 12 GiB), CPU backend only (`n_gpu_layers = 0`), MSVC toolchain.
  - The process runs inside a Windows Job Object with `JOB_OBJECT_LIMIT_JOB_MEMORY` (a commit limit). Any limit that makes a large `_aligned_malloc` fail reproduces it.

## Reproduction
1. **Model:** `SmolLM2-1.7B-Instruct-Q4_K_M.gguf`, sha256 `77665ea4815999596525c636fbeb56ba8b080b46ae85efef4f0d986a139834d7`, 1,055,609,824 bytes (from `bartowski/SmolLM2-1.7B-Instruct-GGUF` at revision `72da00985aa7cb57cd436a6172be99c3bd906d9c`). A second small model is co-resident: Qwen2.5-0.5B-Instruct Q4_K_M.
2. **Cap:** set the process commit cap so that loading the weights and allocating the KV cache succeed, but the compute buffer does not. Reproduced at a **1,740,000,000-byte** job memory limit with about 872 MB committed after model load.
3. **Context:** `llama_init_from_model` with `n_ctx = 4096` (f16 KV, `n_ubatch` 512, flash attention auto).
   - The KV buffer allocates successfully: `llama_kv_cache: CPU KV buffer size = 768.00 MiB`.
   - Then: `ggml_backend_cpu_buffer_type_alloc_buffer: failed to allocate buffer of size 125849600`, then `graph_reserve: failed to allocate compute buffers`, then `llama_init_from_model: failed to initialize the context: failed to allocate compute pp buffers`.
   - The function returns NULL.
4. **Observation** (process private bytes / commit, sampled externally):

   | Point | Private bytes |
   |---|---|
   | Before the failing call | 871–872 MB |
   | After it returns NULL | **1,716–1,717 MB** (+845 MB, about the 768 MiB KV buffer plus overhead) |
   | After 3 more identical attempts | unchanged (each fails earlier, allocating the 805,306,368-byte KV buffer) |
   | After `llama_model_free` of SmolLM2 | **936–937 MB** (idle baseline about 92 MB, so **about 845 MB still committed**) |
   | Same model unloaded after a *successful* context | returns to baseline (about 92 MB) |

5. **Control:** when the failure happens at the **KV** allocation instead (a smaller cap), nothing is retained.

## Why this looks like a leak, not allocator caching
- A later request for the same 805 MB KV block fails, instead of being satisfied from a freed block.
- The memory is not released by freeing the model, and it persists for the process lifetime.
- The `~llama_context` destructor log line never appears for the failed construction. (Members held by `std::unique_ptr` would still be destroyed during unwinding, which suggests some allocation on this path is not owned by an RAII member, or is owned by an object that outlives the failed constructor.)

## Suggested investigation for upstream
- Ownership of the KV-cache and scheduler buffers allocated before `sched_reserve()` throws in `llama_context::llama_context`. That covers `memory` (`llama_memory_ptr`), the per-buffer-type contexts in `llama_kv_cache::ctxs_bufs`, and the scheduler's partially reserved buffers.
- Whether any allocation made through `ggml_backend_buft_alloc_buffer` during `graph_reserve` is reachable only through the scheduler, and so leaked when construction unwinds.
