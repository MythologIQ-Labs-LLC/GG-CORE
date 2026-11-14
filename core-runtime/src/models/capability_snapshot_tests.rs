//! Capability snapshot: separation of compiled, loaded, hardware, qualified
//! and operator facts; truthful loaded paths; real configuration values.

use std::sync::Arc;

use super::*;
use crate::engine::{
    GenerationResult, InferenceConfig, InferenceError as EngineError, InferenceInput,
    InferenceOutput,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::{ModelMetadata, ModelRegistry};

/// A model of no known backend type declaring `caps`.
struct Declares(&'static [InferenceCapability]);

#[async_trait::async_trait]
impl Model for Declares {
    fn model_id(&self) -> &str {
        "declares"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        self.0
    }
    fn memory_usage(&self) -> usize {
        100
    }
    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, EngineError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: String::new(),
            tokens_generated: 0,
            finish_reason: crate::engine::FinishReason::MaxTokens,
        }))
    }
    async fn unload(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn rig(limits: Option<ResourceLimits>) -> (ModelLifecycle, Arc<InferenceEngine>) {
    let engine = Arc::new(InferenceEngine::new(4096));
    let lifecycle = ModelLifecycle::new(Arc::new(ModelRegistry::new()), Arc::clone(&engine));
    let lifecycle = match limits {
        Some(l) => lifecycle.with_resource_limits(l),
        None => lifecycle,
    };
    (lifecycle, engine)
}

fn meta(name: &str) -> ModelMetadata {
    ModelMetadata {
        name: name.into(),
        size_bytes: 100,
    }
}

#[tokio::test]
async fn unknown_backend_capabilities_are_declared_only_never_serving() {
    let (lc, engine) = rig(None);
    let caps: &'static [InferenceCapability] = &[
        InferenceCapability::TextGeneration,
        InferenceCapability::NamedEntityRecognition,
    ];
    let handle = lc
        .load("m".into(), meta("m"), Arc::new(Declares(caps)))
        .await
        .unwrap();
    let snap = capability_snapshot(&lc, &engine, true).await;
    assert_eq!(snap.loaded.len(), 1);
    let m = &snap.loaded[0];
    assert_eq!((m.model_id.as_str(), m.handle_id), ("m", handle.id()));
    assert_eq!(m.backend, None);
    assert!(
        m.paths.is_empty(),
        "no serving claim without a known backend"
    );
    let cpu_gen = ServingPath::new(InferenceTask::TextGeneration, ExecutionTarget::Cpu);
    assert_eq!(m.declared_only_paths, vec![cpu_gen], "NER has no task cell");
}

#[tokio::test]
async fn hardware_and_compiled_paths_never_become_qualified_or_loaded() {
    let (lc, engine) = rig(None);
    let snap = capability_snapshot(&lc, &engine, true).await;
    assert!(snap.loaded.is_empty());
    assert!(snap.qualified.is_empty());
    assert!(snap.hardware.logical_cpus >= 1);
    assert_eq!(snap.hardware.accelerators, None, "not reported");
    assert_eq!(snap.compiled, compiled_backend_capabilities());
    assert_eq!(snap.schema_version, CAPABILITY_SCHEMA_VERSION);
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains("\"target\":\"cuda\"") && !json.contains("\"target\":\"metal\""));
}

#[tokio::test]
async fn operator_constraints_come_from_the_running_configuration() {
    let (lc, engine) = rig(None);
    let open = capability_snapshot(&lc, &engine, false).await.operator;
    assert!(
        !open.auth_required,
        "read from the handler config, not hardcoded"
    );
    assert_eq!(open.budget, None, "no budget configured");
    assert!(open.offline_only && !open.network_listener);

    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 64,
        max_total_memory: 4096,
        max_concurrent: 2,
    });
    let (lc, engine) = rig(Some(limits));
    let budgeted = capability_snapshot(&lc, &engine, true).await.operator;
    assert!(budgeted.auth_required);
    let budget = budgeted.budget.expect("configured");
    assert_eq!(budget.max_total_memory, 4096);
}

#[tokio::test]
async fn an_engine_model_without_a_lifecycle_handle_is_omitted() {
    let (lc, engine) = rig(None);
    let caps: &'static [InferenceCapability] = &[InferenceCapability::Embedding];
    let handle = crate::models::ModelHandle::new(9_999);
    engine
        .register_model("orphan".into(), handle, Arc::new(Declares(caps)))
        .await;
    let snap = capability_snapshot(&lc, &engine, true).await;
    assert!(
        snap.loaded.is_empty(),
        "not registered through the lifecycle"
    );
}
