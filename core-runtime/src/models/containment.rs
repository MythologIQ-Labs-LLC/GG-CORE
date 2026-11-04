//! Physical (symlink-aware) containment for validated model paths.
//!
//! `ModelLoader::validate_path` normalizes lexically so traversal can never
//! escape the allowlist on paper. Lexical checks cannot see filesystem
//! indirection: a symlink planted inside `models/` resolves wherever it
//! points. This module resolves the *real* location of an existing
//! candidate and requires it to lie under the real location of an allowed
//! root. Absent paths pass (nothing can be read through them; the loader
//! reports NotFound), dangling or unresolvable entries fail closed.

use std::path::{Path, PathBuf};

use crate::telemetry::{log_security_event, SecurityEvent};

/// Result of resolving a candidate against the allowed roots.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Physical {
    /// The candidate exists and its real path is under an allowed root.
    Contained,
    /// Nothing exists at the candidate path (lexical result stands).
    Absent,
    /// The candidate resolves outside every allowed root, or cannot be
    /// resolved (dangling symlink, unreadable component).
    Escapes,
}

/// Resolve `candidate` physically and classify it against `roots`.
pub(super) fn check(candidate: &Path, roots: &[PathBuf]) -> Physical {
    match std::fs::symlink_metadata(candidate) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Physical::Absent,
        Err(_) => return Physical::Escapes,
        Ok(_) => {}
    }
    let real = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return Physical::Escapes,
    };
    let contained = roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| real.starts_with(&root));
    if contained {
        Physical::Contained
    } else {
        log_security_event(
            SecurityEvent::PathTraversalAttempt,
            "model path resolves outside the allowed directories (filesystem indirection)",
            &[
                ("requested", &candidate.display().to_string()),
                ("resolved", &real.display().to_string()),
            ],
        );
        Physical::Escapes
    }
}
