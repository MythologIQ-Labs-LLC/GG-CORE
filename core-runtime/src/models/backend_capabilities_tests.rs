//! Model-free tests for feature-to-backend capability declarations.
//! Native CUDA/Metal execution remains unverified until hardware qualification.

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

#[test]
fn simulated_native_compile_matrix_exercises_both_gpu_branches_on_cpu_ci() {
    // These are synthetic compile-feature tests, NOT installed GPU proofs.
    let combinations = [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (true, false, true),
        (true, true, true),
    ];
    for (has_gguf, cuda, metal_on_macos) in combinations {
        let result = compiled_for_features(CompiledFeatures {
            gguf: has_gguf,
            onnx: true,
            gguf_cuda: cuda,
            gguf_metal_on_macos: metal_on_macos,
        });
        let gguf = result.iter().find(|b| b.backend == BackendKind::Gguf);
        assert_eq!(gguf.is_some(), has_gguf);
        if let Some(gguf) = gguf {
            let path = |target| ServingPath::new(InferenceTask::TextGeneration, target);
            assert!(gguf.compiled_paths.contains(&path(ExecutionTarget::Cpu)));
            assert_eq!(gguf.compiled_paths.contains(&path(ExecutionTarget::Cuda)), cuda);
            assert_eq!(
                gguf.compiled_paths.contains(&path(ExecutionTarget::Metal)),
                metal_on_macos
            );
            assert_eq!(gguf.compiled_paths.len(), 1 + usize::from(cuda) + usize::from(metal_on_macos));
        }
        let onnx = result.iter().find(|b| b.backend == BackendKind::Onnx).unwrap();
        assert!(onnx.compiled_paths.iter().all(|path| path.target == ExecutionTarget::Cpu));
        assert!(onnx.compiled_paths.iter().all(|path| path.task != InferenceTask::TextGeneration));
        assert!(result.iter().all(|backend| backend.qualified_paths.is_empty()));
    }
}

#[test]
fn synthetic_cuda_metal_flags_without_gguf_never_invent_inference() {
    let result = compiled_for_features(CompiledFeatures {
        gguf: false,
        onnx: false,
        gguf_cuda: true,
        gguf_metal_on_macos: true,
    });
    assert!(result.is_empty());
}
