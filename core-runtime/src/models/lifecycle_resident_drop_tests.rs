//! Model-free regression: destructor failures must not escape the final
//! fallback release path or silently credit an unreleased native budget.

use std::sync::Arc;

use super::*;
use crate::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
    InferenceOutput, Model,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};

struct TearDownModel {
    panic_on_drop: bool,
}

impl Drop for TearDownModel {
    fn drop(&mut self) {
        if self.panic_on_drop {
            panic!("synthetic backend destructor panic");
        }
    }
}

#[async_trait::async_trait]
impl Model for TearDownModel {
    fn model_id(&self) -> &str {
        "teardown"
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }

    fn memory_usage(&self) -> usize {
        100
    }

    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: String::new(),
            tokens_generated: 0,
            finish_reason: crate::engine::FinishReason::Stop,
        }))
    }

    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn orphaned_release_destructor_panic_is_isolated_and_budget_stays_charged() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 100,
        max_total_memory: 1_000,
        max_concurrent: 1,
    });
    let guard = limits.reserve_resident(600).expect("faulty model budget");
    let model: Arc<dyn Model> = Arc::new(TearDownModel {
        panic_on_drop: true,
    });
    let (done, watch) = release_channel();
    let release = PendingRelease {
        resident: Some(Resident::new(model, Some(guard))),
        done,
    };

    // Before this change, PendingRelease::drop leaked the panic to its caller.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(release);
    }));
    assert!(
        outcome.is_ok(),
        "failed cleanup unwound out of release supervisor"
    );
    assert!(!watch.released(), "failed destructor cannot signal release");
    assert_eq!(
        limits.current_memory(),
        600,
        "native destructor panic must not credit resident memory"
    );
    assert!(
        limits.reserve_resident(500).is_err(),
        "incorrectly freed memory admitted overlapping work"
    );

    // A separate healthy release still returns its budget after the failure.
    let healthy_guard = limits.reserve_resident(100).expect("healthy budget");
    let healthy: Arc<dyn Model> = Arc::new(TearDownModel {
        panic_on_drop: false,
    });
    let (done, _watch) = release_channel();
    drop(PendingRelease {
        resident: Some(Resident::new(healthy, Some(healthy_guard))),
        done,
    });
    assert_eq!(limits.current_memory(), 600, "healthy cleanup was blocked");
}

#[test]
fn fallback_with_live_external_holder_retains_the_charge() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 100,
        max_total_memory: 1_000,
        max_concurrent: 1,
    });
    let guard = limits.reserve_resident(400).expect("native charge");
    let external: Arc<dyn Model> = Arc::new(TearDownModel {
        panic_on_drop: false,
    });
    let (done, watch) = release_channel();
    drop(PendingRelease {
        resident: Some(Resident::new(Arc::clone(&external), Some(guard))),
        done,
    });
    assert_eq!(limits.current_memory(), 400);
    assert!(!watch.released());
    drop(external);
    assert_eq!(
        limits.current_memory(),
        400,
        "supervisor loss must retain the budget rather than credit it early"
    );
}
