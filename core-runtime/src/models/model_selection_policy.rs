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
use super::hardware_profile::HardwareProfile;

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

/// Opt-in, non-executing hardware-aware selection. Trusted local capability
/// evidence and the host's policy limits are both mandatory. Unknown target
/// availability or cold-load headroom refuses the candidate, rather than
/// assuming that a compiled backend can serve it on the actual machine.
pub fn plan_model_selection_with_hardware<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    hardware: &HardwareProfile,
) -> ModelSelectionPlan<'a> {
    let compiled = compiled_backend_capabilities();
    decision::plan(offers, policy, &compiled, Some(hardware))
}

/// Opt-in local-budget advisory selector, without client-supplied hardware
/// headroom. The snapshot reads the same shared reservation accounting used
/// by native model residency and per-request working admission.
///
/// It is a proposal only, never a memory reservation. A concurrent
/// request can consume capacity before the suggested model is loaded.
/// Exact model evidence, host approval and canonical admission remain
/// mandatory at the execution boundary.
pub fn plan_model_selection_with_local_limits<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    limits: Option<&crate::memory::ResourceLimits>,
) -> ModelSelectionPlan<'a> {
    let hardware = HardwareProfile::detect_with_limits(limits);
    plan_model_selection_with_hardware(offers, policy, &hardware)
}

/// Private seam for tests: production always uses its own compiled catalog.
fn plan_with_compiled_catalog<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> ModelSelectionPlan<'a> {
    decision::plan(offers, policy, compiled, None)
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

#[cfg(test)]
fn plan_with_compiled_catalog_and_hardware<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
    hardware: &HardwareProfile,
) -> ModelSelectionPlan<'a> {
    decision::plan(offers, policy, compiled, Some(hardware))
}

#[cfg(test)]
#[path = "model_selection_test_hardware.rs"]
mod test_hardware;

#[cfg(test)]
#[path = "model_selection_test_local_limits.rs"]
mod test_local_limits;
