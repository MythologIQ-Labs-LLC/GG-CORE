//! Explained eligibility: every rejection reason, no substitution, no
//! ambiguity resolution, and deterministic results.

use super::*;
use crate::models::backend_capabilities::{ExecutionTarget, InferenceTask};

fn entry(backend: BackendKind, task: InferenceTask, qualified: bool) -> RuntimeBackendCapabilities {
    let path = ServingPath::new(task, ExecutionTarget::Cpu);
    RuntimeBackendCapabilities {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        backend,
        model_formats: vec!["synthetic".into()],
        compiled_paths: vec![path],
        supports_streaming: backend == BackendKind::Gguf,
        supports_kv_cache: false,
        qualified_paths: if qualified { vec![path] } else { Vec::new() },
    }
}

fn need(task: InferenceTask) -> BackendRequirement {
    BackendRequirement {
        task,
        target: ExecutionTarget::Cpu,
        streaming: false,
        pinned_backend: None,
    }
}

#[test]
fn every_rejection_reason_is_reported() {
    let mut stale = entry(BackendKind::Gguf, InferenceTask::TextGeneration, true);
    stale.schema_version += 1;
    let catalog = [
        stale,
        entry(BackendKind::Onnx, InferenceTask::Embedding, true),
        entry(BackendKind::Gguf, InferenceTask::TextGeneration, false),
    ];
    let e = evaluate_backends(&catalog, need(InferenceTask::TextGeneration));
    assert_eq!(e.selected, None);
    assert_eq!(
        e.rejections,
        vec![
            (0, BackendKind::Gguf, Rejection::SchemaVersion),
            (1, BackendKind::Onnx, Rejection::PathNotCompiled),
            (2, BackendKind::Gguf, Rejection::PathNotQualified),
        ]
    );
}

#[test]
fn streaming_and_pinning_refuse_without_substitution() {
    let catalog = [
        entry(BackendKind::Onnx, InferenceTask::Embedding, true),
        entry(BackendKind::Gguf, InferenceTask::Embedding, true),
    ];
    let mut r = need(InferenceTask::Embedding);
    r.pinned_backend = Some(BackendKind::Onnx);
    r.streaming = true;
    let e = evaluate_backends(&catalog, r);
    assert_eq!(
        e.selected, None,
        "the pinned backend cannot stream; GGUF is not substituted"
    );
    assert_eq!(
        e.rejections[0],
        (0, BackendKind::Onnx, Rejection::NoStreaming)
    );
    assert_eq!(
        e.rejections[1],
        (1, BackendKind::Gguf, Rejection::NotPinned)
    );
}

#[test]
fn ambiguity_selects_nothing_and_results_are_deterministic() {
    let catalog = [
        entry(BackendKind::Gguf, InferenceTask::TextGeneration, true),
        entry(BackendKind::Gguf, InferenceTask::TextGeneration, true),
    ];
    let first = evaluate_backends(&catalog, need(InferenceTask::TextGeneration));
    assert!(first.is_ambiguous());
    assert_eq!((first.selected, first.eligible.clone()), (None, vec![0, 1]));
    for _ in 0..10 {
        assert_eq!(
            evaluate_backends(&catalog, need(InferenceTask::TextGeneration)),
            first
        );
    }
}

#[test]
fn exactly_one_qualified_entry_is_selected() {
    let catalog = [
        entry(BackendKind::Gguf, InferenceTask::TextGeneration, false),
        entry(BackendKind::Onnx, InferenceTask::Classification, true),
    ];
    let e = evaluate_backends(&catalog, need(InferenceTask::Classification));
    assert_eq!(e.selected, Some(1));
    assert_eq!(
        e.rejections,
        vec![(0, BackendKind::Gguf, Rejection::PathNotCompiled)]
    );
}
