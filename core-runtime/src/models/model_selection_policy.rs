//! Quality-floor-first model-level routing plan (#50 / #150).
//!
//! This is an evidence-bound *decision*, never an execution or certification.
//! The caller supplies trusted, locally verified offers and host constraints.
//! No network fallback, model mutation, inference or automatic cold swap occurs.
//! Real lifecycle admission, handle validation, draining and readiness remain
//! independently mandatory at execution time (see #137 and #153).
//!
//! Evidence cannot be minted by the caller of an authenticated IPC request:
//! a future controller must source it from its own reviewed, exact-artifact
//! model/benchmark records. These public structs are NOT a trust boundary.

use super::backend_capabilities::{compiled_backend_capabilities, RuntimeBackendCapabilities};

#[path = "model_selection_types.rs"]
mod types;
pub use types::*;

#[path = "model_selection_assess.rs"]
mod assessment;
#[path = "model_selection_decide.rs"]
mod decision;

/// Plan a request-boundary choice using measured total cost of a useful answer
/// AFTER enforcing qualification, quality, custody, task and budgets.
/// This is pure advisory output. Revalidate model identity, host policy and
/// resource admission at the actual authenticated execution boundary.
pub fn plan_model_selection<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
) -> ModelSelectionPlan<'a> {
    let compiled = compiled_backend_capabilities();
    plan_with_compiled_catalog(offers, policy, &compiled)
}

/// Private seam for tests: production always uses its own compiled catalog.
fn plan_with_compiled_catalog<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> ModelSelectionPlan<'a> {
    decision::plan(offers, policy, compiled)
}

#[cfg(test)]
#[path = "model_selection_test_authority.rs"]
mod test_authority;
#[cfg(test)]
#[path = "model_selection_test_evidence.rs"]
mod test_evidence;
#[cfg(test)]
#[path = "model_selection_test_quality.rs"]
mod test_quality;
#[cfg(test)]
#[path = "model_selection_test_support.rs"]
mod test_support;
