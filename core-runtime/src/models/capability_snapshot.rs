//! Authenticated, versioned capability snapshot (#49 / #50).
//!
//! Output-only: every type is `Serialize` without `Deserialize`, and the IPC
//! response carries the serialized JSON as an opaque value, so the server
//! never ingests client-supplied capability metadata. Compiled paths, loaded
//! model capabilities, detected hardware, evidence-qualified support and
//! operator constraints are separate fields; none is inferred from another.

use serde::Serialize;

use super::backend_capabilities::{
    compiled_backend_capabilities, BackendKind, ExecutionTarget, InferenceTask,
    RuntimeBackendCapabilities, ServingPath, CAPABILITY_SCHEMA_VERSION,
};
use super::lifecycle::ModelLifecycle;
use crate::engine::{InferenceCapability, InferenceEngine, Model};

/// One registered, serving model's capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct LoadedModelCapability {
    pub model_id: String,
    pub handle_id: u64,
    /// The concrete backend object found by type, or `None` (never guessed).
    pub backend: Option<BackendKind>,
    /// Declared capabilities that the detected backend's compiled paths serve.
    pub paths: Vec<ServingPath>,
    /// Declared by a model of unknown backend: not a serving claim.
    pub declared_only_paths: Vec<ServingPath>,
}

/// Detected host facts. Detection is never a serving claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct HardwareFacts {
    pub logical_cpus: usize,
    /// `None` means "not reported", not "none present".
    pub accelerators: Option<Vec<String>>,
}

/// The configured memory budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct BudgetFacts {
    pub max_total_memory: usize,
    pub per_call_ceiling: Option<usize>,
}

/// Operator constraints, read from the running configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct OperatorConstraints {
    /// Compile-time fact: no network inference path exists in this runtime.
    pub offline_only: bool,
    /// Compile-time fact: the runtime opens no network listener.
    pub network_listener: bool,
    /// From the IPC handler configuration (`require_auth`).
    pub auth_required: bool,
    /// `None`: no budget configured (embedded, unlimited).
    pub budget: Option<BudgetFacts>,
}

/// The full snapshot (capability schema version, not the IPC version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct CapabilitySnapshot {
    pub schema_version: u16,
    pub runtime_version: String,
    pub compiled: Vec<RuntimeBackendCapabilities>,
    pub loaded: Vec<LoadedModelCapability>,
    pub hardware: HardwareFacts,
    /// Evidence-qualified cells: empty until a trusted evidence registry exists.
    pub qualified: Vec<ServingPath>,
    pub operator: OperatorConstraints,
}

/// Build the snapshot. Engine models are inspected under one read lock
/// (synchronous visitor), then joined to lifecycle handles by model id after
/// that lock is released; a model without a lifecycle handle is omitted.
/// Known limit: a model unloaded and reloaded under the same id between the
/// two phases can pair the new handle with the old instance's backend and
/// tasks. The result is transient and never claims more than either
/// instance could serve. `paths` means compiled-reachable for the detected
/// backend type, not a readiness or quality claim.
pub(crate) async fn capability_snapshot(
    lifecycle: &ModelLifecycle,
    engine: &InferenceEngine,
    auth_required: bool,
) -> CapabilitySnapshot {
    let compiled = compiled_backend_capabilities();
    let mut seen = Vec::new();
    engine
        .inspect_models(|id, model| seen.push((id.to_string(), backend_of(model), tasks_of(model))))
        .await;
    let mut loaded = Vec::new();
    for (model_id, backend, tasks) in seen {
        if let Some(handle) = lifecycle.get_handle(&model_id).await {
            loaded.push(loaded_entry(
                model_id,
                handle.id(),
                backend,
                &tasks,
                &compiled,
            ));
        }
    }
    loaded.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    CapabilitySnapshot {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        runtime_version: env!("CARGO_PKG_VERSION").to_string(),
        compiled,
        loaded,
        hardware: hardware_facts(),
        qualified: Vec::new(),
        operator: operator_constraints(lifecycle, auth_required),
    }
}

fn loaded_entry(
    model_id: String,
    handle_id: u64,
    backend: Option<BackendKind>,
    tasks: &[InferenceTask],
    compiled: &[RuntimeBackendCapabilities],
) -> LoadedModelCapability {
    let declared: Vec<ServingPath> = tasks
        .iter()
        .map(|&task| ServingPath::new(task, ExecutionTarget::Cpu))
        .collect();
    let served = |path: &ServingPath| {
        compiled
            .iter()
            .any(|c| Some(c.backend) == backend && c.compiled_paths.contains(path))
    };
    let (paths, declared_only_paths) = match backend {
        Some(_) => (declared.into_iter().filter(served).collect(), Vec::new()),
        None => (Vec::new(), declared),
    };
    LoadedModelCapability {
        model_id,
        handle_id,
        backend,
        paths,
        declared_only_paths,
    }
}

/// The concrete backend type of a registered model, by downcast.
fn backend_of(model: &dyn Model) -> Option<BackendKind> {
    let any = model.as_any();
    #[cfg(feature = "gguf")]
    if any.is::<crate::engine::gguf::GgufGenerator>() {
        return Some(BackendKind::Gguf);
    }
    #[cfg(feature = "onnx")]
    if any.is::<crate::engine::onnx::OnnxEmbedder>()
        || any.is::<crate::engine::onnx::OnnxClassifier>()
    {
        return Some(BackendKind::Onnx);
    }
    let _ = any;
    None
}

/// Declared tasks; capabilities with no task cell (NER) are omitted.
fn tasks_of(model: &dyn Model) -> Vec<InferenceTask> {
    model
        .capabilities()
        .iter()
        .filter_map(|cap| match cap {
            InferenceCapability::TextGeneration => Some(InferenceTask::TextGeneration),
            InferenceCapability::Embedding => Some(InferenceTask::Embedding),
            InferenceCapability::TextClassification => Some(InferenceTask::Classification),
            InferenceCapability::NamedEntityRecognition => None,
        })
        .collect()
}

fn hardware_facts() -> HardwareFacts {
    HardwareFacts {
        logical_cpus: std::thread::available_parallelism().map_or(1, |n| n.get()),
        accelerators: None,
    }
}

fn operator_constraints(lifecycle: &ModelLifecycle, auth_required: bool) -> OperatorConstraints {
    let budget = lifecycle.resource_limits().map(|limits| BudgetFacts {
        max_total_memory: limits.max_total_memory(),
        per_call_ceiling: limits.per_call_ceiling(),
    });
    OperatorConstraints {
        offline_only: true,
        network_listener: false,
        auth_required,
        budget,
    }
}

#[cfg(test)]
#[path = "capability_snapshot_tests.rs"]
mod tests;
