// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! `models unload <id> --wait-ms N`: unload, then wait for the release of the
//! unloaded instance on the same authenticated connection (#137).
//!
//! "Released" means the last reference was dropped, backend destructors ran
//! and the memory budget was returned. Exit codes: 0 unloaded (released or
//! still pending), 1 unload/auth/protocol failure, 3 connection failure.

use std::time::Duration;

use crate::cli::{CliError, CliIpcClient};
use crate::ipc::protocol::{
    decode_message, encode_message, IpcMessage, ModelReleaseWaitRequest, ModelReleaseWaitResponse,
    ModelUnloadRequest, MAX_RELEASE_WAIT_MS,
};

/// Slack on top of the requested wait for connect, unload and framing.
const CLIENT_MARGIN: Duration = Duration::from_secs(5);

/// Run `models unload <id> --wait-ms N`.
pub async fn run_unload_with_wait(socket_path: &str, model_id: &str, wait_ms: u64) -> i32 {
    let wait_ms = wait_ms.min(MAX_RELEASE_WAIT_MS);
    let unload = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: model_id.to_string(),
    });
    let wait = IpcMessage::ModelReleaseWaitRequest(ModelReleaseWaitRequest::new(model_id, wait_ms));
    let (Ok(first), Ok(second)) = (encode_message(&unload), encode_message(&wait)) else {
        eprintln!("Encode error");
        return 1;
    };
    let client = CliIpcClient::new(socket_path.to_string())
        .with_timeout(Duration::from_millis(wait_ms) + CLIENT_MARGIN);
    let (a, b) = match client
        .send_receive_authenticated_pair(&first, &second)
        .await
    {
        Ok(pair) => pair,
        Err(e) => return connection_error(e),
    };
    match (decode_message(&a), decode_message(&b)) {
        (Ok(IpcMessage::ModelUnloadResponse(r)), _) if !r.success => {
            let why = r.error.unwrap_or_else(|| "unknown error".into());
            eprintln!("Unload failed for '{}': {}", r.model_id, why);
            1
        }
        (Ok(IpcMessage::ModelUnloadResponse(_)), Ok(IpcMessage::ModelReleaseWaitResponse(w))) => {
            report(&w)
        }
        _ => {
            eprintln!("Unexpected response from GG-CORE server");
            1
        }
    }
}

fn report(w: &ModelReleaseWaitResponse) -> i32 {
    if w.released {
        println!("Unloaded model '{}': released", w.model_id);
    } else {
        println!(
            "Unloaded model '{}': release pending: {} instance(s) still held by in-flight requests",
            w.model_id, w.pending_instances
        );
    }
    0
}

fn connection_error(e: CliError) -> i32 {
    match e {
        CliError::ConnectionFailed(_) | CliError::Timeout => {
            eprintln!("Error connecting to GG-CORE server: {}", e);
            eprintln!("Is the server running? Check GG_CORE_SOCKET_PATH.");
            3
        }
        other => {
            eprintln!("Error: {}", other);
            1
        }
    }
}
