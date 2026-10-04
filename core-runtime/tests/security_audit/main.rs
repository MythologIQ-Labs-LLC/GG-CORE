//! Security Audit Test Suite for COREFORGE CORE Runtime
//!
//! This module contains penetration testing scenarios designed to validate
//! the security boundaries and defenses of the CORE runtime. These tests
//! simulate adversarial behavior to ensure the system is resilient against
//! real-world attacks.
//!
//! ## Test Categories
//!
//! - **boundary_tests**: Sandbox escape attempts and isolation verification
//! - **crypto_tests**: Cryptographic correctness and attack resistance
//! - **ipc_fuzzing**: Protocol fuzzing and malformed message handling
//! - **auth_attacks**: Authentication bypass and token manipulation
//!
//! ## Security Boundaries Tested
//!
//! 1. IPC Authentication (SHA-256 HMAC tokens)
//! 2. Model Encryption (AES-256-GCM with PBKDF2)
//! 3. Windows Job Objects / Unix seccomp sandbox
//! 4. Input validation on all IPC messages
//! 5. Path traversal prevention
//!
//! ## C.O.R.E. Constraints Verified
//!
//! - **Contained**: Sandbox with no ambient privileges
//! - **Offline**: Zero network access (inbound/outbound blocked)
//! - **Restricted**: IPC-only communication with authenticated callers
//! - **Execution**: Pure compute, no business logic or decision authority

mod auth_attacks;
mod boundary_tests;
mod crypto_tests;
mod ipc_fuzzing;
