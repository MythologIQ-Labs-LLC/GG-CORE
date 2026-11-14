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
pub enum BackendKind {
    Gguf,
    Onnx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceTask {
    TextGeneration,
    Embedding,
    Classification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTarget {
    Cpu,
    Cuda,
    Metal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBackendCapabilities {
    pub schema_version: u16,
    pub backend: BackendKind,
    /// Loader-recognized formats, not every architecture or model family.
    pub model_formats: Vec<String>,
    /// Tasks with compiled code paths. This is NOT a packaged-support claim.
    pub compiled_tasks: Vec<InferenceTask>,
    /// Targets reachable by the current serving code, not probed devices.
    pub execution_targets: Vec<ExecutionTarget>,
    /// Whether the compiled source implements token streaming for this backend.
    pub supports_streaming: bool,
    pub supports_kv_cache: bool,
    /// No v1 cell is certified by a build-time declaration. Installed product
    /// qualification must be evaluated against pinned external evidence.
    pub qualified_tasks: Vec<InferenceTask>,
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
            compiled_tasks: vec![InferenceTask::TextGeneration],
            execution_targets: vec![ExecutionTarget::Cpu],
            supports_streaming: true,
            supports_kv_cache: true,
            qualified_tasks: Vec::new(),
        });
    }
    if cfg!(feature = "onnx") {
        backends.push(RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Onnx,
            model_formats: vec!["onnx".to_string()],
            compiled_tasks: vec![InferenceTask::Embedding, InferenceTask::Classification],
            execution_targets: vec![ExecutionTarget::Cpu],
            supports_streaming: false,
            supports_kv_cache: false,
            qualified_tasks: Vec::new(),
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
            assert_eq!(capabilities.execution_targets, vec![ExecutionTarget::Cpu]);
            assert!(capabilities.qualified_tasks.is_empty());
        }
    }

    #[test]
    fn onnx_generation_and_unsupported_bitnet_are_not_advertised() {
        let catalog = compiled_backend_capabilities();
        assert!(catalog.iter().all(|c| c.backend != BackendKind::Onnx
            || !c.compiled_tasks.contains(&InferenceTask::TextGeneration)));
        assert!(catalog.iter().all(|c| c.backend == BackendKind::Gguf
            || c.backend == BackendKind::Onnx));
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
        assert_eq!(catalog.len(), usize::from(cfg!(feature = "gguf"))
            + usize::from(cfg!(feature = "onnx")));
    }

    #[test]
    fn schema_round_trips_without_inventing_hardware() {
        let input = RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Gguf,
            model_formats: vec!["gguf".into()],
            compiled_tasks: vec![InferenceTask::TextGeneration],
            execution_targets: vec![ExecutionTarget::Cpu],
            supports_streaming: true,
            supports_kv_cache: true,
            qualified_tasks: Vec::new(),
        };
        let json = serde_json::to_string(&input).expect("serialize");
        assert!(json.contains("\"schema_version\":1"));
        assert!(!json.contains("cuda"));
        assert!(!json.contains("metal"));
        let decoded: RuntimeBackendCapabilities =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, input);
    }
}
