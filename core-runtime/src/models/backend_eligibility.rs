//! Explained, fail-closed backend eligibility (#50).
//!
//! Pure and stateless: no scoring, no history, so identical inputs always
//! give the identical result (no oscillation). Informational only: it loads
//! nothing, selects no model, and never raises a memory budget or any other
//! authority. A pinned backend is never substituted; ambiguity selects none.
//! Memory budgets, host authority and model selection are deferred to the
//! #50 / #153 model-selection policy (see V1_BACKEND_CAPABILITY_CONTRACT.md).

use super::backend_capabilities::{
    BackendKind, RuntimeBackendCapabilities, ServingPath, CAPABILITY_SCHEMA_VERSION,
};
use super::backend_selection::BackendRequirement;

/// Why one catalog entry was not eligible (the first failing rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rejection {
    SchemaVersion,
    NotPinned,
    PathNotCompiled,
    PathNotQualified,
    NoStreaming,
}

/// Every candidate's verdict, and the single selection if exactly one passed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Eligibility {
    /// Catalog index of the selected entry: set only when exactly one passed.
    pub selected: Option<usize>,
    /// Catalog indexes of every entry that passed every rule.
    pub eligible: Vec<usize>,
    /// Catalog index, backend and first failing rule of every other entry.
    pub rejections: Vec<(usize, BackendKind, Rejection)>,
}

impl Eligibility {
    /// More than one entry passed and no owner preference chose between them.
    pub fn is_ambiguous(&self) -> bool {
        self.eligible.len() > 1
    }
}

/// Evaluate every entry against `requirement`, recording each refusal.
pub fn evaluate_backends(
    catalog: &[RuntimeBackendCapabilities],
    requirement: BackendRequirement,
) -> Eligibility {
    let path = ServingPath::new(requirement.task, requirement.target);
    let (mut eligible, mut rejections) = (Vec::new(), Vec::new());
    for (index, backend) in catalog.iter().enumerate() {
        match first_failure(backend, &path, requirement) {
            None => eligible.push(index),
            Some(reason) => rejections.push((index, backend.backend, reason)),
        }
    }
    let selected = (eligible.len() == 1).then(|| eligible[0]);
    Eligibility {
        selected,
        eligible,
        rejections,
    }
}

fn first_failure(
    backend: &RuntimeBackendCapabilities,
    path: &ServingPath,
    requirement: BackendRequirement,
) -> Option<Rejection> {
    if backend.schema_version != CAPABILITY_SCHEMA_VERSION {
        return Some(Rejection::SchemaVersion);
    }
    if requirement
        .pinned_backend
        .is_some_and(|pinned| backend.backend != pinned)
    {
        return Some(Rejection::NotPinned);
    }
    if !backend.compiled_paths.contains(path) {
        return Some(Rejection::PathNotCompiled);
    }
    if !backend.qualified_paths.contains(path) {
        return Some(Rejection::PathNotQualified);
    }
    if requirement.streaming && !backend.supports_streaming {
        return Some(Rejection::NoStreaming);
    }
    None
}

#[cfg(test)]
#[path = "backend_eligibility_tests.rs"]
mod tests;
