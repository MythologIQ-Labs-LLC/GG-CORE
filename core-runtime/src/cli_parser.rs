//! CLI argument parsing and help text for GG-CORE.

/// Print general usage information.
pub fn print_usage() {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!(
        "GG-CORE - Greatest Good: Contained Offline Restricted Execution v{}

USAGE:
    GG-CORE [COMMAND] [OPTIONS]

COMMANDS:
    serve        Run the IPC server (default when no command given)
    infer        Run inference on a model (supports streaming)
    embed        Typed embedding over authenticated IPC (--model ID --text T ...)
    classify     Typed text classification over authenticated IPC (--model ID --text T)
    health       Full health check (exit 0 if healthy, 1 if unhealthy)
    live         Liveness probe for Kubernetes (exit 0 if alive)
    ready        Readiness probe for Kubernetes (exit 0 if ready)
    status       Show system status and statistics
    verify       Verify deployment health and configuration
    models       Manage loaded models (list, load, unload)
    config       Manage configuration (validate, show)
    version      Show version (--json: commit, build date, features)
    help         Show this help message

OPTIONS:
    -h, --help     Show help for command
    -V, --version  Show version information
    -v, --verbose  Enable verbose output
    --socket PATH  Override IPC socket path

EXAMPLES:
    GG-CORE serve                    # Run IPC server (also the default)
    GG-CORE infer --model phi-3 --prompt \"Hello\" [--stream]
    GG-CORE health                   # Full health check
    GG-CORE live                     # Liveness probe
    GG-CORE ready                    # Readiness probe
    GG-CORE status                   # Show system status
    GG-CORE models list              # List loaded models
    GG-CORE config validate          # Validate configuration

ENVIRONMENT:
    GG_CORE_SOCKET_PATH  IPC socket path
    CORE_AUTH_TOKEN      Shared auth token (required by serve; CLI uses it too)
    GG_CORE_ALLOW_INSECURE_NO_AUTH=1  Dev-only: serve without a token
    GG_CORE_SANDBOX      Process sandbox: enforce|seccomp|off
    RUST_LOG             Log level (debug, info, warn, error)

EXIT CODES:
    0  Success / Healthy
    1  Failure / Unhealthy
    2  Configuration error
    3  Connection error
",
        version
    );
}

/// Print detailed help for a specific command.
pub fn print_command_help(command: &str) {
    match command {
        "serve" => print_serve_help(),
        "health" => print_health_help(),
        "live" | "liveness" => print_live_help(),
        "ready" | "readiness" => print_ready_help(),
        "status" => print_status_help(),
        "infer" => print_infer_help(),
        "verify" => print_verify_help(),
        "models" => print_models_help(),
        "config" => print_config_help(),
        _ => {
            eprintln!(
                "No detailed help available for '{}'. Use 'GG-CORE help' for general usage.",
                command
            );
        }
    }
}

fn print_serve_help() {
    eprintln!(
        "GG-CORE serve - Run the IPC server

USAGE:
    GG-CORE serve [--model <path>]... [--model-id <id>]...

OPTIONS:
    --model PATH      Preload a model at startup (repeatable; path is
                      relative to the configured base path)
    --model-id ID     Id for the matching --model (positional pairing)

DESCRIPTION:
    Starts the GG-CORE IPC server. Default command when none is specified.
    Performs FIPS 140-3 power-on self-tests before starting. A preload
    failure aborts startup. GG_CORE_PRELOAD_MODELS (comma-separated paths)
    is used when no --model flags are given. Readiness reports not-ready
    until at least one model is loaded. A process sandbox is applied per
    GG_CORE_SANDBOX (enforce|seccomp|off; seccomp syscall allowlist is the
    Linux x86_64 default) — a sandbox that cannot apply aborts startup.

EXAMPLES:
    GG-CORE serve --model models/qwen.gguf --model-id local-model
"
    );
}

fn print_health_help() {
    eprintln!(
        "GG-CORE health - Full health check

USAGE:
    GG-CORE health [OPTIONS]

OPTIONS:
    --socket PATH  Override IPC socket path
    --json         Output in JSON format

EXIT CODES:
    0  System is healthy
    1  System is unhealthy
    3  Connection error
"
    );
}

fn print_live_help() {
    eprintln!(
        "GG-CORE live - Liveness probe

USAGE:
    GG-CORE live [OPTIONS]

OPTIONS:
    --socket PATH  Override IPC socket path

EXIT CODES:
    0  Process is alive
    1  Process is unresponsive
    3  Connection error
"
    );
}

fn print_ready_help() {
    eprintln!(
        "GG-CORE ready - Readiness probe

USAGE:
    GG-CORE ready [OPTIONS]

OPTIONS:
    --socket PATH  Override IPC socket path

EXIT CODES:
    0  Ready to serve traffic
    1  Not ready
    3  Connection error
"
    );
}

fn print_status_help() {
    eprintln!(
        "GG-CORE status - Show system status

USAGE:
    GG-CORE status [OPTIONS]

OPTIONS:
    --socket PATH  Override IPC socket path
    --json         Output in JSON format
"
    );
}

fn print_infer_help() {
    eprintln!(
        "GG-CORE infer - Run inference

USAGE:
    GG-CORE infer --model <MODEL> --prompt <PROMPT> [OPTIONS]

OPTIONS:
    --model <MODEL>      Model ID to use for inference
    --prompt <PROMPT>    Input prompt for generation
    --max-tokens <N>     Maximum tokens to generate (default: 256)
    --chat-template ID   Opt-in model-specific prompt format: qwen2.5
    --system TEXT        System instructions (requires --chat-template)
    --stream             Enable token-by-token streaming output
    --timeout-ms <MS>    Request deadline enforced by the daemon (default: 300000)

EXIT CODES:
    0  Inference completed successfully
    1  Inference failed or connection error

DESCRIPTION:
    Default: raw completion. Qwen2.5 ChatML is explicit opt-in only;
    it does NOT inspect model metadata or change daemon security policy.
    Reject reserved role-token syntax in chat message text.

EXAMPLES:
    GG-CORE infer --model phi-3 --prompt \"Count to 5\" --stream
    GG-CORE infer --model qwen --prompt \"Hello\" --chat-template qwen2.5 --stream
"
    );
}

fn print_verify_help() {
    eprintln!(
        "GG-CORE verify - Verify deployment

USAGE:
    GG-CORE verify [OPTIONS]

OPTIONS:
    --socket PATH  Override IPC socket path
    --all          Run all verification checks

EXIT CODES:
    0  All checks passed
    1  One or more checks failed
"
    );
}

fn print_models_help() {
    eprintln!(
        "GG-CORE models - Manage models

USAGE:
    GG-CORE models <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    list [--json]         List loaded models
    load <path> [--id ID] Load a model (authenticated; base-path relative)
    unload <model-id>     Unload a loaded model (authenticated)

OPTIONS:
    --json         Output in JSON format (list)

ENVIRONMENT:
    CORE_AUTH_TOKEN  Shared token for authenticated load/unload
"
    );
}

fn print_config_help() {
    eprintln!(
        "GG-CORE config - Manage configuration

USAGE:
    GG-CORE config <SUBCOMMAND>

SUBCOMMANDS:
    show           Show current configuration
    validate       Validate configuration file
    defaults       Show default configuration
"
    );
}
