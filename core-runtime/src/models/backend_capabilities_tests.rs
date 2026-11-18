//! Model-free tests for feature-to-backend capability declarations.
//! Native CUDA/Metal execution remains unverified until hardware qualification.

use super::*;

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
