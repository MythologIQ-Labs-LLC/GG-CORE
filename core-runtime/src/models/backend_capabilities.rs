//! Versioned, fail-closed *compile-time* backend capability declarations (#49).
//!
//! These are not installed-model claims or release-support certificates.
//! GPU detection, Cargo features and library-only algorithms cannot be used
//! to advertise functional serving paths. A later authenticated IPC surface
//! will compose this catalog with verified model/task and hardware evidence.

use serde::{Deserialize, Serialize};

/// Version of this additive metadata representation, not IPC protocol V1.
pub const CAPABILITY_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BackendKind {
    Gguf,
    Onnx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InferenceTask {
    TextGeneration,
    Embedding,
    Classification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecutionTarget {
    Cpu,
    Cuda,
    Metal,
}

/// A task/execution-target pair is indivisible: never infer a Cartesian
/// product such as GPU embeddings from CPU embeddings plus GPU generation.
///
/// Output-only (`Serialize`, no `Deserialize`): support claims flow out of
/// the runtime and are never accepted from clients or files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ServingPath {
    pub task: InferenceTask,
    pub target: ExecutionTarget,
}

impl ServingPath {
    /// One exact task on one execution target.
    pub const fn new(task: InferenceTask, target: ExecutionTarget) -> Self {
        Self { task, target }
    }
}

/// Output-only like [`ServingPath`]: there is deliberately no `Deserialize`,
/// so untrusted JSON can never become a `qualified_paths` claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RuntimeBackendCapabilities {
    pub schema_version: u16,
    pub backend: BackendKind,
    /// Loader-recognized formats, not every architecture or model family.
    pub model_formats: Vec<String>,
    /// Task+accelerator paths reachable in the compiled serving code.
    /// This is NOT a packaged-support claim.
    pub compiled_paths: Vec<ServingPath>,
    /// Whether the compiled source implements token streaming for this backend.
    pub supports_streaming: bool,
    pub supports_kv_cache: bool,
    /// Only exact task+accelerator cells may be qualified by separate
    /// pinned real-model installed evidence. Empty until supplied by a
    /// trusted runtime evidence registry, never derived from Cargo flags.
    pub qualified_paths: Vec<ServingPath>,
}

/// Only backends with compiled implementation paths are declared.
///
/// In the current GGUF/ONNX code, CPU is the only actual backend target.
/// CUDA/Metal modules do not imply serving offload, and ONNX generation is
/// not implemented. Even compiled tasks need a separately audited installed
/// real-model support matrix before they can be advertised as qualified.
pub fn compiled_backend_capabilities() -> Vec<RuntimeBackendCapabilities> {
    let mut backends = Vec::new();
    if cfg!(feature = "gguf") {
        backends.push(RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Gguf,
            model_formats: vec!["gguf".to_string()],
            compiled_paths: vec![ServingPath {
                task: InferenceTask::TextGeneration,
                target: ExecutionTarget::Cpu,
            }],
            supports_streaming: true,
            supports_kv_cache: true,
            qualified_paths: Vec::new(),
        });
    }
    if cfg!(feature = "onnx") {
        backends.push(RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Onnx,
            model_formats: vec!["onnx".to_string()],
            compiled_paths: vec![
                ServingPath {
                    task: InferenceTask::Embedding,
                    target: ExecutionTarget::Cpu,
                },
                ServingPath {
                    task: InferenceTask::Classification,
                    target: ExecutionTarget::Cpu,
                },
            ],
            supports_streaming: false,
            supports_kv_cache: false,
            qualified_paths: Vec::new(),
        });
    }
    backends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_false_acceleration_or_release_claims() {
        for capabilities in compiled_backend_capabilities() {
            assert_eq!(capabilities.schema_version, CAPABILITY_SCHEMA_VERSION);
            assert!(capabilities
                .compiled_paths
                .iter()
                .all(|path| path.target == ExecutionTarget::Cpu));
            assert!(capabilities.qualified_paths.is_empty());
        }
    }

    #[test]
    fn onnx_generation_and_unsupported_bitnet_are_not_advertised() {
        let catalog = compiled_backend_capabilities();
        assert!(catalog.iter().all(|c| c.backend != BackendKind::Onnx
            || !c
                .compiled_paths
                .iter()
                .any(|path| { path.task == InferenceTask::TextGeneration })));
        assert!(catalog
            .iter()
            .all(|c| c.backend == BackendKind::Gguf || c.backend == BackendKind::Onnx));
    }

    #[test]
    fn compile_time_feature_matrix_is_exact() {
        let catalog = compiled_backend_capabilities();
        assert_eq!(
            catalog.iter().any(|c| c.backend == BackendKind::Gguf),
            cfg!(feature = "gguf")
        );
        assert_eq!(
            catalog.iter().any(|c| c.backend == BackendKind::Onnx),
            cfg!(feature = "onnx")
        );
        assert_eq!(
            catalog.len(),
            usize::from(cfg!(feature = "gguf")) + usize::from(cfg!(feature = "onnx"))
        );
    }

    #[test]
    fn schema_serializes_without_inventing_hardware() {
        let input = RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Gguf,
            model_formats: vec!["gguf".into()],
            compiled_paths: vec![ServingPath::new(
                InferenceTask::TextGeneration,
                ExecutionTarget::Cpu,
            )],
            supports_streaming: true,
            supports_kv_cache: true,
            qualified_paths: Vec::new(),
        };
        let json = serde_json::to_string(&input).expect("serialize");
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"task\":\"text_generation\",\"target\":\"cpu\""));
        assert!(json.contains("\"qualified_paths\":[]"));
        assert!(!json.contains("cuda"));
        assert!(!json.contains("metal"));
    }
}
