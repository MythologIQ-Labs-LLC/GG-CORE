//! Admission fence for one registered model (#153 option 2, S9).
//!
//! A cold switch that drains A before unloading it must stop new requests
//! from taking a lease on A while it counts the leases still out, without
//! disturbing requests already running. The fence lives beside the model
//! store: it is raised under the store's write lock and read under the same
//! read lock that clones a model `Arc`, so a lease can never slip between
//! "fence raised" and "leases counted". Waiters keep their own deadline and
//! cancellation and never wait longer than their request allows. Dropping
//! the fence, including when the switch task is aborted, reopens admissions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::Notify;

use super::{InferenceEngine, InferenceError};
use crate::engine::{Model, StopControl};

/// Fenced model ids and the wake-up for their waiters. Taken only while the
/// model store lock (read or write) is held, so it never orders against it.
pub(super) type Fences = Arc<Mutex<HashMap<String, Arc<Notify>>>>;

/// Longest a waiter sleeps before re-checking the fence; a wake-up missed
/// between dropping the store lock and awaiting costs at most this.
const RECHECK: Duration = Duration::from_millis(20);

/// A raised fence. Dropping it reopens admissions and wakes every waiter.
pub(crate) struct AdmissionFence {
    fences: Fences,
    model_id: String,
}

impl Drop for AdmissionFence {
    fn drop(&mut self) {
        if let Some(notify) = lock(&self.fences).remove(&self.model_id) {
            notify.notify_waiters();
        }
    }
}

fn lock(fences: &Fences) -> MutexGuard<'_, HashMap<String, Arc<Notify>>> {
    // The map is plain bookkeeping; poisoned data is still consistent.
    fences.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_loaded(model_id: &str) -> InferenceError {
    InferenceError::ModelNotLoaded(model_id.to_string())
}

impl InferenceEngine {
    /// Raise the fence for `model_id` and count its outstanding `Arc` clones
    /// at that instant, both under the store's write lock. `None` when the
    /// model is not registered or is already fenced. The count includes this
    /// engine's own store reference.
    pub(crate) async fn fence_admissions(&self, model_id: &str) -> Option<(AdmissionFence, usize)> {
        let models = self.models.write().await;
        let model = models.get(model_id)?;
        let mut fences = lock(&self.fences);
        if fences.contains_key(model_id) {
            return None;
        }
        fences.insert(model_id.to_string(), Arc::new(Notify::new()));
        let fence = AdmissionFence {
            fences: Arc::clone(&self.fences),
            model_id: model_id.to_string(),
        };
        Some((fence, Arc::strong_count(model)))
    }

    /// Outstanding `Arc` clones of `model_id`, this engine's own included.
    pub(crate) async fn strong_count(&self, model_id: &str) -> Option<usize> {
        self.models
            .read()
            .await
            .get(model_id)
            .map(Arc::strong_count)
    }

    /// Take a lease on `model_id` (an `Arc` clone). While the model is fenced
    /// the request waits without holding any lock or lease, honouring its own
    /// deadline and cancellation; a fence lifted after an unload yields
    /// `ModelNotLoaded`, one lifted after a refused switch yields the model.
    pub(super) async fn admit(
        &self,
        model_id: &str,
        control: Option<&StopControl>,
    ) -> Result<Arc<dyn Model>, InferenceError> {
        loop {
            let notify = {
                let models = self.models.read().await;
                match lock(&self.fences).get(model_id).cloned() {
                    Some(notify) => notify,
                    None => {
                        return models
                            .get(model_id)
                            .cloned()
                            .ok_or_else(|| not_loaded(model_id))
                    }
                }
            };
            if let Some(stopped) = control.and_then(StopControl::stop_error) {
                return Err(stopped);
            }
            let _ = tokio::time::timeout(RECHECK, notify.notified()).await;
        }
    }

    /// [`Self::admit`] for native streaming threads: no reactor is required,
    /// and the stream's own stop check bounds the wait.
    #[cfg_attr(not(feature = "gguf"), allow(dead_code))]
    pub(super) fn admit_blocking(
        &self,
        model_id: &str,
        should_stop: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<Arc<dyn Model>, InferenceError> {
        loop {
            {
                let models = self.models.blocking_read();
                if !lock(&self.fences).contains_key(model_id) {
                    return models
                        .get(model_id)
                        .cloned()
                        .ok_or_else(|| not_loaded(model_id));
                }
            }
            if should_stop.is_some_and(|check| check()) {
                return Err(InferenceError::ExecutionFailed(
                    "stream cancelled or deadline exceeded".into(),
                ));
            }
            std::thread::sleep(RECHECK);
        }
    }
}

#[cfg(test)]
#[path = "inference_fence_tests.rs"]
mod tests;
