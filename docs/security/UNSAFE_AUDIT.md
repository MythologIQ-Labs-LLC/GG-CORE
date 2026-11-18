# Unsafe Code Audit Report (Historical Snapshot)

> **CURRENT SECURITY STATUS (2026-11-17): HISTORICAL AUDIT SUPERSEDED FOR MEMORY SAFETY.**
> This document records a February 2026 point-in-time audit. Its "High: 0" count and
> "No High-Risk Issues Found" conclusion are **not valid as claims about current main**.
> The active standalone GG-CORE source at `main@79a16fbeddebd3b4df3983ccbb3027d8fe0c94d3`
> contains a confirmed unsafe ownership pattern in
> `core-runtime/src/scheduler/worker_streaming.rs::run_stream`: references to
> `InferenceEngine` and `SecurityPipeline` are cast to integer addresses, then
> reconstructed with `unsafe` inside `tokio::task::spawn_blocking`. Dropping
> the waiting async future does not guarantee the native blocking closure has
> terminated. The independent source review supports use-after-free risk, and
> the owner reports a symbolized Windows WER `0xc0000005` crash through
> `SecurityPipeline::sanitize_output`. See **[P0 #219](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/219)**.
>
> The awaiting async frame also holds the `ResourceGuard`; dropping it early
> can decrement admission counters while native compute persists. This second
> risk is source-derived and requires an explicit ownership/reclamation test.
> Claude is implementing a runtime correction; **do not treat an unmerged
> fix, source CI, or this notice as a completed remediation**. Native Windows
> stream/drop/cancel, memory/lifecycle and exact candidate tests remain required.
> GG-CORE is upstream of CoreForge, not a CoreForge subcomponent. The historical
> original classification, inventory and observations below are retained solely
> as an auditable record and must not be represented as fresh current evidence.


**Project:** COREFORGE CORE Runtime
**Version:** 1.0.0
**Last Updated:** 2026-02-18
**Risk Grade:** L3 (Security-Critical)
**Classification:** Internal / Security Audit

---

## 1. Executive Summary

This document provides a comprehensive audit of all `unsafe` blocks in the COREFORGE CORE Runtime codebase. Each unsafe block is documented with its safety invariants, justification, and risk assessment.

### 1.1 Unsafe Code Statistics

| Category | Count | Risk Level |
|----------|-------|------------|
| **FFI Boundaries** | 18 | Medium |
| **Send/Sync Implementations** | 8 | Low-Medium |
| **SIMD Intrinsics** | 14 | Low |
| **Memory-Mapped Files** | 2 | Low |
| **Windows API Calls** | 2 | Medium |
| **Raw Pointer Operations** | 4 | Medium |
| **Total** | 48 | -- |

### 1.2 Risk Summary

| Risk Level | Count | Description |
|------------|-------|-------------|
| High | 0 | Potential memory corruption or security vulnerability |
| Medium | 22 | Requires careful invariant maintenance |
| Low | 26 | Well-understood patterns with clear safety |

---

## 2. FFI Boundary Functions

### 2.1 Runtime Lifecycle (ffi/runtime.rs)

#### core_config_default
```rust
// Line 25-31
#[no_mangle]
pub extern "C" fn core_config_default(config: *mut CoreConfig) {
    if config.is_null() {
        return;
    }
    unsafe {
        *config = CoreConfig::default();
    }
}
```

**Safety Invariants:**
1. Null pointer check performed before dereference
2. Caller must provide valid, aligned, writable memory
3. CoreConfig is a #[repr(C)] struct with no invalid states

**Risk Level:** Low
**Justification:** Standard FFI pattern with null guard

---

#### core_runtime_create
```rust
// Line 36-75
#[no_mangle]
pub unsafe extern "C" fn core_runtime_create(
    config: *const CoreConfig,
    out_runtime: *mut *mut CoreRuntime,
) -> CoreErrorCode
```

**Safety Invariants:**
1. Null checks for both config and out_runtime pointers
2. config must point to valid CoreConfig data
3. out_runtime must point to valid, writable pointer location
4. Caller takes ownership of returned CoreRuntime pointer

**Risk Level:** Medium
**Justification:** Complex ownership transfer; caller must eventually call core_runtime_destroy

---

#### core_runtime_destroy
```rust
// Line 78-92
#[no_mangle]
pub unsafe extern "C" fn core_runtime_destroy(runtime: *mut CoreRuntime) {
    if runtime.is_null() {
        return;
    }
    let rt = Box::from_raw(runtime);
    // ...
}
```

**Safety Invariants:**
1. Null check performed
2. runtime must be a valid pointer from core_runtime_create
3. runtime must not be used after this call
4. runtime must not be destroyed twice (double-free prevention)

**Risk Level:** Medium
**Justification:** Ownership reclaim; relies on caller discipline

---

### 2.2 Authentication (ffi/auth.rs)

#### core_authenticate
```rust
// Line 22-67
#[no_mangle]
pub unsafe extern "C" fn core_authenticate(
    runtime: *mut CoreRuntime,
    token: *const c_char,
    out_session: *mut *mut CoreSession,
) -> CoreErrorCode
```

**Safety Invariants:**
1. Null checks for all three pointers
2. token must be valid null-terminated C string
3. token must be valid UTF-8 (checked with CStr::to_str)
4. out_session receives owned pointer on success

**Risk Level:** Medium
**Justification:** UTF-8 validation protects against malformed input

---

#### core_session_validate
```rust
// Line 70-91
#[no_mangle]
pub unsafe extern "C" fn core_session_validate(
    runtime: *mut CoreRuntime,
    session: *mut CoreSession,
) -> CoreErrorCode
```

**Safety Invariants:**
1. Null checks for both pointers
2. session must be valid pointer from core_authenticate
3. Borrows session (does not take ownership)

**Risk Level:** Low
**Justification:** Simple validation with no ownership changes

---

#### core_session_release
```rust
// Line 94-99
#[no_mangle]
pub unsafe extern "C" fn core_session_release(session: *mut CoreSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}
```

**Safety Invariants:**
1. Null check performed
2. session must be valid pointer from core_authenticate
3. session must not be used after this call
4. Must not be called twice on same pointer

**Risk Level:** Medium
**Justification:** Ownership reclaim with double-free risk

---

#### core_session_id
```rust
// Line 102-108
#[no_mangle]
pub unsafe extern "C" fn core_session_id(session: *const CoreSession) -> *const c_char {
    if session.is_null() {
        return std::ptr::null();
    }
    (*session).session_id_cstr.as_ptr()
}
```

**Safety Invariants:**
1. Null check with null return on invalid input
2. Returned pointer valid only while session is valid
3. Caller must not modify returned string

**Risk Level:** Low
**Justification:** Read-only borrowed pointer access

---

### 2.3 Model Operations (ffi/models.rs)

#### core_model_load
```rust
// Line 14-55
#[no_mangle]
pub unsafe extern "C" fn core_model_load(
    runtime: *mut CoreRuntime,
    model_path: *const c_char,
    out_handle_id: *mut u64,
) -> CoreErrorCode
```

**Safety Invariants:**
1. Null checks for all three pointers
2. model_path must be valid null-terminated C string
3. Path validation prevents directory traversal
4. out_handle_id receives model handle on success

**Risk Level:** Medium
**Justification:** Path validation is critical for filesystem security

---

#### core_model_unload
```rust
// Line 59-82
#[no_mangle]
pub unsafe extern "C" fn core_model_unload(
    runtime: *mut CoreRuntime,
    handle_id: u64,
) -> CoreErrorCode
```

**Safety Invariants:**
1. Null check for runtime
2. handle_id validated by model registry

**Risk Level:** Low
**Justification:** Handle validation prevents invalid operations

---

#### core_free_model_metadata
```rust
// Line 120-128
#[no_mangle]
pub unsafe extern "C" fn core_free_model_metadata(metadata: *mut CoreModelMetadata) {
    if !metadata.is_null() {
        let m = &mut *metadata;
        if !m.name.is_null() {
            drop(CString::from_raw(m.name as *mut c_char));
            m.name = std::ptr::null();
        }
    }
}
```

**Safety Invariants:**
1. Null checks for struct and nested pointer
2. name must have been allocated by CString
3. Sets name to null after free (prevents double-free)

**Risk Level:** Medium
**Justification:** Proper null-setting prevents use-after-free

---

### 2.4 Inference (ffi/inference.rs)

#### core_infer
```rust
// Line 17-79
#[no_mangle]
pub unsafe extern "C" fn core_infer(
    runtime: *mut CoreRuntime,
    session: *mut CoreSession,
    model_id: *const c_char,
    prompt_tokens: *const u32,
    prompt_token_count: u32,
    params: *const CoreInferenceParams,
    out_tokens: *mut *mut u32,
    out_token_count: *mut u32,
) -> CoreErrorCode
```

**Safety Invariants:**
1. Null checks for all required pointers
2. prompt_tokens must be valid array of prompt_token_count u32 values
3. out_tokens receives allocated array on success (caller must free)
4. Session validation ensures authenticated access

**Risk Level:** Medium
**Justification:** Array bounds from untrusted input; validated by slice construction

---

#### core_free_tokens
```rust
// Line 115-119
#[no_mangle]
pub unsafe extern "C" fn core_free_tokens(tokens: *mut u32, count: u32) {
    if !tokens.is_null() && count > 0 {
        let _ = Vec::from_raw_parts(tokens, count as usize, count as usize);
    }
}
```

**Safety Invariants:**
1. Null check performed
2. count must match allocated size exactly
3. tokens must have been allocated by core_infer

**Risk Level:** Medium
**Justification:** Capacity must match allocation; mismatch causes UB

---

### 2.5 Streaming (ffi/streaming.rs)

#### CallbackInvoker unsafe impl
```rust
// Line 34-36
// SAFETY: user_data pointer is provided by caller who ensures thread safety
unsafe impl Send for CallbackInvoker {}
unsafe impl Sync for CallbackInvoker {}
```

**Safety Invariants:**
1. user_data pointer must be thread-safe (caller responsibility)
2. callback function must be safe to call from any thread
3. cancelled flag uses atomic operations

**Risk Level:** Medium
**Justification:** Relies on external code contract

---

#### core_infer_streaming
```rust
// Line 64-133
#[no_mangle]
pub unsafe extern "C" fn core_infer_streaming(
    runtime: *mut CoreRuntime,
    session: *mut CoreSession,
    model_id: *const c_char,
    prompt_tokens: *const u32,
    prompt_token_count: u32,
    params: *const CoreInferenceParams,
    callback: CoreStreamCallback,
    user_data: *mut c_void,
) -> CoreErrorCode
```

**Safety Invariants:**
1. All pointer arguments validated
2. callback must be valid function pointer
3. user_data ownership/lifetime managed by caller
4. callback invoked with valid arguments

**Risk Level:** Medium
**Justification:** Callback execution crosses FFI boundary

---

#### core_free_string
```rust
// Line 164-168
#[no_mangle]
pub unsafe extern "C" fn core_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
```

**Safety Invariants:**
1. Null check performed
2. s must have been allocated by CString in Rust code
3. Must not be called twice on same pointer

**Risk Level:** Medium
**Justification:** Standard CString deallocation pattern

---

### 2.6 Health Check (ffi/health.rs)

#### core_health_check, core_is_alive, core_is_ready, core_get_metrics_json
```rust
// Multiple functions with similar patterns
#[no_mangle]
pub unsafe extern "C" fn core_health_check(...)
pub unsafe extern "C" fn core_is_alive(...)
pub unsafe extern "C" fn core_is_ready(...)
pub unsafe extern "C" fn core_get_metrics_json(...)
```

**Safety Invariants:**
1. Null checks for all pointers
2. Read-only access to runtime state
3. No ownership transfer for runtime pointer

**Risk Level:** Low
**Justification:** Simple read operations with proper guards

---

## 3. Send/Sync Implementations

### 3.1 Arena Allocator (memory/arena.rs)

```rust
// Line 18-21
// SAFETY: Arena uses atomic operations for thread-safe allocation.
// The compare_exchange loop ensures unique allocations per thread.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}
```

**Safety Invariants:**
1. offset field uses AtomicUsize with proper ordering
2. compare_exchange_weak ensures unique allocation per thread
3. No mutable aliasing of allocated regions
4. reset() requires external synchronization (documented)

**Risk Level:** Medium
**Justification:** Atomic operations ensure thread-safe allocation

---

### 3.2 ArenaSlice Methods (memory/arena.rs)

```rust
// Line 93-102
pub fn as_slice(&self) -> &[T] {
    // SAFETY: ptr is valid for len elements, lifetime tied to arena
    unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
}

pub fn as_mut_slice(&mut self) -> &mut [T] {
    // SAFETY: ptr is valid for len elements, we have exclusive access
    unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
}
```

**Safety Invariants:**
1. ptr allocated from Arena with correct size and alignment
2. len matches allocation size
3. &mut requires exclusive access (enforced by &mut self)
4. Lifetime bound to Arena prevents use-after-free

**Risk Level:** Low
**Justification:** PhantomData lifetime marker enforces bounds

---

### 3.3 MappedModel (models/loader.rs)

```rust
// Line 108-111
// SAFETY: Mmap is Send+Sync when underlying file is read-only and not modified.
// We only use read-only mappings and models are immutable during inference.
unsafe impl Send for MappedModel {}
unsafe impl Sync for MappedModel {}
```

**Safety Invariants:**
1. File opened with read-only mode
2. No modification of mapped region
3. Model files not modified during runtime operation

**Risk Level:** Low
**Justification:** Read-only memory mapping is inherently thread-safe

---

```rust
// Line 115-119
pub fn open(path: &ModelPath) -> Result<Self, LoadError> {
    let file = File::open(path.as_path())?;
    // SAFETY: File is opened read-only, model files are not modified during runtime
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(Self { mmap })
}
```

**Safety Invariants:**
1. File opened read-only (implicit in File::open)
2. ModelPath validated by ModelLoader
3. File must not be modified while mapped

**Risk Level:** Low
**Justification:** Standard memory-mapping with documented constraints

---

### 3.4 CallbackInvoker (ffi/streaming.rs)

```rust
// Line 34-36
// SAFETY: user_data pointer is provided by caller who ensures thread safety
unsafe impl Send for CallbackInvoker {}
unsafe impl Sync for CallbackInvoker {}
```

**Safety Invariants:**
1. Caller ensures user_data is thread-safe
2. callback function is thread-safe
3. cancelled uses atomic operations

**Risk Level:** Medium
**Justification:** External code contract; documented in API

---

### 3.5 GpuMemory (engine/gpu.rs)

```rust
// Line 355-357
// Safety: GpuMemory can be sent between threads
unsafe impl Send for GpuMemory {}
unsafe impl Sync for GpuMemory {}
```

**Safety Invariants:**
1. GPU operations are thread-safe via driver
2. No aliased mutable access
3. Device context properly managed

**Risk Level:** Low
**Justification:** GPU APIs designed for multi-threaded use

---

## 4. SIMD Intrinsics

### 4.1 AVX2 Kernels (engine/simd_matmul.rs)

```rust
// Line 85-103
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_q8_avx2(q_data: &[u8], input: &[f32], scale: f32) -> f32

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_q4_avx2(q_data: &[u8], input: &[f32], scale: f32) -> f32

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn load_q8_to_i32_avx2(data: &[u8]) -> __m256i

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn horizontal_sum_avx2(v: __m256) -> f32
```

**Safety Invariants:**
1. target_feature gates function to supported CPUs
2. Called only after is_x86_feature_detected!("avx2") check
3. Input slices bounds-checked before SIMD operations
4. Remainder elements processed in scalar fallback

**Risk Level:** Low
**Justification:** CPU feature detection ensures safe execution

---

### 4.2 NEON Kernels (engine/simd_neon.rs)

```rust
// Line 14-78
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn dot_q8_neon(q_data: &[u8], input: &[f32], scale: f32) -> f32

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn dot_q4_neon(q_data: &[u8], input: &[f32], scale: f32) -> f32

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn load_q8_to_i32_neon(data: &[u8]) -> int32x4_t

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn unpack_q4_to_i32_neon(data: &[u8]) -> int32x4_t

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn horizontal_sum_neon(v: float32x4_t) -> f32
```

**Safety Invariants:**
1. NEON always available on aarch64
2. target_feature annotation ensures proper code generation
3. Bounds checking on input slices

**Risk Level:** Low
**Justification:** NEON is mandatory on aarch64

---

### 4.3 SIMD Tokenizer (engine/simd_tokenizer.rs, simd_tokenizer_v2.rs)

```rust
// Line 55-118 (simd_tokenizer.rs)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_whitespace_avx2(text: &[u8]) -> Vec<usize>

// Line 108-214 (simd_tokenizer_v2.rs)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_whitespace_avx2(text: &[u8]) -> Vec<usize>

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn find_whitespace_neon(text: &[u8]) -> Vec<usize>

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_token_boundaries_avx2(text: &[u8]) -> Vec<usize>
```

**Safety Invariants:**
1. Feature detection before calling unsafe variants
2. Chunks processed with remainder handled separately
3. Output positions bounds-checked

**Risk Level:** Low
**Justification:** Standard SIMD text processing patterns

---

## 5. Windows API Calls

### 5.1 Windows Sandbox (sandbox/windows.rs)

```rust
// Line 111-156
#[cfg(target_os = "windows")]
fn apply_job_object_limits(config: &SandboxConfig) -> Result<isize, String> {
    use windows_sys::Win32::System::JobObjects::{...};

    unsafe {
        // Create a job object
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job == 0 {
            return Err("Failed to create job object".to_string());
        }

        // Configure limits
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        // ... configure limits ...

        // Apply the limits
        let result = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );

        if result == 0 {
            CloseHandle(job);
            return Err("Failed to set job object limits".to_string());
        }

        Ok(job)
    }
}
```

**Safety Invariants:**
1. Windows API calls with proper error checking
2. Resources cleaned up on failure
3. Job handle properly sized for platform
4. mem::zeroed() valid for Windows struct initialization

**Risk Level:** Medium
**Justification:** Windows API requires careful resource management

---

```rust
// Line 159-166
impl Drop for WindowsSandbox {
    fn drop(&mut self) {
        if let Some(handle) = self.job_handle {
            #[cfg(target_os = "windows")]
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}
```

**Safety Invariants:**
1. Handle valid from successful job creation
2. CloseHandle called once in Drop
3. Platform guard ensures Windows-only

**Risk Level:** Low
**Justification:** Standard RAII cleanup pattern

---

## 6. Metal GPU Operations

### 6.1 Metal Buffer (engine/metal.rs)

```rust
// Line 320-329
pub fn contents(&self) -> &[u8] {
    // Safety: Metal guarantees the buffer contents are valid for the lifetime of the buffer
    unsafe { std::slice::from_raw_parts(self.buffer.contents() as *const u8, self.size) }
}

pub fn contents_mut(&mut self) -> &mut [u8] {
    // Safety: Metal guarantees the buffer contents are valid for the lifetime of the buffer
    unsafe { std::slice::from_raw_parts_mut(self.buffer.contents() as *mut u8, self.size) }
}
```

**Safety Invariants:**
1. Metal buffer contents() returns valid pointer
2. size matches allocated buffer size
3. &mut requires exclusive access (enforced by &mut self)
4. Buffer lifetime >= slice lifetime

**Risk Level:** Low
**Justification:** Metal API guarantees documented

---

## 7. Security-Critical Observations

### 7.1 Historical conclusion, invalidated by #219

The **February 2026** audit identified no high-risk unsafe code patterns *in its then-reviewed sample*. This conclusion is **superseded for current main** by the P0 stream-worker use-after-free tracked in #219. Historical assertions retained below must be reassessed rather than taken as current assurances. The original audit asserted all unsafe blocks:
- Have documented safety invariants
- Include null pointer checks where applicable
- Use standard patterns (FFI, SIMD, memory-mapping)
- Have appropriate platform guards

### 7.2 Recommendations

1. **Add debug assertions** in ArenaSlice to validate pointer/length
2. **Document double-free prevention** in FFI release functions
3. **Consider safer FFI wrappers** (e.g., cbindgen safety annotations)
4. **Add fuzzing** for FFI boundary functions

### 7.3 Test Coverage

| Category | Unit Tests | Integration Tests |
|----------|------------|-------------------|
| FFI Functions | Yes | Manual C tests |
| Send/Sync | Yes | Concurrent tests |
| SIMD | Yes | Benchmark suite |
| Memory-mapped | Yes | File tests |
| Windows API | Limited | Platform tests |

---

## 8. Document Control

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0.0 | 2026-02-18 | GUARDIAN (Security Engineer) | Initial unsafe code audit |

---

## Appendix A: Quick Reference

### Unsafe Code Locations

| File | Line Range | Category |
|------|------------|----------|
| ffi/runtime.rs | 29-31, 36-75, 79-92, 99, 108 | FFI |
| ffi/auth.rs | 23-108 | FFI |
| ffi/models.rs | 14-157 | FFI |
| ffi/inference.rs | 17-119 | FFI |
| ffi/streaming.rs | 35-36, 50-52, 64-133, 164-168 | FFI |
| ffi/health.rs | 15-97 | FFI |
| memory/arena.rs | 20-21, 95, 101 | Memory |
| models/loader.rs | 110-111, 118 | Memory-mapped |
| sandbox/windows.rs | 111-156, 163-165 | Windows API |
| engine/gpu.rs | 356-357 | Send/Sync |
| engine/metal.rs | 322, 328 | GPU buffer |
| engine/simd_matmul.rs | 49, 54, 67, 72, 85-141 | SIMD |
| engine/simd_neon.rs | 14-78 | SIMD |
| engine/simd_tokenizer.rs | 55-118, 114 | SIMD |
| engine/simd_tokenizer_v2.rs | 108-273 | SIMD |

### Safety Documentation Checklist

- [x] All unsafe blocks have // SAFETY: comments
- [x] Send/Sync implementations have documented invariants
- [x] FFI functions document null pointer handling
- [x] SIMD functions use target_feature gates
- [x] Platform-specific code uses cfg guards
