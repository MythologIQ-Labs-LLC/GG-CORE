> **Historical-provenance notice (2026-11-14):** This dated analysis retains its original terminology, measurements and findings for audit traceability. References to **"COREFORGE CORE Runtime"** describe earlier project framing, **not the current product dependency or ownership model**. **GG-CORE is the independent upstream standalone inference runtime; CoreForge is an optional downstream consumer.** Do not use this archival document to claim current platform isolation, security certification, supported model paths, performance or release maturity. Refer to [standalone dependency direction](../../docs/architecture/STANDALONE_DEPENDENCY_DIRECTION.md), [living architecture](../../docs/architecture/CORE_RUNTIME_ARCHITECTURE.md), [current security policy](../../SECURITY.md) and exact release evidence.

# FIPS 140-3 Security Policy Draft

**Document:** FIPS-4 Cryptographic Module Security Policy
**Version:** 0.1 (DRAFT)
**Date:** 2026-02-18
**Classification:** Internal / Compliance Preparation
**Project:** COREFORGE CORE Runtime v0.6.0

---

## Document Purpose

This document is a **draft** security policy prepared in anticipation of future FIPS 140-3 validation. It documents the cryptographic module boundary, algorithm inventory, key management procedures, and self-test procedures as they would be required for CMVP submission.

**Status:** This is a preparatory document. The cryptographic module described herein is NOT currently FIPS validated.

---

## 1. Cryptographic Module Specification

### 1.1 Module Identification

| Attribute | Value |
|-----------|-------|
| Module Name | COREFORGE CORE Cryptographic Module |
| Module Version | 0.6.0 |
| Module Type | Software |
| Cryptographic Boundary | Defined in Section 2 |
| Security Level | Level 1 (Target) |
| Operational Environment | General Purpose Computer |

### 1.2 Module Description

The COREFORGE CORE Cryptographic Module provides cryptographic services for the CORE Runtime inference engine. It protects model files at rest and authenticates IPC sessions between the runtime and control plane.

### 1.3 Approved Modes of Operation

| Mode | Description | Status |
|------|-------------|--------|
| FIPS Approved | All approved algorithms only | TARGET |
| Non-Approved | Non-validated operations | NOT ALLOWED |

---

## 2. Cryptographic Module Boundary

### 2.1 Physical Boundary

As a software module, the physical boundary is the general-purpose computer on which the module executes. The module does not have a physical perimeter distinct from its host.

### 2.2 Logical Boundary

The logical boundary encompasses the following components:

```
┌─────────────────────────────────────────────────────────────────┐
│                CRYPTOGRAPHIC MODULE BOUNDARY                     │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  security/encryption.rs                                     │ │
│  │  ├── ModelEncryption struct                                 │ │
│  │  ├── encrypt() -> AES-256-GCM encryption                   │ │
│  │  ├── decrypt() -> AES-256-GCM decryption                   │ │
│  │  ├── from_password() -> PBKDF2-HMAC-SHA256                 │ │
│  │  ├── from_machine_id() -> PBKDF2-HMAC-SHA256               │ │
│  │  └── generate_nonce() -> CSPRNG random generation          │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  ipc/auth.rs                                                │ │
│  │  ├── SessionAuth struct                                     │ │
│  │  ├── authenticate() -> SHA-256 hash + compare              │ │
│  │  ├── validate() -> session verification                    │ │
│  │  ├── constant_time_compare() -> timing-safe compare        │ │
│  │  └── generate_session_id() -> CSPRNG random generation     │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  Dependencies (within boundary)                             │ │
│  │  ├── aes-gcm 0.10 (AES-256-GCM implementation)             │ │
│  │  ├── pbkdf2 0.12 (PBKDF2-HMAC-SHA256 implementation)       │ │
│  │  ├── sha2 0.10 (SHA-256 implementation)                    │ │
│  │  └── rand 0.8 (OsRng CSPRNG wrapper)                       │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.3 Excluded Components

The following components are explicitly outside the cryptographic boundary:

| Component | Reason for Exclusion |
|-----------|---------------------|
| Inference Engine | Non-cryptographic computation |
| Model Loader | File I/O only (uses module for decryption) |
| IPC Protocol | Message encoding only (uses module for auth) |
| Telemetry | Logging only |
| Scheduler | Job management only |
| Memory Management | Buffer management only |

### 2.4 Entry and Exit Points

| Point | Type | Function | Data |
|-------|------|----------|------|
| encrypt() | Entry | Data encryption | Plaintext in, ciphertext out |
| decrypt() | Entry | Data decryption | Ciphertext in, plaintext out |
| from_password() | Entry | Key derivation | Password + salt in, key derived |
| authenticate() | Entry | Session creation | Token in, session out |
| validate() | Entry | Session validation | Session in, result out |

---

## 3. Approved Cryptographic Algorithms

### 3.1 Algorithm Inventory

| Algorithm | Mode/Use | Key Size | Standard | CAVP Status |
|-----------|----------|----------|----------|-------------|
| AES | GCM (Encryption) | 256 bits | FIPS 197, SP 800-38D | Pending |
| SHA-2 | SHA-256 (Hashing) | N/A | FIPS 180-4 | Pending |
| HMAC | HMAC-SHA-256 (MAC) | 256 bits | FIPS 198-1 | Pending |
| PBKDF2 | Key Derivation | 256 bits output | SP 800-132 | Pending |
| DRBG | Random Generation | N/A | SP 800-90A | OS-provided |

### 3.2 Algorithm Specifications

#### 3.2.1 AES-256-GCM

| Parameter | Value | Reference |
|-----------|-------|-----------|
| Block Size | 128 bits | FIPS 197 |
| Key Size | 256 bits | FIPS 197 |
| Nonce Size | 96 bits | SP 800-38D |
| Tag Size | 128 bits | SP 800-38D |
| Implementation | aes-gcm crate | RustCrypto |

**Approved Use:** Symmetric encryption of model files at rest.

#### 3.2.2 SHA-256

| Parameter | Value | Reference |
|-----------|-------|-----------|
| Message Digest Size | 256 bits | FIPS 180-4 |
| Block Size | 512 bits | FIPS 180-4 |
| Implementation | sha2 crate | RustCrypto |

**Approved Use:** Hashing for token comparison, HMAC component.

#### 3.2.3 HMAC-SHA-256

| Parameter | Value | Reference |
|-----------|-------|-----------|
| Key Size | 256 bits | FIPS 198-1 |
| Output Size | 256 bits | FIPS 198-1 |
| Implementation | pbkdf2 crate | RustCrypto |

**Approved Use:** Key derivation (as PRF in PBKDF2).

#### 3.2.4 PBKDF2

| Parameter | Value | Reference |
|-----------|-------|-----------|
| PRF | HMAC-SHA-256 | SP 800-132 |
| Iteration Count | 100,000 | OWASP Recommendation |
| Output Length | 256 bits | Key size requirement |
| Salt Length | 160 bits (20 bytes) | Current implementation |
| Implementation | pbkdf2 crate | RustCrypto |

**Approved Use:** Deriving encryption keys from passwords.

#### 3.2.5 DRBG (Random Number Generation)

| Parameter | Value | Reference |
|-----------|-------|-----------|
| Algorithm | OS-provided CSPRNG | SP 800-90A (delegated) |
| Interface | OsRng | rand crate |
| Entropy Source | Operating system | Platform-dependent |

**Approved Use:** Nonce generation, session ID generation.

**Note:** The random number generator is provided by the operating system. For full FIPS compliance, the OS must be configured in FIPS mode.

### 3.3 Non-Approved Algorithms

| Algorithm | Use | Status | Migration Plan |
|-----------|-----|--------|----------------|
| AES-ECB | Legacy decryption | DEPRECATED | Removed in v0.5.0 |

---

## 4. Key Management

### 4.1 Key Types

| Key Type | Size | Algorithm | Lifetime | Storage |
|----------|------|-----------|----------|---------|
| Model Encryption Key | 256 bits | AES-256-GCM | Per-session | Memory |
| PBKDF2 Derived Key | 256 bits | PBKDF2 | Per-derivation | Memory |
| Token Hash | 256 bits | SHA-256 | Configuration lifetime | Memory |
| Session ID | 256 bits | Random | Session lifetime | Memory |

### 4.2 Key Generation

#### 4.2.1 Model Encryption Key

Keys are derived using PBKDF2-HMAC-SHA256:

```
Input:
  - Password: User-provided string OR machine identifier
  - Salt: 20-byte constant ("hearthlink-core-salt")
  - Iterations: 100,000
  - Output Length: 32 bytes (256 bits)

Output:
  - AES-256 key for model encryption/decryption
```

#### 4.2.2 Session ID

Session IDs are generated using the operating system's CSPRNG:

```
Input:
  - None (entropy from OS)

Process:
  - Request 32 bytes from OsRng
  - Encode as 64-character hexadecimal string

Output:
  - 256-bit session identifier
```

### 4.3 Key Storage

| Key Type | Storage Location | Protection |
|----------|------------------|------------|
| Derived Keys | Process memory | Memory isolation |
| Session IDs | Process memory | Memory isolation |
| Token Hashes | Process memory | Computed at initialization |

**Note:** Keys are not persisted to disk. All keys exist only in process memory during runtime.

### 4.4 Key Destruction

Keys are destroyed through the following mechanisms:

| Event | Destruction Method |
|-------|-------------------|
| Session expiration | Memory overwrite on struct drop |
| Process termination | OS memory reclamation |
| Explicit cleanup | Memory zeroization (planned) |

**Future Enhancement:** Implement explicit zeroization using `zeroize` crate for enhanced key destruction.

### 4.5 Key Entry and Output

| Direction | Method | Protection |
|-----------|--------|------------|
| Key Entry | API parameter (password) | Not displayed |
| Key Entry | Machine ID derivation | Automatic |
| Key Output | None | Keys never exported |

---

## 5. Security Roles and Authentication

### 5.1 Operator Roles

| Role | Description | Authentication |
|------|-------------|----------------|
| User | Performs cryptographic operations | Handshake token |
| Crypto Officer | Configures cryptographic parameters | Not implemented (future) |

### 5.2 Authentication Mechanisms

#### 5.2.1 User Authentication

Users authenticate via handshake token:

1. User provides pre-shared token via IPC
2. Token is hashed with SHA-256
3. Hash compared using constant-time algorithm
4. On success, session ID is generated and returned

#### 5.2.2 Authentication Strength

| Aspect | Specification |
|--------|---------------|
| Token Length | Implementation-defined (recommended >32 chars) |
| Hash Algorithm | SHA-256 |
| Comparison | Constant-time XOR-based |
| Rate Limiting | 5 failures per 60 seconds, 30-second lockout |

### 5.3 Unauthenticated Services

The following services are available without authentication:

| Service | Justification |
|---------|---------------|
| Health check | Required for orchestration |
| Metrics export | Monitoring requirement |

---

## 6. Self-Tests

### 6.1 Power-On Self-Tests (POST)

The following tests MUST pass before the module enters operational state:

| Test ID | Algorithm | Test Type | Implementation Status |
|---------|-----------|-----------|----------------------|
| POST-001 | AES-256-GCM | Known Answer Test (KAT) | PLANNED |
| POST-002 | SHA-256 | Known Answer Test (KAT) | PLANNED |
| POST-003 | HMAC-SHA-256 | Known Answer Test (KAT) | PLANNED |
| POST-004 | PBKDF2 | Known Answer Test (KAT) | PLANNED |
| POST-005 | CSPRNG | Health Test | PLANNED |

#### 6.1.1 AES-256-GCM KAT

```rust
// Example KAT structure (to be implemented)
const AES_GCM_KAT_KEY: [u8; 32] = [...];
const AES_GCM_KAT_NONCE: [u8; 12] = [...];
const AES_GCM_KAT_PLAINTEXT: [u8; 16] = [...];
const AES_GCM_KAT_EXPECTED_CIPHERTEXT: [u8; 32] = [...]; // includes tag

fn post_aes_gcm() -> Result<(), PostError> {
    let result = encrypt(AES_GCM_KAT_KEY, AES_GCM_KAT_NONCE, AES_GCM_KAT_PLAINTEXT);
    if result != AES_GCM_KAT_EXPECTED_CIPHERTEXT {
        return Err(PostError::AesGcmKatFailed);
    }
    Ok(())
}
```

### 6.2 Conditional Self-Tests

| Test ID | Algorithm | Trigger | Implementation Status |
|---------|-----------|---------|----------------------|
| COND-001 | AES-256-GCM | Key pair generation | PLANNED |
| COND-002 | CSPRNG | Continuous test | PLANNED |

### 6.3 Failure Handling

On self-test failure:

1. Module enters error state
2. All cryptographic operations are disabled
3. Error is logged to security audit log
4. Module must be restarted to retry self-tests

---

## 7. Physical Security

### 7.1 Security Level 1 Requirements

As a software module targeting Security Level 1:

| Requirement | Implementation |
|-------------|----------------|
| Production-grade components | Commercial operating system |
| Single operator mode | Process-level isolation |
| No physical security mechanisms | N/A (software module) |

---

## 8. Operational Environment

### 8.1 Supported Platforms

| Platform | Operating System | Status |
|----------|------------------|--------|
| x86_64 | Windows 10/11 | SUPPORTED |
| x86_64 | Windows Server 2016+ | SUPPORTED |
| x86_64 | Linux (glibc 2.17+) | SUPPORTED |
| x86_64 | macOS 10.15+ | SUPPORTED |
| aarch64 | Linux | SUPPORTED |
| aarch64 | macOS (Apple Silicon) | SUPPORTED |

### 8.2 Operating Environment Requirements

For FIPS-approved operation:

| Requirement | Specification |
|-------------|---------------|
| Operating System | Must be FIPS-capable |
| FIPS Mode | OS must be configured in FIPS mode |
| Random Source | OS-provided FIPS-validated DRBG |
| Memory Protection | Standard OS memory protection |

---

## 9. Mitigation of Other Attacks

### 9.1 Timing Attacks

| Countermeasure | Implementation |
|----------------|----------------|
| Constant-time comparison | XOR-based token comparison |
| Timing-safe crypto | RustCrypto constant-time implementations |

### 9.2 Side-Channel Attacks

| Attack Vector | Mitigation |
|---------------|------------|
| Cache timing | Hardware acceleration (AES-NI) |
| Power analysis | Not applicable (software module) |
| EM analysis | Not applicable (software module) |

### 9.3 Fault Injection

| Countermeasure | Implementation |
|----------------|----------------|
| Self-tests | POST and conditional tests (planned) |
| Error checking | Return value verification |

---

## 10. Documentation Requirements

### 10.1 Required Documents for CMVP Submission

| Document | Status |
|----------|--------|
| Security Policy (this document) | DRAFT |
| Finite State Model | NOT STARTED |
| Key Management Procedures | PARTIAL (Section 4) |
| Self-Test Procedures | PLANNED (Section 6) |
| Entropy Assessment | NOT STARTED |
| Source Code | AVAILABLE |

### 10.2 User Documentation

| Document | Status |
|----------|--------|
| Admin Guide | PARTIAL |
| User Guide | docs/USAGE_GUIDE.md |
| Deployment Guide | NOT STARTED |

---

## 11. Version History

| Version | Date | Changes |
|---------|------|---------|
| 0.1 | 2026-02-18 | Initial draft |

---

## Appendix A: Acronyms

| Acronym | Definition |
|---------|------------|
| AES | Advanced Encryption Standard |
| CAVP | Cryptographic Algorithm Validation Program |
| CMVP | Cryptographic Module Validation Program |
| CSPRNG | Cryptographically Secure Pseudo-Random Number Generator |
| DRBG | Deterministic Random Bit Generator |
| FIPS | Federal Information Processing Standards |
| GCM | Galois/Counter Mode |
| HMAC | Hash-based Message Authentication Code |
| KAT | Known Answer Test |
| NIST | National Institute of Standards and Technology |
| PBKDF2 | Password-Based Key Derivation Function 2 |
| POST | Power-On Self-Test |

---

## Appendix B: FIPS 140-3 Compliance Checklist

| Requirement | FIPS 140-3 Section | Status |
|-------------|-------------------|--------|
| Cryptographic Module Specification | 7.2 | PARTIAL |
| Cryptographic Module Interfaces | 7.3 | PARTIAL |
| Roles, Services, Authentication | 7.4 | PARTIAL |
| Software/Firmware Security | 7.5 | NOT STARTED |
| Operational Environment | 7.6 | PARTIAL |
| Physical Security | 7.7 | N/A (Level 1) |
| Non-Invasive Security | 7.8 | NOT STARTED |
| Sensitive Security Parameter Mgmt | 7.9 | PARTIAL |
| Self-Tests | 7.10 | PLANNED |
| Life-Cycle Assurance | 7.11 | NOT STARTED |
| Mitigation of Other Attacks | 7.12 | PARTIAL |

---

## Appendix C: Implementation Files

| File Path | Purpose | Lines |
|-----------|---------|-------|
| `core-runtime/src/security/encryption.rs` | AES-256-GCM, PBKDF2 | 992 |
| `core-runtime/src/ipc/auth.rs` | SHA-256, session management | 547 |
| `core-runtime/src/telemetry/security_log.rs` | Security audit logging | 223 |
| `core-runtime/src/security/audit.rs` | Enterprise audit module | 619 |

---

**DRAFT DOCUMENT - NOT FOR CMVP SUBMISSION**

This document requires completion of:
1. Self-test implementation
2. Entropy assessment
3. Finite state model
4. CAVP algorithm validation
5. Independent testing laboratory engagement

---

**END OF SECURITY POLICY DRAFT**
