//! Fail-closed backend eligibility for a future standalone selector (#49/#50).
//!
//! The decision is purely informational: it does NOT load or execute models,
//! choose a model ID, grant authority or expand supported features. In
//! particular a compiled path is NOT evidence of installed task support.
//! Only a later evidence-bound, authenticated runtime snapshot may populate
//! qualified_tasks before this function is used in a serving controller.

use super::backend_capabilities::{
    BackendKind, ExecutionTarget, InferenceTask, RuntimeBackendCapabilities,
};

/// A caller's explicit task and execution target. No implicit cloud fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendRequirement {
    pub task: InferenceTask,
    pub target: ExecutionTarget,
    pub streaming: bool,
    /// When set, an exact backend is required; never silently substitute.
    pub pinned_backend: Option<BackendKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendRefusal {
    /// No backend had an evidence-qualified task and execution target.
    NoQualifiedCandidate,
    /// More than one eligible backend, and no owner-authored preference.
    AmbiguousSelection,
}

/// Identify a single eligible backend without inventing support or priority.
///
/// No performance score is supplied until exact, local, independently
/// qualified benchmark data are available. Ambiguity refuses rather than
/// silently choosing a model family or overriding caller policy.
///
/// The caller is responsible for ensuring entries originate from trusted,
/// evidence-bound local state; this function is not a certification oracle.
pub fn select_qualified_backend<'a>(
    catalog: &'a [RuntimeBackendCapabilities],
    requirement: BackendRequirement,
) -> Result<&'a RuntimeBackendCapabilities, BackendRefusal> {
    let mut matches = catalog.iter().filter(|backend| {
        backend.compiled_tasks.contains(&requirement.task)
            && backend.qualified_tasks.contains(&requirement.task)
            && backend.execution_targets.contains(&requirement.target)
            && (!requirement.streaming || backend.supports_streaming)
            && requirement
                .pinned_backend
                .is_none_or(|pinned| backend.backend == pinned)
    });
    let first = matches.next().ok_or(BackendRefusal::NoQualifiedCandidate)?;
    if matches.next().is_some() {
        return Err(BackendRefusal::AmbiguousSelection);
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::backend_capabilities::{
        compiled_backend_capabilities, CAPABILITY_SCHEMA_VERSION,
    };

    fn synthetic_qualified(backend: BackendKind, task: InferenceTask) -> RuntimeBackendCapabilities {
        RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend,
            model_formats: vec!["synthetic".to_owned()],
            compiled_tasks: vec![task],
            execution_targets: vec![ExecutionTarget::Cpu],
            supports_streaming: backend == BackendKind::Gguf,
            supports_kv_cache: backend == BackendKind::Gguf,
            qualified_tasks: vec![task],
        }
    }

    fn request(task: InferenceTask) -> BackendRequirement {
        BackendRequirement {
            task,
            target: ExecutionTarget::Cpu,
            streaming: false,
            pinned_backend: None,
        }
    }

    #[test]
    fn compiled_capability_catalog_never_claims_qualification() {
        let r = select_qualified_backend(
            &compiled_backend_capabilities(),
            request(InferenceTask::TextGeneration),
        );
        assert_eq!(r.unwrap_err(), BackendRefusal::NoQualifiedCandidate);
    }

    #[test]
    fn synthetic_evidence_matches_only_the_exact_supported_task() {
        let catalog = [synthetic_qualified(BackendKind::Onnx, InferenceTask::Embedding)];
        assert_eq!(
            select_qualified_backend(&catalog, request(InferenceTask::TextGeneration))
                .unwrap_err(),
            BackendRefusal::NoQualifiedCandidate
        );
        assert_eq!(
            select_qualified_backend(&catalog, request(InferenceTask::Embedding))
                .unwrap()
                .backend,
            BackendKind::Onnx
        );
    }

    #[test]
    fn unqualified_task_is_refused_even_when_compiled() {
        let mut only_compiled =
            synthetic_qualified(BackendKind::Gguf, InferenceTask::TextGeneration);
        only_compiled.qualified_tasks.clear();
        assert_eq!(
            select_qualified_backend(
                &[only_compiled],
                request(InferenceTask::TextGeneration)
            )
            .unwrap_err(),
            BackendRefusal::NoQualifiedCandidate
        );
    }

    #[test]
    fn streaming_and_hardware_require_exact_eligibility() {
        let catalog = [synthetic_qualified(BackendKind::Onnx, InferenceTask::Embedding)];
        let mut r = request(InferenceTask::Embedding);
        r.streaming = true;
        assert_eq!(
            select_qualified_backend(&catalog, r).unwrap_err(),
            BackendRefusal::NoQualifiedCandidate
        );
        r.streaming = false;
        r.target = ExecutionTarget::Cuda;
        assert_eq!(
            select_qualified_backend(&catalog, r).unwrap_err(),
            BackendRefusal::NoQualifiedCandidate
        );
    }

    #[test]
    fn multiple_candidates_require_an_explicit_backend_choice() {
        let catalog = [
            synthetic_qualified(BackendKind::Gguf, InferenceTask::TextGeneration),
            synthetic_qualified(BackendKind::Gguf, InferenceTask::TextGeneration),
        ];
        assert_eq!(
            select_qualified_backend(&catalog, request(InferenceTask::TextGeneration))
                .unwrap_err(),
            BackendRefusal::AmbiguousSelection
        );
        let mut r = request(InferenceTask::TextGeneration);
        r.pinned_backend = Some(BackendKind::Onnx);
        assert_eq!(
            select_qualified_backend(&catalog, r).unwrap_err(),
            BackendRefusal::NoQualifiedCandidate
        );
    }
}
