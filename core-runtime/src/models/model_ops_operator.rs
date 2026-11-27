//! Authenticated operator mutation entry points for conservative cold switching.
//! Standalone embedded load/unload remain separately exposed; no public ABI change.

use std::sync::Arc;
use std::time::Duration;

use super::{unload_model, LoadedModel, ModelOpError};
use crate::models::{ModelLifecycle, ModelLoader};

/// Operator load: serialized authority and source identity checked by the
/// canonical preparer; the native backend loads off the async runtime.
pub async fn load_model_operated(
    lifecycle: &Arc<ModelLifecycle>,
    loader: &Arc<ModelLoader>,
    relative_path: &str,
    model_id: Option<String>,
    workers: usize,
    wait: Duration,
) -> Result<LoadedModel, ModelOpError> {
    use crate::models::transition::{load_operated, CanonicalPreparer, Preparer};
    let prep: Arc<dyn Preparer> = Arc::new(CanonicalPreparer {
        loader: Arc::clone(loader),
        workers,
    });
    load_operated(lifecycle, &prep, relative_path, model_id, wait).await
}

/// Operator unload: serialize mutations and retain the #219 safe unload path.
pub async fn unload_model_operated(
    lifecycle: &Arc<ModelLifecycle>,
    model_id: &str,
    wait: Duration,
) -> Result<(), ModelOpError> {
    let _authority = lifecycle
        .begin_operation_within(wait)
        .await
        .ok_or(ModelOpError::Busy)?;
    unload_model(lifecycle, model_id).await
}
