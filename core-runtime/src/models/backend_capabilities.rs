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
/// GGUF CPU is always reachable with its native backend compiled. Native
/// llama.cpp CUDA and macOS Metal serving paths are reachable only with
/// their dedicated inference features. These are distinct from the `cuda`
/// and `metal` device-management features. A compiled offload path says
/// nothing about an installed model, driver, active GPU kernels, available
/// VRAM, operator settings, or release support. ONNX generation is absent.
/// `qualified_paths` remains empty pending pinned native acceptance.
pub fn compiled_backend_capabilities() -> Vec<RuntimeBackendCapabilities> {
    let mut backends = Vec::new();
    if cfg!(feature = "gguf") {
        let mut compiled_paths = vec![ServingPath::new(
            InferenceTask::TextGeneration,
            ExecutionTarget::Cpu,
        )];
        // Report reachable native inference backends, NOT installed or
        // qualified acceleration. Device-only `cuda` / `metal` do not count.
        if cfg!(feature = "gguf-cuda") {
            compiled_paths.push(ServingPath::new(
                InferenceTask::TextGeneration,
                ExecutionTarget::Cuda,
            ));
        }
        if cfg!(all(feature = "gguf-metal", target_os = "macos")) {
            compiled_paths.push(ServingPath::new(
                InferenceTask::TextGeneration,
                ExecutionTarget::Metal,
            ));
        }
        backends.push(RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Gguf,
            model_formats: vec!["gguf".to_string()],
            compiled_paths,
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
    fn compiled_offload_is_feature_exact_and_never_qualified_by_compilation() {
        let backends = compiled_backend_capabilities();
        for capabilities in &backends {
            assert_eq!(capabilities.schema_version, CAPABILITY_SCHEMA_VERSION);
            assert!(capabilities.qualified_paths.is_empty());
        }
        let gguf = backends.iter().find(|c| c.backend == BackendKind::Gguf);
        if let Some(gguf) = gguf {
            let has = |target| {
                gguf.compiled_paths
                    .contains(&ServingPath::new(InferenceTask::TextGeneration, target))
            };
            assert!(has(ExecutionTarget::Cpu));
            assert_eq!(has(ExecutionTarget::Cuda), cfg!(feature = "gguf-cuda"));
            assert_eq!(
                has(ExecutionTarget::Metal),
                cfg!(all(feature = "gguf-metal", target_os = "macos"))
            );
            assert!(gguf
                .compiled_paths
                .iter()
                .all(|p| p.task == InferenceTask::TextGeneration));
        }
    }

    #[test]
    fn gpu_device_management_alone_cannot_advertise_native_offload() {
        let backends = compiled_backend_capabilities();
        let native_cuda = backends.iter().any(|c| {
            c.backend == BackendKind::Gguf
                && c.compiled_paths.contains(&ServingPath::new(
                    InferenceTask::TextGeneration,
                    ExecutionTarget::Cuda,
                ))
        });
        let native_metal = backends.iter().any(|c| {
            c.backend == BackendKind::Gguf
                && c.compiled_paths.contains(&ServingPath::new(
                    InferenceTask::TextGeneration,
                    ExecutionTarget::Metal,
                ))
        });
        assert_eq!(native_cuda, cfg!(feature = "gguf-cuda"));
        assert_eq!(
            native_metal,
            cfg!(all(feature = "gguf-metal", target_os = "macos"))
        );
        assert!(backends.iter().all(|c| c.qualified_paths.is_empty()));
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
