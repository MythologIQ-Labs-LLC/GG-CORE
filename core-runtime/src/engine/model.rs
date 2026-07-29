//! Backend-neutral model abstraction.
//!
//! The inference registry holds `Arc<dyn Model>`, so GGUF and ONNX backends
//! share one home. `Model` is the superset of the ONNX surface;
//! `infer_cancellable` defaults to `infer` for backends without per-token
//! cancellation, and `as_any` supports the streaming downcast to a concrete
//! backend (a non-streaming backend simply fails the downcast).

use crate::engine::{InferenceCapability, InferenceConfig, InferenceError};
use crate::engine::{InferenceInput, InferenceOutput};

/// A loaded inference model, independent of its file format / backend.
#[async_trait::async_trait]
pub trait Model: Send + Sync {
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> &[InferenceCapability];
    fn memory_usage(&self) -> usize;

    async fn infer(
        &self,
        input: &InferenceInput,
        config: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError>;

    /// Infer with optional per-token cancellation.
    ///
    /// Default delegates to `infer()` (ignoring the cancellation callback);
    /// backends that support per-token cancellation override this.
    async fn infer_cancellable(
        &self,
        input: &InferenceInput,
        config: &InferenceConfig,
        _is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, InferenceError> {
        self.infer(input, config).await
    }

    async fn unload(&mut self) -> Result<(), InferenceError>;

    /// Downcast support for streaming access to the concrete backend type.
    fn as_any(&self) -> &dyn std::any::Any;
}
