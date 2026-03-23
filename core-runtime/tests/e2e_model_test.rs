//! E2E test with real GGUF model.
//!
//! Requires: models/qwen2.5-0.5b-instruct-q4_k_m.gguf

#[cfg(feature = "gguf")]
mod tests {
    use gg_core::engine::gguf::{GgufConfig, GgufGenerator};
    use gg_core::engine::{InferenceConfig, InferenceInput, InferenceOutput, GgufModel, TokenStream, ChatMessage, ChatRole};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    fn load_test_model() -> Option<GgufGenerator> {
        let model_path = Path::new("../models/qwen2.5-0.5b-instruct-q4_k_m.gguf");
        if !model_path.exists() {
            eprintln!("Skipping: model not found at {:?}", model_path);
            return None;
        }
        // 4 threads is optimal for small models like 0.5B
        // Use n_threads: 0 for auto-detect with larger models
        let config = GgufConfig { n_ctx: 512, n_threads: 4, n_gpu_layers: 0 };
        GgufGenerator::load("qwen-0.5b".to_string(), model_path, &config).ok()
    }

    #[test]
    fn e2e_load_and_generate() {
        let Some(gen) = load_test_model() else { return };
        println!("Model loaded: {}", gen.model_id());

        let input = InferenceInput::Text("What is 2 + 2? Answer briefly:".to_string());
        let mut inf_config = InferenceConfig::default();
        inf_config.max_tokens = Some(50);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(gen.infer(&input, &inf_config));

        match result {
            Ok(output) => {
                println!("\n=== INFERENCE SUCCESS ===");
                println!("Output: {:?}", output);
            }
            Err(e) => panic!("Inference failed: {:?}", e),
        }
    }

    #[test]
    fn e2e_streaming_generation() {
        let Some(gen) = load_test_model() else { return };
        println!("Model loaded for streaming: {}", gen.model_id());

        let mut inf_config = InferenceConfig::default();
        inf_config.max_tokens = Some(20);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let token_count = Arc::new(AtomicUsize::new(0));
        let count_clone = token_count.clone();

        let result = rt.block_on(async {
            let (sender, mut stream) = TokenStream::new(32);
            let prompt = "Count from 1 to 5:";

            // Spawn streaming task
            let gen_handle = tokio::task::spawn_blocking({
                move || gen.generate_stream(prompt, &inf_config, sender)
            });

            // Collect tokens
            print!("\n=== STREAMING OUTPUT ===\n");
            while let Some(chunk) = stream.next().await {
                count_clone.fetch_add(1, Ordering::SeqCst);
                print!("[tok:{}]", chunk.token);
                if chunk.is_final {
                    println!(" [DONE]");
                    break;
                }
            }

            gen_handle.await.unwrap()
        });

        let tokens = token_count.load(Ordering::SeqCst);
        println!("Streamed {} tokens", tokens);

        match result {
            Ok(()) => {
                assert!(tokens > 0, "Should have streamed at least 1 token");
                println!("=== STREAMING SUCCESS ===");
            }
            Err(e) => panic!("Streaming failed: {:?}", e),
        }
    }

    #[test]
    fn e2e_chat_messages() {
        let Some(gen) = load_test_model() else { return };
        println!("Model loaded for chat: {}", gen.model_id());

        let messages = vec![
            ChatMessage { role: ChatRole::System, content: "You are a helpful assistant.".into() },
            ChatMessage { role: ChatRole::User, content: "What is the capital of France?".into() },
        ];
        let input = InferenceInput::ChatMessages(messages);
        let mut inf_config = InferenceConfig::default();
        inf_config.max_tokens = Some(30);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(gen.infer(&input, &inf_config));

        match result {
            Ok(output) => {
                println!("\n=== CHAT INFERENCE SUCCESS ===");
                println!("Output: {:?}", output);
            }
            Err(e) => panic!("Chat inference failed: {:?}", e),
        }
    }

    #[test]
    fn e2e_performance_benchmark() {
        let Some(gen) = load_test_model() else { return };
        let threads = num_cpus::get_physical();
        println!("\n=== PERFORMANCE BENCHMARK ===");
        println!("Physical CPU cores: {}", threads);
        println!("Model: {}", gen.model_id());

        let input = InferenceInput::Text("Write a detailed essay about climate change:".to_string());
        let mut inf_config = InferenceConfig::default();
        inf_config.max_tokens = Some(100);

        let rt = tokio::runtime::Runtime::new().unwrap();

        // Warmup run
        let _ = rt.block_on(gen.infer(&input, &inf_config));

        // Timed run
        let start = Instant::now();
        let result = rt.block_on(gen.infer(&input, &inf_config));
        let elapsed = start.elapsed();

        match result {
            Ok(InferenceOutput::Generation(gen_result)) => {
                let tokens = gen_result.tokens_generated;
                let secs = elapsed.as_secs_f64();
                let tok_per_sec = tokens as f64 / secs;
                println!("Tokens generated: {}", tokens);
                println!("Time elapsed: {:.3}s", secs);
                println!("Throughput: {:.2} tok/s", tok_per_sec);
                println!("=== BENCHMARK COMPLETE ===");
            }
            Ok(_) => panic!("Unexpected output type"),
            Err(e) => panic!("Benchmark failed: {:?}", e),
        }
    }

    #[test]
    fn e2e_speculative_decoding() {
        use gg_core::engine::gguf::{GgufDraftModel, GgufTargetModel};
        use gg_core::engine::speculative::{SpeculativeConfig, SpeculativeDecoder};

        let Some(gen) = load_test_model() else { return };
        println!("\n=== SPECULATIVE DECODING TEST ===");
        println!("Model: {}", gen.model_id());

        let gen = Arc::new(gen);
        let draft = GgufDraftModel::new(gen.clone());
        let target = GgufTargetModel::new(gen.clone());

        let config = SpeculativeConfig {
            draft_tokens: 4,
            acceptance_threshold: 0.9,
            enabled: true,
        };
        let decoder = SpeculativeDecoder::new(draft, target, config);

        let rt = tokio::runtime::Runtime::new().unwrap();

        // Tokenize prompt (use backend directly for token IDs)
        let prompt = "Count: 1, 2, 3,";
        let prompt_tokens: Vec<u32> = (1..=10).collect(); // Simple test tokens

        let start = Instant::now();
        let result = rt.block_on(decoder.generate(&prompt_tokens, 20));
        let elapsed = start.elapsed();

        match result {
            Ok(tokens) => {
                println!("Generated {} tokens via speculative decoding", tokens.len());
                println!("Time: {:.3}s", elapsed.as_secs_f64());
                println!("Tokens: {:?}", tokens);
                assert!(!tokens.is_empty(), "Should generate at least 1 token");
                println!("=== SPECULATIVE SUCCESS ===");
            }
            Err(e) => panic!("Speculative decoding failed: {:?}", e),
        }
    }
}
