//! GG-CORE Runtime entry point.
//!
//! Bootstraps the sandboxed inference engine with FIPS 140-3 self-tests,
//! configuration loading, IPC listener setup, and signal handling.

mod auth_boot;
mod chat_template;
mod cli_parser;
mod runtime_init;
mod sandbox_boot;
mod version_info;

use std::process::ExitCode;

use gg_core::cli::{get_socket_path, run_health, run_liveness, run_readiness, run_status};
use gg_core::security::fips_tests;
use gg_core::Runtime;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("serve");

    match command {
        "serve" | "" => run_serve(&args).await,
        "health" => run_probe(|p| Box::pin(run_health(p))).await,
        "live" | "liveness" => run_probe(|p| Box::pin(run_liveness(p))).await,
        "ready" | "readiness" => run_probe(|p| Box::pin(run_readiness(p))).await,
        "help" | "--help" | "-h" => {
            if let Some(sub) = args.get(2) {
                cli_parser::print_command_help(sub);
            } else {
                cli_parser::print_usage();
            }
            ExitCode::SUCCESS
        }
        "version" | "--version" | "-V" => {
            let json = args.get(2).map(|s| s.as_str()) == Some("--json");
            version_info::print_version(json);
            ExitCode::SUCCESS
        }
        "status" => {
            let sp = get_socket_path();
            let json = args.get(2).map(|s| s.as_str()) == Some("--json");
            ExitCode::from(run_status(&sp, json).await as u8)
        }
        "infer" => ExitCode::from(runtime_init::run_inference(&args).await as u8),
        "embed" | "classify" => {
            use gg_core::ipc::protocol_typed::TypedTask;
            let task = if command == "embed" {
                TypedTask::Embedding
            } else {
                TypedTask::TextClassification
            };
            let rest = args.get(2..).unwrap_or_default();
            ExitCode::from(gg_core::cli::typed_cmd::run_typed(task, rest).await as u8)
        }
        "verify" => {
            eprintln!("Verify command not yet implemented. Use 'GG-CORE health'.");
            ExitCode::from(2u8)
        }
        "models" => run_models_cmd(&args).await,
        "config" => run_config_cmd(&args).await,
        _ => {
            eprintln!("Unknown command: {}", command);
            cli_parser::print_usage();
            ExitCode::FAILURE
        }
    }
}

/// Strict daemon configuration (#182): an invalid or contradictory memory
/// limit aborts startup before auth, model preload, sandbox and IPC bind.
fn daemon_config() -> Option<(gg_core::RuntimeConfig, Option<usize>)> {
    match runtime_init::load_config() {
        Ok(resolved) => Some(resolved),
        Err(e) => {
            eprintln!("serve: invalid configuration: {e}");
            None
        }
    }
}

/// Optional operator per-request ceiling, set once before any preload or
/// serving (unset: requests are bounded only by GG_CORE_MAX_TOTAL_MEMORY).
fn install_per_call_ceiling(runtime: &Runtime, ceiling: Option<usize>) {
    if let Some(ceiling) = ceiling {
        let _ = runtime.resource_limits.set_per_call_ceiling(ceiling);
    }
}

async fn run_serve(args: &[String]) -> ExitCode {
    let preload = match runtime_init::parse_serve_models(args) {
        Ok(models) => models,
        Err(e) => {
            eprintln!("serve: {}", e);
            eprintln!("Usage: GG-CORE serve [--model <path>]... [--model-id <id>]...");
            return ExitCode::FAILURE;
        }
    };
    let sandbox_mode = match sandbox_boot::sandbox_mode_from_env() {
        Ok(mode) => mode,
        Err(e) => {
            eprintln!("serve: {}", e);
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fips_tests::run_power_on_self_tests() {
        eprintln!("FIPS self-test FAILED: {}", e);
        eprintln!("Cryptographic operations disabled. Aborting startup.");
        return ExitCode::FAILURE;
    }
    eprintln!("FIPS 140-3 self-tests: PASSED");

    let Some((config, per_call_ceiling)) = daemon_config() else {
        return ExitCode::FAILURE;
    };
    // RC acceptance: fail secure on a missing credential (see auth_boot).
    match auth_boot::auth_policy(&config.auth_token) {
        Ok(policy) => eprintln!("{}", auth_boot::describe(policy, &config.auth_token)),
        Err(e) => {
            eprintln!("serve: {}", e);
            return ExitCode::FAILURE;
        }
    }
    let runtime = Runtime::new(config);
    install_per_call_ceiling(&runtime, per_call_ceiling);

    // B-41 (#106): fail-loud startup preload through the canonical load path.
    if let Err(e) = runtime_init::preload_models(&runtime, &preload).await {
        eprintln!("Startup preload FAILED: {}", e);
        return ExitCode::FAILURE;
    }

    // RC hardening (SECURITY.md Known Gap 1): apply the process sandbox after
    // preload (model files are mmapped already) and before the IPC server
    // starts. A requested sandbox that cannot apply aborts startup.
    match sandbox_boot::apply_daemon_sandbox(sandbox_mode, &runtime.config) {
        Ok(status) => eprintln!("{}", status),
        Err(e) => {
            eprintln!("Sandbox FAILED: {}", e);
            return ExitCode::FAILURE;
        }
    }

    match runtime_init::run_ipc_server(runtime).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Server error: {}", e);
            ExitCode::FAILURE
        }
    }
}

async fn run_probe<F>(f: F) -> ExitCode
where
    F: FnOnce(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = i32> + Send + '_>>,
{
    let sp = get_socket_path();
    ExitCode::from(f(&sp).await as u8)
}

async fn run_models_cmd(args: &[String]) -> ExitCode {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let sp = get_socket_path();
    match sub {
        "list" => {
            let json = args.iter().skip(3).any(|a| a == "--json");
            ExitCode::from(gg_core::cli::models_cmd::run_list(&sp, json).await as u8)
        }
        "load" => {
            let Some(path) = args.get(3).filter(|a| !a.starts_with("--")) else {
                eprintln!("Usage: GG-CORE models load <path> [--id ID]");
                return ExitCode::FAILURE;
            };
            let model_id = args
                .iter()
                .skip(4)
                .position(|a| a == "--id")
                .and_then(|i| args.get(4 + i + 1))
                .cloned();
            ExitCode::from(
                gg_core::cli::models_lifecycle_cmd::run_load(&sp, path, model_id).await as u8,
            )
        }
        "unload" => {
            let Some(id) = args.get(3) else {
                eprintln!("Usage: GG-CORE models unload <model-id>");
                return ExitCode::FAILURE;
            };
            ExitCode::from(gg_core::cli::models_lifecycle_cmd::run_unload(&sp, id).await as u8)
        }
        _ => {
            eprintln!("Unknown models subcommand: {}", sub);
            cli_parser::print_command_help("models");
            ExitCode::FAILURE
        }
    }
}

async fn run_config_cmd(args: &[String]) -> ExitCode {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "show" => ExitCode::from(gg_core::cli::config_cmd::try_show() as u8),
        "defaults" => {
            gg_core::cli::config_cmd::run_defaults();
            ExitCode::SUCCESS
        }
        "validate" => ExitCode::from(gg_core::cli::config_cmd::run_validate() as u8),
        _ => {
            eprintln!("Unknown config subcommand: {}", sub);
            cli_parser::print_command_help("config");
            ExitCode::FAILURE
        }
    }
}
