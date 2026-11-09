//! Real-model check of the stream stall bound on the embedded
//! `Runtime::infer_stream` path (HOL plan, Phase 1, V2).
//!
//! An embedder that stops reading must lose the stream within the stall
//! bound instead of parking the generation task until it reads again: after
//! a long pause the stream must not resume and report `Complete`.
//!
//! Requires: models/qwen2.5-0.5b-instruct-q4_k_m.gguf (skips when absent).

#[cfg(feature = "gguf")]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use gg_core::engine::gguf::{GgufConfig, GgufGenerator};
    use gg_core::engine::{InferenceConfig, StreamItem, StreamTerminal};
    use gg_core::ipc::IpcServerConfig;
    use gg_core::models::ModelHandle;
    use gg_core::{Runtime, RuntimeConfig};

    const STALL: Duration = Duration::from_millis(200);

    fn load_model() -> Option<GgufGenerator> {
        let path = Path::new("../models/qwen2.5-0.5b-instruct-q4_k_m.gguf");
        if !path.exists() {
            eprintln!("Skipping: model not found at {path:?}");
            return None;
        }
        let config = GgufConfig {
            n_ctx: 1024,
            n_threads: 4,
            n_gpu_layers: 0,
        };
        GgufGenerator::load("qwen-0.5b".into(), path, &config).ok()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unread_infer_stream_is_ended_by_the_stall_bound() {
        let Some(model) = load_model() else { return };
        let runtime = Runtime::new(RuntimeConfig {
            ipc_server: IpcServerConfig {
                stream_stall: STALL,
                ..Default::default()
            },
            ..Default::default()
        });
        runtime
            .inference_engine
            .register_model("m".into(), ModelHandle::new(1), Arc::new(model))
            .await;
        let config = InferenceConfig {
            max_tokens: Some(400),
            timeout_ms: 120_000,
            ..Default::default()
        };
        let mut stream = runtime
            .infer_stream(
                "m",
                "List the numbers from 1 to 300, one per line.",
                &config,
            )
            .expect("clean prompt admitted");

        // Read one frame so generation is known to be running, then stop
        // reading far longer than the stall bound (and than the generation).
        let first = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .expect("first frame");
        assert!(first.is_some());
        tokio::time::sleep(Duration::from_secs(20)).await;

        let (rest, terminal) = drain(&mut stream).await;
        let frames = rest + 1;
        eprintln!("frames delivered before the stall: {frames}; terminal: {terminal:?}");
        assert!(
            !matches!(terminal, Some(StreamTerminal::Complete)),
            "a stalled stream resumed and reported Complete"
        );
        assert!(frames <= 40, "only the buffered frames may be delivered");
    }

    /// Frames until the terminal (or the end of the stream).
    async fn drain(stream: &mut gg_core::engine::TokenStream) -> (usize, Option<StreamTerminal>) {
        let mut frames = 0usize;
        loop {
            match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
                Ok(Some(StreamItem::End(t))) => return (frames, Some(t)),
                Ok(Some(_)) => frames += 1,
                Ok(None) => return (frames, None),
                Err(_) => panic!("stream neither ended nor resumed"),
            }
        }
    }
}
