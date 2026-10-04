//! Tests for serve-argument parsing and preload config (B-41, issue #106).

use super::*;
use std::sync::Mutex;

// GG_CORE_PRELOAD_MODELS is process-global; serialize the tests that touch it
// (same pattern as cli::tests ENV_LOCK).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn args(list: &[&str]) -> Vec<String> {
    // Real argv shape: [bin, "serve", rest...]
    let mut v = vec!["gg-core-cli".to_string(), "serve".to_string()];
    v.extend(list.iter().map(|s| s.to_string()));
    v
}

#[test]
fn no_flags_and_no_env_means_no_preload() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var(PRELOAD_MODELS_ENV);
    assert_eq!(parse_serve_models(&args(&[])).unwrap(), vec![]);
}

#[test]
fn repeatable_model_flags_with_paired_ids() {
    let models = parse_serve_models(&args(&[
        "--model",
        "models/a.gguf",
        "--model",
        "models/b.gguf",
        "--model-id",
        "alpha",
    ]))
    .unwrap();
    assert_eq!(
        models,
        vec![
            ("models/a.gguf".to_string(), Some("alpha".to_string())),
            ("models/b.gguf".to_string(), None),
        ]
    );
}

#[test]
fn more_ids_than_models_is_rejected() {
    let err = parse_serve_models(&args(&["--model-id", "x"])).unwrap_err();
    assert!(err.contains("More --model-id"));
}

#[test]
fn missing_flag_value_is_rejected() {
    assert!(parse_serve_models(&args(&["--model"])).is_err());
    assert!(parse_serve_models(&args(&["--model-id"])).is_err());
}

#[test]
fn unknown_argument_is_rejected() {
    let err = parse_serve_models(&args(&["--socket", "/x"])).unwrap_err();
    assert!(err.contains("Unknown serve argument"));
}

#[test]
fn env_fallback_parses_comma_separated_paths() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var(PRELOAD_MODELS_ENV, "models/a.gguf, models/b.gguf,");
    let models = parse_serve_models(&args(&[])).unwrap();
    std::env::remove_var(PRELOAD_MODELS_ENV);
    assert_eq!(
        models,
        vec![
            ("models/a.gguf".to_string(), None),
            ("models/b.gguf".to_string(), None),
        ]
    );
}

#[test]
fn explicit_flags_win_over_env() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var(PRELOAD_MODELS_ENV, "models/env.gguf");
    let models = parse_serve_models(&args(&["--model", "models/flag.gguf"])).unwrap();
    std::env::remove_var(PRELOAD_MODELS_ENV);
    assert_eq!(models, vec![("models/flag.gguf".to_string(), None)]);
}

#[test]
fn daemon_config_requires_model_for_readiness() {
    let config = load_config();
    assert!(config.health.require_model_loaded);
}
