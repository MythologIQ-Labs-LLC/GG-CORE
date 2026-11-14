# Core Runtime Automation Scripts

> **Historical script guidance, not a qualified current Windows build matrix.** Check script contents and prerequisites against the pinned source and root README before running. The hardcoded LLVM/bindgen compatibility assertions and workaround below are not verified as universal restrictions. GG-CORE is standalone upstream; CoreForge is only a downstream consumer.

This directory contains automation scripts for building, testing, and validating GG-CORE.

## Available Scripts

### 1. download_models.ps1

**Purpose:** Download test models for GGUF and ONNX backends

**Usage:**

```powershell
.\download_models.ps1
```

**What it does:**

- Downloads phi3-mini-q4km.gguf (~2.3 GB) for GGUF backend
- Provides instructions for downloading ONNX models from Hugging Face
- Validates model file integrity after download

**Models downloaded:**

- GGUF: phi3-mini-q4km.gguf
- ONNX: tinybert-classifier.onnx, minilm-embedder.onnx (manual download required)

**Output directory:** `core-runtime/fixtures/models/`

---

### 2. build_gguf.ps1

**Purpose:** Build the GGUF backend with proper environment variable setup

**Usage:**

```powershell
.\build_gguf.ps1
```

**What it does:**

- Validates LLVM installation
- Checks for required libclang.dll file
- Sets LIBCLANG_PATH environment variable
- Adds LLVM to PATH if needed
- Builds project with `--features gguf`
- Reports build success/failure with clear messaging

**Requirements:**

- LLVM 15.0.7 installed at `C:\Program Files\llvm15.0.7\bin`
  - **Historical note (unverified for current dependencies):** This setup pinned LLVM 15.0.7. Do not treat it as a universal version ceiling.
- libclang.dll present in LLVM bin directory
- Cargo and Rust toolchain installed
- Visual Studio 2022 (CMAKE_GENERATOR = "Visual Studio 17 2022")

**Build time:** 10-15 minutes (initial), 1-2 minutes (incremental)

---

## Troubleshooting

### GGUF Build Issues

If the GGUF build fails with bindgen errors:

**Root Cause:** bindgen v0.72.1 is incompatible with LLVM 22.1.0-rc3

1. **See troubleshooting guide:**
   - Full guide: `../../GGUF_BUILD_TROUBLESHOOTING.md`
   - 4 resolution options documented (downgrade bindgen, LLVM with DLL, pre-built bindings, alternative crate)
   - Contact information provided

2. **Historical workaround (do not apply without reproducing and validating against the current lockfile):** Downgrade bindgen to v0.69.4

   ```powershell
   # Create .cargo/config.toml
   @"
   [patch.crates-io]
   bindgen = { version = `"0.69`", git = `"https://github.com/rust-lang/rust-bindgen`", tag = `"v0.69.4`" }
   "@ | Out-File -Encoding UTF8 .cargo\config.toml

   # Clean and rebuild
   cargo clean
   cargo build --features gguf
   ```

### Model Download Issues

If model downloads fail:

1. **Check internet connectivity**
2. **Verify Hugging Face is accessible**
3. **Check disk space** (need ~3 GB for all models)
4. **Try manual download from Hugging Face**

---

## Quick Start

### Complete Tier 1 Validation

```powershell
# 1. Download models
.\download_models.ps1

# 2. Build GGUF backend
.\build_gguf.ps1

# 3. Run tests
cd ..\..
cargo test --features gguf --test integration_gguf_test

# 4. Run benchmarks
cargo bench --features gguf
```

### ONNX-Only Validation

```powershell
# 1. Build ONNX backend
cd ..\..
cargo build --features onnx --release

# 2. Run tests
cargo test --features onnx --test integration_onnx_test

# 3. Run benchmarks
cargo bench --features onnx
```

---

## Script Exit Codes

| Code  | Meaning                                 |
| ----- | --------------------------------------- |
| 0     | Success                                 |
| 1     | Error (check error message for details) |
| Other | Cargo/build error (check cargo output)  |

---

## Additional Resources

- **Setup Guide:** `../../TIER1_SETUP_GUIDE.md`
- **Execution Plan:** `../../TIER1_EXECUTION_PLAN.md`
- **Validation Summary:** `../../TIER1_VALIDATION_SUMMARY.md`
- **GGUF Troubleshooting:** `../../GGUF_BUILD_TROUBLESHOOTING.md`
- **Baseline Metrics:** `../fixtures/baselines/baseline_metrics.json`

---

**Last Updated:** 2026-02-20
**Scripts Version:** 0.8.1
