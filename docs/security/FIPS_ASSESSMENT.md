> **Historical-provenance notice (2026-11-14):** This dated analysis retains its original terminology, measurements and findings for audit traceability. References to **"COREFORGE CORE Runtime"** describe earlier project framing, **not the current product dependency or ownership model**. **GG-CORE is the independent upstream standalone inference runtime; CoreForge is an optional downstream consumer.** Do not use this archival document to claim current platform isolation, security certification, supported model paths, performance or release maturity. Refer to [standalone dependency direction](../../docs/architecture/STANDALONE_DEPENDENCY_DIRECTION.md), [living architecture](../../docs/architecture/CORE_RUNTIME_ARCHITECTURE.md), [current security policy](../../SECURITY.md) and exact release evidence.

# FIPS Cryptographic Module Assessment

**Document:** FIPS-1 Feasibility Assessment
**Version:** 1.0
**Date:** 2026-02-18
**Classification:** Internal / Compliance Planning
**Project:** COREFORGE CORE Runtime v0.6.0

---

## Executive Summary

This document assesses the feasibility of achieving FIPS 140-2/140-3 compliance for the cryptographic operations within the COREFORGE CORE Runtime. The assessment covers the current cryptographic ecosystem, implementation options, and a cost-benefit analysis for different compliance paths.

### Key Findings

| Criterion | Current Status | FIPS Requirement |
|-----------|----------------|------------------|
| Encryption Algorithm | AES-256-GCM | NIST Approved |
| Key Derivation | PBKDF2-HMAC-SHA256 | NIST SP 800-132 Compliant |
| Random Number Generation | OS CSPRNG | Requires NIST SP 800-90A DRBG |
| Library Validation | Not Validated | Requires CMVP Certificate |
| Module Boundary | Not Defined | Required for Certification |

**Recommendation:** For current business needs, maintain Option A (Pure Rust) with documentation of NIST-aligned algorithms. Pursue FIPS validation only if government/regulated sector contracts require it.

---

## 1. Current Cryptographic Implementation

### 1.1 Algorithm Inventory

| Component | Algorithm | Crate | Standard |
|-----------|-----------|-------|----------|
| Model Encryption | AES-256-GCM | `aes-gcm 0.10` | NIST SP 800-38D |
| Key Derivation | PBKDF2-HMAC-SHA256 | `pbkdf2 0.12` | NIST SP 800-132 |
| Session Hashing | SHA-256 | `sha2 0.10` | FIPS 180-4 |
| Random Generation | OS RNG | `rand 0.8` (OsRng) | Platform-dependent |
| Constant-time Compare | XOR-based | Custom | N/A |

### 1.2 Cryptographic Parameters

```
File: core-runtime/src/security/encryption.rs
```

| Parameter | Value | FIPS Requirement | Status |
|-----------|-------|------------------|--------|
| Key Size | 256 bits | Min 128 bits | COMPLIANT |
| Nonce Size | 96 bits | As specified in SP 800-38D | COMPLIANT |
| Authentication Tag | 128 bits | Min 96 bits | COMPLIANT |
| PBKDF2 Iterations | 100,000 | No minimum specified | ACCEPTABLE |
| Salt Length | 20 bytes (hardcoded) | Min 128 bits recommended | COMPLIANT |

### 1.3 RustCrypto Ecosystem FIPS Status

The RustCrypto project (`aes-gcm`, `pbkdf2`, `sha2`) provides:

| Aspect | Status |
|--------|--------|
| Algorithm Correctness | Verified via test vectors |
| CMVP Validation | **Not Validated** |
| Security Audits | Community-reviewed, no formal audit |
| Side-channel Resistance | Constant-time implementations |
| NIST Algorithm Alignment | Yes (approved algorithms) |

**Critical Gap:** No FIPS 140-2/140-3 validated Rust cryptographic library currently exists.

---

## 2. Implementation Options Analysis

### 2.1 Option A: Pure Rust (Status Quo)

**Description:** Continue using RustCrypto crates without FIPS validation.

| Aspect | Assessment |
|--------|------------|
| **Algorithms** | NIST-approved (AES-GCM, SHA-256, PBKDF2) |
| **Validation** | None |
| **Effort** | None (already implemented) |
| **Maintenance** | Low (pure Rust, no FFI) |
| **Performance** | Excellent (hardware acceleration via AES-NI) |

**Pros:**
- Zero additional development effort
- No FFI complexity or memory safety concerns
- Full control over implementation
- Easy updates via Cargo

**Cons:**
- Cannot claim FIPS compliance
- May be rejected by government/regulated contracts
- No third-party validation certificate

**Use Cases:**
- Commercial deployments without regulatory requirements
- Internal enterprise use
- Research and development

---

### 2.2 Option B: BoringSSL FFI (FIPS Validated)

**Description:** Replace RustCrypto with bindings to Google's BoringSSL, which has FIPS 140-2 validation.

| Aspect | Assessment |
|--------|------------|
| **Algorithms** | NIST-approved via BoringCrypto module |
| **Validation** | CMVP Certificate #3678 (FIPS 140-2 Level 1) |
| **Effort** | High (FFI bindings, build complexity) |
| **Maintenance** | Medium (track BoringSSL updates) |
| **Performance** | Excellent (native C, hardware acceleration) |

**Implementation Requirements:**

1. **Build System Changes:**
   ```toml
   [dependencies]
   boring = "4.0"  # or ring (also uses BoringSSL)
   ```

2. **Code Changes:**
   - Replace `aes-gcm` with BoringSSL AES-GCM
   - Replace `pbkdf2` with BoringSSL PBKDF2
   - Replace `sha2` with BoringSSL SHA-256
   - Ensure FIPS mode is enabled at initialization

3. **Build Complexity:**
   - Requires C/C++ toolchain
   - CMake for BoringSSL compilation
   - Platform-specific builds

**Pros:**
- CMVP-validated cryptographic module
- Battle-tested by Google
- Cross-platform support

**Cons:**
- FFI introduces memory safety boundary
- Complex build dependencies
- BoringSSL validation may not cover Rust bindings
- Larger binary size (~2MB for crypto module)

**Estimated Effort:** 2-4 weeks development, ongoing maintenance

---

### 2.3 Option C: Windows CNG / macOS Security.framework

**Description:** Use operating system native cryptographic providers that have FIPS validation.

#### Windows CNG (Cryptography Next Generation)

| Aspect | Assessment |
|--------|------------|
| **Validation** | FIPS 140-2 certificates (varies by Windows version) |
| **Effort** | Medium (Windows-only FFI) |
| **Availability** | Windows 10/11, Windows Server 2016+ |

**Implementation:**
```rust
// windows-sys crate for CNG bindings
use windows_sys::Win32::Security::Cryptography::*;
```

#### macOS Security.framework

| Aspect | Assessment |
|--------|------------|
| **Validation** | Apple CoreCrypto FIPS validation |
| **Effort** | Medium (macOS-only) |
| **Availability** | macOS 10.12+ |

**Implementation:**
```rust
// security-framework crate for CommonCrypto
use security_framework::encrypt::*;
```

**Pros:**
- Leverages validated OS crypto
- No additional crypto library maintenance
- Matches platform security expectations

**Cons:**
- Platform-specific implementations required
- Different APIs per platform
- Linux requires separate solution
- Validation coverage varies by OS version

**Estimated Effort:** 4-6 weeks for multi-platform support

---

### 2.4 Option D: Commercial FIPS Module (HSM/Library)

**Description:** Integrate with a commercial FIPS-validated HSM or cryptographic library.

| Vendor | Product | FIPS Level | Integration |
|--------|---------|------------|-------------|
| SafeNet | Luna HSM | Level 3 | PKCS#11 |
| AWS | CloudHSM | Level 3 | AWS SDK |
| Fortanix | Runtime Encryption | Level 2 | Rust SDK |
| Entrust | nShield | Level 3 | PKCS#11 |

**Pros:**
- Highest assurance (Level 3 physical security)
- Key material never leaves HSM
- Audit-ready compliance

**Cons:**
- Significant cost ($10K-100K+/year)
- Network dependency (conflicts with C.O.R.E. offline principle)
- Complex deployment
- Performance overhead for HSM calls

**Assessment:** Not recommended for CORE Runtime due to offline requirement violation.

---

## 3. Cost-Benefit Analysis

### 3.1 FIPS Validation Costs

| Cost Category | Estimate | Notes |
|---------------|----------|-------|
| Laboratory Testing | $50,000 - $150,000 | Depends on module complexity |
| Documentation Prep | $20,000 - $50,000 | Security policies, design docs |
| Code Remediation | $25,000 - $100,000 | Algorithm boundary isolation |
| Re-validation (annual) | $15,000 - $30,000 | For updates and patches |
| Consulting | $10,000 - $40,000 | FIPS expert guidance |
| **Total Initial** | **$105,000 - $340,000** | |
| **Annual Ongoing** | **$25,000 - $50,000** | |

### 3.2 Timeline Estimates

| Phase | Duration |
|-------|----------|
| Module Design & Documentation | 2-4 months |
| Implementation Changes | 1-3 months |
| Internal Testing | 1-2 months |
| Laboratory Engagement | 6-12 months |
| CMVP Review | 3-6 months |
| **Total** | **13-27 months** |

### 3.3 Market Requirements Analysis

| Sector | FIPS Required | Market Size Impact |
|--------|---------------|-------------------|
| US Federal Government | Mandatory | High value, limited volume |
| US Department of Defense | Mandatory | High value, strict vetting |
| Healthcare (HIPAA) | Recommended | Large market, not mandatory |
| Financial (PCI-DSS) | Not required | Large market |
| Commercial Enterprise | Rarely required | Largest market |
| Research/Academic | Not required | Growing market |

### 3.4 Decision Matrix

| Option | Cost | Effort | Compliance | Recommendation |
|--------|------|--------|------------|----------------|
| A: Pure Rust | $0 | None | No FIPS claim | **Current** |
| B: BoringSSL FFI | $50K-100K | Medium | FIPS-adjacent | Conditional |
| C: OS Native | $75K-150K | High | FIPS-adjacent | Deferred |
| D: Full Validation | $150K-400K | Very High | Full FIPS | Future |

---

## 4. Recommendations

### 4.1 Immediate Actions (v0.6.0)

1. **Document Current Compliance Posture:**
   - Algorithms are NIST-approved
   - Implementation follows NIST guidelines
   - No FIPS module validation claim

2. **Prepare Cryptographic Module Boundary:**
   - Isolate all crypto operations in `security/encryption.rs`
   - Document entry/exit points
   - Prepare for future module extraction

3. **Security Self-Test Framework:**
   - Implement power-on self-tests (POST)
   - Known-answer tests (KAT) for algorithms
   - Enable conditional self-test mode

### 4.2 Short-Term (v0.7.0 - v1.0)

1. **Evaluate Market Demand:**
   - Track customer requests for FIPS compliance
   - Identify target government/regulated contracts
   - Assess competitive landscape

2. **Conditional BoringSSL Integration:**
   - If FIPS demand materializes, implement Option B
   - Feature flag: `--features fips-crypto`
   - Maintain pure Rust as default

### 4.3 Long-Term (Post v1.0)

1. **Full FIPS Validation (if justified):**
   - Engage NVLAP-accredited laboratory
   - Submit for CMVP certification
   - Budget $200K+ and 18+ months

2. **Alternative: FIPS-Ready Certification:**
   - Third-party attestation of algorithm alignment
   - Lower cost than full validation
   - May satisfy some compliance requirements

---

## 5. Technical Appendix

### 5.1 Current Cryptographic Module Boundary

```
┌─────────────────────────────────────────────────────────────────┐
│                    CORE Runtime Process                          │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │              Proposed Cryptographic Module Boundary          ││
│  │                                                              ││
│  │  security/encryption.rs                                      ││
│  │  ├── ModelEncryption::encrypt()      [AES-256-GCM]          ││
│  │  ├── ModelEncryption::decrypt()      [AES-256-GCM]          ││
│  │  ├── ModelEncryption::from_password() [PBKDF2-SHA256]       ││
│  │  └── generate_nonce()                [OS CSPRNG]            ││
│  │                                                              ││
│  │  ipc/auth.rs                                                 ││
│  │  ├── SessionAuth::authenticate()     [SHA-256]              ││
│  │  ├── constant_time_compare()         [XOR compare]          ││
│  │  └── generate_session_id()           [OS CSPRNG]            ││
│  │                                                              ││
│  │  models/loader.rs                                            ││
│  │  └── hash_verify()                   [SHA-256]              ││
│  │                                                              ││
│  └─────────────────────────────────────────────────────────────┘│
│                                                                   │
│  Non-Cryptographic Components                                     │
│  ├── IPC Protocol (no encryption)                                │
│  ├── Inference Engine                                            │
│  ├── Memory Management                                           │
│  └── Telemetry                                                   │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 FIPS Algorithm Approval Status

| Algorithm | FIPS Approved | CAVP Test | Our Implementation |
|-----------|---------------|-----------|-------------------|
| AES-256 | Yes (FIPS 197) | AESAVS | aes-gcm crate |
| GCM Mode | Yes (SP 800-38D) | GCMVS | aes-gcm crate |
| SHA-256 | Yes (FIPS 180-4) | SHAVS | sha2 crate |
| PBKDF2 | Yes (SP 800-132) | No CAVP | pbkdf2 crate |
| HMAC-SHA256 | Yes (FIPS 198-1) | HMACVS | pbkdf2 crate |
| DRBG | Required (SP 800-90A) | DRBGVS | OS-provided |

### 5.3 Self-Test Implementation Guide

For future FIPS module implementation, the following self-tests would be required:

```rust
// Example: Power-on self-test (POST) structure
pub fn perform_power_on_self_test() -> Result<(), FipsError> {
    // 1. AES-256-GCM Known Answer Test
    aes_gcm_kat()?;

    // 2. SHA-256 Known Answer Test
    sha256_kat()?;

    // 3. PBKDF2 Known Answer Test
    pbkdf2_kat()?;

    // 4. CSPRNG Health Test
    csprng_health_test()?;

    Ok(())
}
```

---

## 6. References

1. NIST FIPS 140-2: Security Requirements for Cryptographic Modules
2. NIST FIPS 140-3: Security Requirements for Cryptographic Modules (successor)
3. NIST SP 800-38D: Recommendation for Block Cipher Modes (GCM)
4. NIST SP 800-132: Recommendation for Password-Based Key Derivation
5. NIST SP 800-90A: Recommendation for Random Number Generation Using DRBGs
6. CMVP: Cryptographic Module Validation Program
7. BoringSSL FIPS Certificate: https://csrc.nist.gov/projects/cryptographic-module-validation-program

---

## 7. Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-02-18 | FORGE/Scribe | Initial assessment |

---

**Document Status:** APPROVED FOR INTERNAL USE
**Next Review:** When FIPS requirements identified by business
