//! Phase 2 precondition (HOL plan): two generations running at the same
//! time on one loaded GGUF model produce exactly the serial outputs. This is
//! the empirical half of the `unsafe impl Sync for LlamaBackendInner`
//! review: each request creates its own context and sampler and only reads
//! the shared, immutable model and vocabulary.
//!
//! Requires: models/qwen2.5-0.5b-instruct-q4_k_m.gguf (skips when absent).

#[cfg(feature = "gguf")]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use gg_core::engine::gguf::{GgufConfig, GgufGenerator};
    use gg_core::engine::{InferenceConfig, InferenceInput, InferenceOutput, Model};

    const PROMPTS: [&str; 2] = [
        "Name three primary colors.",
        "Count from one to ten in words.",
    ];

    fn load() -> Option<Arc<GgufGenerator>> {
        let path = Path::new("../models/qwen2.5-0.5b-instruct-q4_k_m.gguf");
        if !path.exists() {
            eprintln!("Skipping: model not found at {path:?}");
            return None;
        }
        let config = GgufConfig {
            n_ctx: 512,
            n_threads: 2,
            n_gpu_layers: 0,
        };
        GgufGenerator::load("qwen".into(), path, &config)
            .ok()
            .map(Arc::new)
    }

    /// Greedy and seeded: identical inputs must give identical text.
    fn greedy() -> InferenceConfig {
        InferenceConfig {
            max_tokens: Some(48),
            temperature: 0.0,
            top_k: 1,
            ..Default::default()
        }
    }

    fn run(model: &GgufGenerator, prompt: &str) -> String {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let input = InferenceInput::Text(prompt.into());
        match rt.block_on(model.infer(&input, &greedy())).expect("infer") {
            InferenceOutput::Generation(g) => g.text,
            other => panic!("unexpected output {other:?}"),
        }
    }

    #[test]
    fn concurrent_generations_match_serial_outputs() {
        let Some(model) = load() else { return };
        let serial: Vec<String> = PROMPTS.iter().map(|p| run(&model, p)).collect();
        for round in 0..3 {
            let handles: Vec<_> = PROMPTS
                .iter()
                .map(|p| {
                    let (model, prompt) = (Arc::clone(&model), p.to_string());
                    std::thread::spawn(move || run(&model, &prompt))
                })
                .collect();
            let concurrent: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            assert_eq!(
                concurrent, serial,
                "round {round}: concurrent output diverged"
            );
        }
        assert!(serial.iter().all(|t| !t.is_empty()));
    }
}
