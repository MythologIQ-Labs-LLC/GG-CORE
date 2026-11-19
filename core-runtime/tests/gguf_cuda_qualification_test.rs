//! Native CUDA GGUF qualification (#216): real offload, not detection.
//!
//! Built only with `gguf-cuda`; run with `--test-threads=1` so timings do not
//! overlap. Requires the hash-pinned Qwen2.5-0.5B fixture and a CUDA device.
//! Results are `GPUQUAL` lines; assertions cover only behavior, never speed.

#![cfg(feature = "gguf-cuda")]

#[path = "gguf_cuda_qual/common.rs"]
mod common;

use std::sync::Arc;

use common::{greedy, load, report, stream, vram_mib, ALL_LAYERS};
use gg_core::engine::{InferenceConfig, InferenceInput, InferenceOutput, Model};

const PROMPTS: [&str; 3] = [
    "Name three primary colors.",
    "Count from one to ten in words.",
    "What is the capital of France? Answer in one sentence.",
];

fn infer(
    model: &gg_core::engine::gguf::GgufGenerator,
    prompt: &str,
    cfg: &InferenceConfig,
) -> String {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let input = InferenceInput::Text(prompt.into());
    match rt.block_on(model.infer(&input, cfg)).expect("infer") {
        InferenceOutput::Generation(g) => g.text,
        other => panic!("unexpected output {other:?}"),
    }
}

fn common_prefix_chars(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

#[test]
fn q1_gpu_greedy_output_characterization_against_cpu() {
    let cpu = load(0, 1024).expect("cpu load");
    let gpu = load(ALL_LAYERS, 1024).expect("gpu load");
    for (i, prompt) in PROMPTS.iter().enumerate() {
        let (c, g) = (
            infer(&cpu, prompt, &greedy(64)),
            infer(&gpu, prompt, &greedy(64)),
        );
        let exact = c == g;
        report(&format!("correctness.p{i}.exact"), exact);
        report(
            &format!("correctness.p{i}.parity_claim"),
            if exact { "EXACT" } else { "NOT_QUALIFIED_DIFFERENT" },
        );
        report(
            &format!("correctness.p{i}.prefix_chars"),
            common_prefix_chars(&c, &g),
        );
        report(&format!("correctness.p{i}.cpu_chars"), c.chars().count());
        report(&format!("correctness.p{i}.gpu"), format!("{g:?}"));
        assert!(!g.trim().is_empty(), "GPU produced no text for {prompt:?}");
    }
    report("correctness.release_qualification", "NOT_QUALIFIED (tolerance not approved)");
}

fn measure(layers: u32, label: &str, runs: usize) {
    let model = Arc::new(load(layers, 1024).expect("load"));
    let _warm = stream(Arc::clone(&model), PROMPTS[1], greedy(16), None);
    for run in 0..runs {
        let r = stream(Arc::clone(&model), PROMPTS[1], greedy(128), None);
        assert!(r.result.is_ok(), "{label} run {run}: {:?}", r.result);
        let key = format!("perf.{label}.run{run}");
        report(&format!("{key}.tokens"), r.tokens);
        report(&format!("{key}.ttft_ms"), r.ttft.as_millis());
        report(
            &format!("{key}.decode_tps"),
            format!("{:.1}", r.decode_tps()),
        );
    }
}

#[test]
fn q2_first_token_latency_and_throughput_cpu_vs_gpu() {
    measure(0, "cpu", 5);
    measure(ALL_LAYERS, "gpu", 5);
}

#[test]
fn q3_gpu_stream_cancellation_stops_and_model_stays_usable() {
    let model = Arc::new(load(ALL_LAYERS, 1024).expect("load"));
    let cancelled = stream(Arc::clone(&model), PROMPTS[1], greedy(256), Some(8));
    report("cancel.tokens_after_stop_at_8", cancelled.tokens);
    report("cancel.stop_observed", cancelled.stop_observed);
    report("cancel.backend_result", format!("{:?}", cancelled.result));
    assert!(cancelled.tokens >= 8, "model ended before reaching the stop threshold");
    assert!(cancelled.stop_observed, "backend never observed the cancellation predicate");
    // generate_stream currently returns Ok(()) after stopping. Its Result alone
    // cannot distinguish cancellation from EOS or max-tokens completion.
    assert!(cancelled.result.is_ok(), "backend failed: {:?}", cancelled.result);
    assert!(cancelled.tokens < 256, "stream did not stop before the max-token limit");
    let next = stream(model, PROMPTS[0], greedy(16), None);
    assert!(
        next.result.is_ok() && next.tokens > 0,
        "model unusable after cancel"
    );
}

#[test]
fn q4_cpu_gpu_transition_vram_characterization_only() {
    let base = vram_mib();
    report("vram.baseline_mib", format!("{base:?}"));
    report(
        "vram.telemetry",
        if base.is_some() {
            "AGGREGATE_DEVICE_ONLY"
        } else {
            "NOT_QUALIFIED_MISSING_PINNED_GPU_UUID_OR_NVSMI"
        },
    );
    for cycle in 0..3 {
        let gpu = load(ALL_LAYERS, 1024).expect("gpu load");
        report(
            &format!("vram.cycle{cycle}.gpu_loaded_mib"),
            format!("{:?}", vram_mib()),
        );
        assert!(!infer(&gpu, PROMPTS[0], &greedy(16)).is_empty());
        drop(gpu);
        report(
            &format!("vram.cycle{cycle}.gpu_dropped_mib"),
            format!("{:?}", vram_mib()),
        );
        let cpu = load(0, 1024).expect("cpu load");
        assert!(!infer(&cpu, PROMPTS[0], &greedy(16)).is_empty());
        report(
            &format!("vram.cycle{cycle}.cpu_loaded_mib"),
            format!("{:?}", vram_mib()),
        );
    }
    report(
        "vram.release_qualification",
        "NOT_QUALIFIED (no process attribution or approved recovery bound)",
    );
}

#[test]
fn q5_concurrent_gpu_generations_match_serial() {
    let model = Arc::new(load(ALL_LAYERS, 1024).expect("load"));
    let serial: Vec<String> = PROMPTS
        .iter()
        .map(|p| infer(&model, p, &greedy(48)))
        .collect();
    for round in 0..3 {
        let handles: Vec<_> = PROMPTS
            .iter()
            .map(|p| {
                let (m, p) = (Arc::clone(&model), p.to_string());
                std::thread::spawn(move || infer(&m, &p, &greedy(48)))
            })
            .collect();
        let got: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        report(
            &format!("concurrent.round{round}.matches_serial"),
            got == serial,
        );
        assert_eq!(
            got, serial,
            "GPU concurrency round {round} diverged from serial greedy output"
        );
    }
}
