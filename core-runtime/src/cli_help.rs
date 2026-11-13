//! Long help text for the `models` and `config` subcommands (Section 4
//! Razor sidecar of `cli_parser.rs`).

pub(super) fn print_models_help() {
    eprintln!(
        "GG-CORE models - Manage models

USAGE:
    GG-CORE models <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    list [--json]         List loaded models
    load <path> [--id ID] Load a model (authenticated; base-path relative)
    unload <id> [--wait-ms N]  Unload; optionally wait for its memory release

EXIT CODES (unload --wait-ms):
    0  Memory released
    1  Unload/auth/protocol failure
    3  Connection failure
    4  Release still pending after wait

OPTIONS:
    --json         Output in JSON format (list)

ENVIRONMENT:
    CORE_AUTH_TOKEN  Shared token for authenticated load/unload
"
    );
}

pub(super) fn print_config_help() {
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
