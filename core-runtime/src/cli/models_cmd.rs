// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Models CLI subcommand: list.
//!
//! Connects to a running GG-CORE server via IPC and displays
//! the list of currently loaded models.

use crate::cli::CliIpcClient;
use crate::ipc::protocol::ModelsListResponse;

/// Run `models list`, connecting via IPC at `socket_path`.
///
/// Returns exit code: 0 on success, 3 on connection failure.
pub async fn run_list(socket_path: &str) -> i32 {
    let client = CliIpcClient::new(socket_path.to_string());
    match client.get_models().await {
        Ok(response) => {
            print_models(&response);
            0
        }
        Err(e) => {
            eprintln!("Error connecting to GG-CORE server: {}", e);
            eprintln!("Is the server running? Check GG_CORE_SOCKET_PATH.");
            3
        }
    }
}

/// Format and print a `ModelsListResponse` to stdout.
pub fn print_models(response: &ModelsListResponse) {
    if response.models.is_empty() {
        println!("No models currently loaded.");
        return;
    }

    println!(
        "{:<30} {:<12} {:>14} {:>12} {:>12}",
        "NAME", "STATE", "MEMORY (MB)", "REQUESTS", "AVG LAT (ms)"
    );
    println!("{}", "-".repeat(84));

    for m in &response.models {
        let memory_mb = m.memory_bytes / (1024 * 1024);
        println!(
            "{:<30} {:<12} {:>14} {:>12} {:>12.1}",
            truncate(&m.name, 29),
            truncate(&m.state, 11),
            memory_mb,
            m.request_count,
            m.avg_latency_ms,
        );
    }

    let total_mb = response.total_memory_bytes / (1024 * 1024);
    println!("{}", "-".repeat(84));
    println!("Total memory: {} MB  |  {} model(s) loaded", total_mb, response.models.len());
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::ModelInfo;

    fn make_model(name: &str, state: &str, memory_mb: u64, requests: u64, lat: f64) -> ModelInfo {
        ModelInfo {
            handle_id: 1,
            name: name.to_string(),
            format: "gguf".to_string(),
            size_bytes: memory_mb * 1024 * 1024,
            memory_bytes: memory_mb * 1024 * 1024,
            state: state.to_string(),
            request_count: requests,
            avg_latency_ms: lat,
            loaded_at: "2026-02-22T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_print_models_empty() {
        let response = ModelsListResponse { models: vec![], total_memory_bytes: 0 };
        // Smoke-test: must not panic.
        print_models(&response);
    }

    #[test]
    fn test_print_models_with_entries() {
        let response = ModelsListResponse {
            models: vec![
                make_model("qwen2.5-0.5b", "ready", 512, 100, 42.5),
                make_model("phi-3-mini", "loading", 1024, 0, 0.0),
            ],
            total_memory_bytes: 1536 * 1024 * 1024,
        };
        // Smoke-test: must not panic.
        print_models(&response);
    }

    #[test]
    fn test_print_models_truncates_long_names() {
        let long_name = "a".repeat(50);
        let response = ModelsListResponse {
            models: vec![make_model(&long_name, "ready", 256, 10, 5.0)],
            total_memory_bytes: 256 * 1024 * 1024,
        };
        // Must not panic with long model name.
        print_models(&response);
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        assert_eq!(truncate("hello world", 5), "hello");
    }

    #[tokio::test]
    async fn test_run_list_connection_failure_returns_3() {
        let code = run_list("/nonexistent/gg-core-test.sock").await;
        assert_eq!(code, 3, "should return exit code 3 on connection failure");
    }
}
