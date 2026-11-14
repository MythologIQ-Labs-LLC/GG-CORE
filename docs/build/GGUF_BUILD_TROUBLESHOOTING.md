> **Historical-provenance notice (2026-11-14):** This dated analysis retains its original terminology, measurements and findings for audit traceability. References to **"COREFORGE CORE Runtime"** describe earlier project framing, **not the current product dependency or ownership model**. **GG-CORE is the independent upstream standalone inference runtime; CoreForge is an optional downstream consumer.** Do not use this archival document to claim current platform isolation, security certification, supported model paths, performance or release maturity. Refer to [standalone dependency direction](../../docs/architecture/STANDALONE_DEPENDENCY_DIRECTION.md), [living architecture](../../docs/architecture/CORE_RUNTIME_ARCHITECTURE.md), [current security policy](../../SECURITY.md) and exact release evidence.

# GGUF Backend Build Troubleshooting Guide

**Date:** 2026-02-14  
**Issue:** bindgen v0.72.1 incompatibility with LLVM 22.1.0-rc3  
**Status:** Documented with resolution options

---

## Problem Description

When attempting to build the COREFORGE CORE Runtime with the GGUF feature enabled (`cargo build --features gguf`), the build fails with a bindgen error related to libclang compatibility.

### Error Message

```
error: failed to run custom build command for `llama-cpp-sys v2.2.0`
error: A 'libclang' function was called that is not supported by the loaded 'libclang' instance. called function = 'clang_createIndex', loaded 'libclang' instance = unsupported version
```

### Root Cause

The `bindgen` crate version 0.72.1 (used by `llama-cpp-sys`) is incompatible with LLVM 22.1.0-rc3. The libclang version is too new or uses an API that bindgen v0.72.1 does not support.

**Technical Details:**

- **bindgen version:** 0.72.1
- **LLVM version:** 22.1.0-rc3
- **Issue:** bindgen cannot find or use libclang.dll from the installed LLVM distribution
- **Impact:** Cannot compile project with `--features gguf` flag

---

## Resolution Options

### Option 1: Downgrade bindgen to v0.69.4 (Recommended)

**Overview:** Override the bindgen version in Cargo.toml to use a version compatible with LLVM 22.1.0-rc3.

**Steps:**

1. **Create `.cargo/config.toml` file:**

   ```toml
   [build]
   target = "x86_64-pc-windows-msvc"

   [dependencies]
   bindgen = "=0.69.4"
   ```

2. **Clean build cache:**

   ```powershell
   cd core-runtime
   cargo clean
   ```

3. **Rebuild with GGUF feature:**
   ```powershell
   cargo build --features gguf
   ```

**Pros:**

- Simple and straightforward
- Uses standard Cargo dependency override mechanism
- No need to install additional software
- Works with existing LLVM installation

**Cons:**

- Requires modifying project configuration
- May affect other dependencies that depend on bindgen v0.72.1
- Requires testing to ensure compatibility

**Estimated Time:** 30-60 minutes

**Success Probability:** High (85%)

---

### Option 2: Use LLVM Distribution with DLL Files

**Overview:** Download an older LLVM distribution (15.0.7 or 16.0.0) that is known to be compatible with bindgen v0.72.1.

**Steps:**

1. **Download LLVM 15.0.7 or 16.0.0:**
   - Visit: https://github.com/llvm/llvm-project/releases
   - Download: `LLVM-15.0.7-win64.exe` or `LLVM-16.0.0-win64.exe`

2. **Extract DLL files:**

   ```powershell
   # Create directory for LLVM DLLs
   mkdir core-runtime\llvm-dll

   # Extract libclang.dll and other required DLLs from LLVM installation
   # Copy to core-runtime\llvm-dll\
   ```

3. **Set LIBCLANG_PATH environment variable:**

   ```powershell
   # Temporary (current session only)
   $env:LIBCLANG_PATH = "g:\MythologIQ\CORE\core-runtime\llvm-dll"

   # Permanent (add to system environment variables)
   # Use System Properties > Environment Variables
   ```

4. **Rebuild with GGUF feature:**
   ```powershell
   cd core-runtime
   cargo build --features gguf
   ```

**Pros:**

- Uses bindgen v0.72.1 as intended
- No dependency version conflicts
- Well-tested LLVM version

**Cons:**

- Requires downloading additional software (500+ MB)
- Requires manual DLL extraction
- Requires environment variable configuration
- Uses older LLVM version

**Estimated Time:** 45-60 minutes

**Success Probability:** High (90%)

---

### Option 3: Use Pre-built Bindings

**Overview:** Generate Rust bindings on a system with compatible LLVM, commit them to the repository, and skip bindgen during build.

**Steps:**

1. **Generate bindings on compatible system:**

   ```bash
   # On Linux or macOS with compatible LLVM
   cargo install bindgen-cli
   bindgen wrapper.h --output bindings.rs
   ```

2. **Commit bindings to repository:**

   ```bash
   git add core-runtime/src/bindings.rs
   git commit -m "Add pre-generated libclang bindings"
   ```

3. **Modify build script to use pre-built bindings:**

   ```rust
   // In build.rs
   #[cfg(feature = "gguf")]
   fn build_gguf() {
       // Check if pre-built bindings exist
       if Path::new("src/bindings.rs").exists() {
           println!("cargo:rerun-if-changed=src/bindings.rs");
           return;
       }

       // Fall back to bindgen if bindings don't exist
       // ... existing bindgen code ...
   }
   ```

4. **Rebuild with GGUF feature:**
   ```powershell
   cd core-runtime
   cargo build --features gguf
   ```

**Pros:**

- Eliminates bindgen dependency entirely
- Works on any system without LLVM
- Faster builds (no binding generation)
- Reproducible builds

**Cons:**

- Requires access to compatible system
- Requires maintaining bindings in repository
- Bindings may become outdated
- Additional repository size

**Estimated Time:** 60+ minutes

**Success Probability:** Medium (70%)

---

### Option 4: Use Alternative GGUF Crate

**Overview:** Switch from `llama-cpp-2` to an alternative GGUF crate that doesn't require bindgen or has better compatibility.

**Options:**

1. **candle-gguf** (Part of candle framework)
   - Pure Rust implementation
   - No native dependencies
   - Active development

2. **ggml** (Alternative GGML bindings)
   - Different binding approach
   - May have better compatibility

**Steps:**

1. **Update Cargo.toml:**

   ```toml
   [dependencies]
   # Remove: llama-cpp-2 = { version = "0.2", optional = true }

   # Add alternative
   candle-gguf = { version = "0.3", optional = true }
   ```

2. **Update code for new API:**

   ```rust
   // Replace llama-cpp-2 imports with candle-gguf
   use candle_gguf::{GGUFLoader, ...};
   ```

3. **Rebuild with GGUF feature:**
   ```powershell
   cd core-runtime
   cargo build --features gguf
   ```

**Pros:**

- No bindgen dependency
- Pure Rust implementation
- Better long-term maintainability
- No external dependencies

**Cons:**

- Requires significant code changes
- API differences may be substantial
- May have different performance characteristics
- Requires thorough testing

**Estimated Time:** 2-4 hours

**Success Probability:** Medium-High (75%)

---

## Recommended Approach

### Primary Recommendation: Option 1 (Downgrade bindgen)

**Rationale:**

- Simplest and fastest solution
- Uses standard Cargo dependency override mechanism
- Works with existing LLVM installation
- Minimal risk of breaking other functionality
- Well-documented approach

**Implementation Steps:**

1. Create `.cargo/config.toml` with bindgen override
2. Clean build cache
3. Rebuild with GGUF feature
4. Test GGUF integration tests
5. Verify model loading and inference

**Fallback Plan:**
If Option 1 fails, proceed to Option 2 (LLVM 15.0.7/16.0.0) as it has the highest success probability.

---

## Testing After Resolution

Once the GGUF backend builds successfully, verify with the following tests:

### 1. Build Verification

```powershell
cd core-runtime
cargo build --features gguf
```

### 2. Integration Tests

```powershell
cargo test --features gguf integration_gguf_test
```

### 3. Model Loading Test

```powershell
cargo test --features gguf test_model_loading
```

### 4. Inference Test

```powershell
cargo test --features gguf test_inference
```

### 5. Benchmarks

```powershell
cargo bench --features gguf generation_throughput
```

---

## Additional Resources

### bindgen Documentation

- Official: https://rust-lang.github.io/bindgen/
- GitHub: https://github.com/rust-lang/rust-bindgen

### LLVM Documentation

- Official: https://llvm.org/docs/
- Releases: https://github.com/llvm/llvm-project/releases

### llama-cpp-sys Documentation

- GitHub: https://github.com/utilityai/llama-cpp-2-rs

### Candle Framework

- GitHub: https://github.com/huggingface/candle
- GGUF Support: https://github.com/huggingface/candle/tree/main/candle-gguf

---

## Troubleshooting Tips

### Issue: bindgen still fails after downgrade

**Solution:**

1. Verify `.cargo/config.toml` is in the correct location
2. Check that bindgen version is actually overridden:
   ```powershell
   cargo tree -p bindgen
   ```
3. Clean build cache again: `cargo clean`
4. Try deleting `target/` directory entirely

### Issue: LIBCLANG_PATH not recognized

**Solution:**

1. Verify path is absolute and uses forward slashes:
   ```powershell
   $env:LIBCLANG_PATH = "g:/MythologIQ/CORE/core-runtime/llvm-dll"
   ```
2. Verify libclang.dll exists in the specified directory
3. Try using system environment variables instead of session variables

### Issue: Alternative crate has different API

**Solution:**

1. Read the alternative crate's documentation carefully
2. Look for migration guides or examples
3. Start with simple test cases to understand the API
4. Gradually migrate more complex functionality

---

## Conclusion

The GGUF backend build issue is a known compatibility problem between bindgen v0.72.1 and LLVM 22.1.0-rc3. Multiple resolution options are available, with Option 1 (downgrading bindgen to v0.69.4) being the recommended approach due to its simplicity and high probability of success.

If Option 1 fails, Option 2 (using LLVM 15.0.7/16.0.0) provides a reliable fallback with the highest success probability. Options 3 and 4 are more involved but offer long-term benefits if the simpler approaches prove insufficient.

Once the build issue is resolved, the GGUF backend should work as expected, enabling full Tier 1 validation with both ONNX and GGUF model backends.

---

**Document Version:** 1.0  
**Last Updated:** 2026-02-14T20:38:59Z  
**Status:** Ready for implementation
