//! Bounded scheduler concurrency (HOL plan, Phase 2): `max_concurrent`
//! worker loops (1 without resource limits) share one `RequestQueue`. Every
//! request still passes `acquire_guard`, so resident weights plus N working-
//! memory reservations stay within the one checked total. Each loop
//! round-robins streaming, typed and main work so no queue starves another.

use std::sync::Arc;

use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use super::queue::{QueuedRequest, RequestQueue, TypedQueuedRequest};
use super::streaming_queue::StreamingQueuedRequest;
use crate::engine::InferenceEngine;
use crate::memory::ResourceLimits;
use crate::models::lifecycle::ModelLifecycle;
use crate::models::registry::ModelRegistry;
use crate::security::SecurityPipeline;

/// Everything a worker loop needs to execute any request kind.
pub(super) struct WorkerCtx {
    pub engine: Arc<InferenceEngine>,
    pub lifecycle: Option<Arc<ModelLifecycle>>,
    pub registry: Option<Arc<ModelRegistry>>,
    pub limits: Option<ResourceLimits>,
    pub security: Option<Arc<SecurityPipeline>>,
}

enum Work {
    Stream(StreamingQueuedRequest),
    Typed(TypedQueuedRequest),
    Main(QueuedRequest),
}

/// Queue order inside one rotation: streaming, typed, main.
const QUEUES: usize = 3;
const MAIN: usize = 2;

/// Spawn the loops. The returned handle completes once every loop has
/// observed `shutdown` and finished its current request.
pub(super) fn spawn(
    queue: Arc<RequestQueue>,
    ctx: WorkerCtx,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    let loops = ResourceLimits::worker_loops(ctx.limits.as_ref());
    let ctx = Arc::new(ctx);
    tokio::spawn(async move {
        let mut set = JoinSet::new();
        for _ in 0..loops {
            let (queue, ctx, shutdown) = (queue.clone(), ctx.clone(), shutdown.clone());
            set.spawn(async move { worker_loop(&queue, &ctx, shutdown).await });
        }
        while set.join_next().await.is_some() {}
    })
}

async fn worker_loop(queue: &RequestQueue, ctx: &WorkerCtx, shutdown: CancellationToken) {
    let mut cursor = 0;
    loop {
        // Check shutdown before taking queued work. Previously a continuously
        // ready queue prevented the shutdown branch from ever running.
        if shutdown.is_cancelled() {
            drain_cancelled(queue, &mut cursor).await;
            break;
        }
        if let Some(work) = next_ready(queue, &mut cursor).await {
            if shutdown.is_cancelled() {
                reject_shutdown(work).await;
                drain_cancelled(queue, &mut cursor).await;
                break;
            }
            run(ctx, work).await;
            continue;
        }
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                tracing::info!("worker: shutdown signal received");
                drain_cancelled(queue, &mut cursor).await;
                break;
            }
            // `None` means streaming or typed work arrived: rotate again.
            req_opt = queue.wait_and_dequeue() => {
                if let Some(request) = req_opt {
                    cursor = (MAIN + 1) % QUEUES;
                    if shutdown.is_cancelled() {
                        reject_shutdown(Work::Main(request)).await;
                        drain_cancelled(queue, &mut cursor).await;
                        break;
                    }
                    run(ctx, Work::Main(request)).await;
                }
            }
        }
    }
}

/// Do not abandon response channels at shutdown. This drains work already
/// admitted to the bounded queue without starting any additional inference.
/// In-flight native work remains owned until its blocking closure finishes.
async fn drain_cancelled(queue: &RequestQueue, cursor: &mut usize) {
    while let Some(work) = next_ready(queue, cursor).await {
        reject_shutdown(work).await;
    }
}

async fn reject_shutdown(work: Work) {
    match work {
        Work::Stream(request) => {
            request.cancelled.cancel();
            let _ = request
                .token_sender
                .end(crate::engine::StreamTerminal::Error(
                    "stream cancelled: worker shutting down".into(),
                ))
                .await;
        }
        Work::Typed(request) => {
            request
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            request.reject_unexecuted();
        }
        Work::Main(request) => {
            request.cancel();
            request.reject_unexecuted();
        }
    }
}

/// Take the first ready request, starting at `cursor`; on success the next
/// rotation starts after the queue that was served.
async fn next_ready(queue: &RequestQueue, cursor: &mut usize) -> Option<Work> {
    for offset in 0..QUEUES {
        let kind = (*cursor + offset) % QUEUES;
        if let Some(work) = take(queue, kind).await {
            *cursor = (kind + 1) % QUEUES;
            return Some(work);
        }
    }
    None
}

async fn take(queue: &RequestQueue, kind: usize) -> Option<Work> {
    match kind {
        0 => queue.dequeue_streaming().await.map(Work::Stream),
        1 => queue.dequeue_typed().await.map(Work::Typed),
        _ => queue.dequeue().await.map(Work::Main),
    }
}

async fn run(ctx: &WorkerCtx, work: Work) {
    let limits = ctx.limits.as_ref();
    let security = ctx.security.as_deref();
    match work {
        Work::Stream(s) => {
            let sec = ctx.security.as_ref();
            super::worker_streaming::execute(&ctx.engine, limits, sec, s).await
        }
        Work::Typed(t) => super::worker_typed::execute(&ctx.engine, limits, security, t).await,
        Work::Main(r) => {
            let (lifecycle, registry) = (ctx.lifecycle.as_deref(), ctx.registry.as_deref());
            super::worker::execute_request(&ctx.engine, lifecycle, registry, limits, security, r)
                .await
        }
    }
}

#[cfg(test)]
#[path = "worker_pool_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "worker_pool_shutdown_tests.rs"]
mod shutdown_tests;
