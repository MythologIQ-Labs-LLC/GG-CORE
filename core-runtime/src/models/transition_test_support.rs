//! Test support for the cold-switch coordinator: fake weights and a fake
//! preparer that reserves budget exactly like the canonical loader.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceEngine,
    InferenceError as EngineError, InferenceInput, InferenceOutput, Model,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::model_ops::{ModelOpError, PreparedModel};
use crate::models::registry::ModelRegistry;

pub(super) struct Weights(pub(super) usize);

#[async_trait::async_trait]
impl Model for Weights {
    fn model_id(&self) -> &str {
        "w"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        self.0
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

/// One fake model file.
#[derive(Clone)]
pub(super) struct Target {
    /// On-disk size: what preflight reports and the load reserves.
    pub(super) size: u64,
    /// The backend's reported footprint after load.
    pub(super) memory: usize,
    pub(super) sha: String,
    pub(super) preflight_err: Option<String>,
    pub(super) prepare_err: Option<String>,
    pub(super) ready_err: Option<String>,
    /// Whether the fake keeps holding instances it prepares (an in-flight
    /// request stand-in) until `release` is called.
    pub(super) held: bool,
}

pub(super) fn target(bytes: usize, sha: &str) -> Target {
    Target {
        size: bytes as u64,
        memory: bytes,
        sha: sha.into(),
        preflight_err: None,
        prepare_err: None,
        ready_err: None,
        held: true,
    }
}

/// Fake preparer keyed by path. It keeps a clone of every model it prepares
/// in `holders`, standing in for an in-flight request that holds it.
#[derive(Default)]
pub(super) struct Fake {
    pub(super) targets: Mutex<HashMap<String, Target>>,
    pub(super) holders: Mutex<HashMap<String, Arc<dyn Model>>>,
}

impl Fake {
    pub(super) fn with(entries: &[(&str, Target)]) -> Arc<Self> {
        let fake = Self::default();
        for (path, t) in entries {
            fake.set(path, t.clone());
        }
        Arc::new(fake)
    }
    pub(super) fn set(&self, path: &str, t: Target) {
        self.targets.lock().unwrap().insert(path.into(), t);
    }
    fn get(&self, path: &str) -> Result<Target, ModelOpError> {
        let found = self.targets.lock().unwrap().get(path).cloned();
        found.ok_or_else(|| ModelOpError::Backend(format!("no such model: {path}")))
    }
    /// Stop holding the most recent instance prepared from `path`.
    pub(super) fn release(&self, path: &str) {
        self.holders.lock().unwrap().remove(path);
    }
}

impl Preparer for Fake {
    fn preflight(&self, path: &str) -> Result<TargetFacts, ModelOpError> {
        let t = self.get(path)?;
        match t.preflight_err {
            Some(e) => Err(ModelOpError::Backend(e)),
            None => Ok(TargetFacts { size_bytes: t.size }),
        }
    }
    fn prepare_sourced(
        &self,
        path: &str,
        id: Option<String>,
        limits: Option<&ResourceLimits>,
    ) -> Result<(PreparedModel, ModelSource), ModelOpError> {
        let t = self.get(path)?;
        if let Some(e) = t.prepare_err {
            return Err(ModelOpError::Backend(e));
        }
        let guard = match limits {
            Some(l) => Some(l.reserve_resident(t.size as usize).map_err(|e| {
                ModelOpError::Lifecycle(LifecycleError::ResourceExhausted(e.to_string()))
            })?),
            None => None,
        };
        let model: Arc<dyn Model> = Arc::new(Weights(t.memory));
        if t.held {
            self.holders
                .lock()
                .unwrap()
                .insert(path.into(), Arc::clone(&model));
        }
        let id = id.unwrap_or_else(|| path.into());
        let source = ModelSource {
            relative_path: path.into(),
            sha256: t.sha.clone(),
        };
        Ok((PreparedModel::for_test(&id, model, guard), source))
    }
    fn verify_ready(&self, model: &Arc<dyn Model>) -> Result<(), String> {
        let found = self
            .targets
            .lock()
            .unwrap()
            .values()
            .find(|t| t.memory == model.memory_usage())
            .cloned();
        found.and_then(|t| t.ready_err).map_or(Ok(()), Err)
    }
}

/// A budgeted lifecycle with `total` bytes.
pub(super) fn lifecycle(total: usize) -> (Arc<ModelLifecycle>, ResourceLimits) {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 10,
        max_total_memory: total,
        max_concurrent: 2,
    });
    let registry = Arc::new(ModelRegistry::new());
    let engine = Arc::new(InferenceEngine::new(4096));
    let lc = ModelLifecycle::new(registry, engine).with_resource_limits(limits.clone());
    (Arc::new(lc), limits)
}

/// Load `path` as `id` through the operator path (source recorded) and
/// drop the fake's hold, so only the lifecycle holds it.
pub(super) async fn operator_load(
    lc: &Arc<ModelLifecycle>,
    fake: &Arc<Fake>,
    path: &str,
    id: &str,
) -> ModelHandle {
    let handle = operator_load_held(lc, fake, path, id).await;
    fake.release(path);
    handle
}

/// [`operator_load`] that keeps the fake's hold: an in-flight request.
pub(super) async fn operator_load_held(
    lc: &Arc<ModelLifecycle>,
    fake: &Arc<Fake>,
    path: &str,
    id: &str,
) -> ModelHandle {
    let prep: Arc<dyn Preparer> = Arc::clone(fake) as Arc<dyn Preparer>;
    match load_checked(lc, &prep, path, id).await {
        Ok(handle) => handle,
        Err(_) => panic!("operator load of {path} failed"),
    }
}

pub(super) fn request(
    from: &str,
    handle: ModelHandle,
    to_path: &str,
    to: &str,
) -> ColdSwitchRequest {
    ColdSwitchRequest {
        from_id: from.into(),
        expected_handle: handle,
        to_path: to_path.into(),
        to_id: to.into(),
        release_wait: Duration::from_secs(2),
    }
}

/// Spawn a switch and await its outcome.
pub(super) async fn switch(
    lc: &Arc<ModelLifecycle>,
    fake: &Arc<Fake>,
    req: ColdSwitchRequest,
) -> SwitchOutcome {
    let prep: Arc<dyn Preparer> = Arc::clone(fake) as Arc<dyn Preparer>;
    match spawn_cold_switch(Arc::clone(lc), prep, req) {
        Ok(handle) => join_switch(handle).await,
        Err(refusal) => SwitchOutcome::Refused(refusal),
    }
}
