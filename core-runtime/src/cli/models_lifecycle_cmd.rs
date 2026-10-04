// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Models CLI subcommands: load / unload (B-41, issue #106).
//!
//! Both operations mutate daemon state and therefore run over an
//! authenticated IPC connection (`CORE_AUTH_TOKEN`). Paths passed to `load`
//! are relative to the daemon's base path and validated server-side; the CLI
//! never gains filesystem authority.

use crate::cli::CliIpcClient;
use crate::ipc::protocol::{
    decode_message, encode_message, IpcMessage, ModelLoadRequest, ModelUnloadRequest,
};

/// Run `models load <path> [--id ID]`. Exit codes: 0 loaded, 1 load/auth
/// failure, 3 connection failure.
pub async fn run_load(socket_path: &str, path: &str, model_id: Option<String>) -> i32 {
    let request = IpcMessage::ModelLoadRequest(ModelLoadRequest {
        path: path.to_string(),
        model_id,
    });
    let response = match send_authenticated(socket_path, &request).await {
        Ok(r) => r,
        Err(code) => return code,
    };
    match response {
        IpcMessage::ModelLoadResponse(r) if r.success => {
            println!(
                "Loaded model '{}' (handle {})",
                r.model_id.unwrap_or_default(),
                r.handle_id.unwrap_or_default()
            );
            0
        }
        IpcMessage::ModelLoadResponse(r) => {
            eprintln!(
                "Load failed: {}",
                r.error.unwrap_or_else(|| "unknown error".into())
            );
            1
        }
        other => unexpected(other),
    }
}

/// Run `models unload <id>`. Exit codes: 0 unloaded, 1 failure, 3 connection.
pub async fn run_unload(socket_path: &str, model_id: &str) -> i32 {
    let request = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: model_id.to_string(),
    });
    let response = match send_authenticated(socket_path, &request).await {
        Ok(r) => r,
        Err(code) => return code,
    };
    match response {
        IpcMessage::ModelUnloadResponse(r) if r.success => {
            println!("Unloaded model '{}'", r.model_id);
            0
        }
        IpcMessage::ModelUnloadResponse(r) => {
            eprintln!(
                "Unload failed for '{}': {}",
                r.model_id,
                r.error.unwrap_or_else(|| "unknown error".into())
            );
            1
        }
        other => unexpected(other),
    }
}

/// Encode, send over an authenticated connection, decode. `Err` carries the
/// exit code (1 protocol/auth, 3 connection/timeout).
async fn send_authenticated(socket_path: &str, message: &IpcMessage) -> Result<IpcMessage, i32> {
    let bytes = match encode_message(message) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Encode error: {}", e);
            return Err(1);
        }
    };
    let client = CliIpcClient::new(socket_path.to_string());
    let response_bytes = match client.send_receive_authenticated(&bytes).await {
        Ok(b) => b,
        Err(e @ (crate::cli::CliError::ConnectionFailed(_) | crate::cli::CliError::Timeout)) => {
            eprintln!("Error connecting to GG-CORE server: {}", e);
            eprintln!("Is the server running? Check GG_CORE_SOCKET_PATH.");
            return Err(3);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return Err(1);
        }
    };
    decode_message(&response_bytes).map_err(|e| {
        eprintln!("Protocol error: {}", e);
        1
    })
}

fn unexpected(message: IpcMessage) -> i32 {
    if let IpcMessage::Error { code, message } = message {
        eprintln!("Server error {}: {}", code, message);
    } else {
        eprintln!("Unexpected response type");
    }
    1
}
