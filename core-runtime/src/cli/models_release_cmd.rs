// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! `models unload <id> --wait-ms N`: unload, then wait for the release of the
//! unloaded instance on the same authenticated connection (#137).
//!
//! "Released" means the last reference was dropped, backend destructors ran
//! and the memory budget was returned. Exit codes: 0 released, 1
//! unload/auth/protocol failure, 3 connection failure, 4 still pending
//! after the requested wait. Plain `models unload` (without --wait-ms)
//! keeps its historical exit-code contract.

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
        (Ok(IpcMessage::ModelUnloadResponse(r)), Ok(IpcMessage::ModelReleaseWaitResponse(w)))
            if r.model_id == model_id =>
        {
            report(model_id, &w)
        }
        _ => {
            eprintln!("Unexpected response from GG-CORE server");
            1
        }
    }
}

fn report(expected_model_id: &str, w: &ModelReleaseWaitResponse) -> i32 {
    // A release receipt must identify precisely the model requested and
    // agree on whether any unloaded instances remain. Otherwise a broken
    // or out-of-order reply must never be interpreted as memory reclamation.
    if w.model_id != expected_model_id || w.released != (w.pending_instances == 0) {
        eprintln!("Inconsistent model release response");
        return 1;
    }
    if w.released {
        println!("Unloaded model '{}': released", w.model_id);
        0
    } else {
        eprintln!(
            "Unloaded model '{}': release pending: {} instance(s) still held by in-flight requests",
            w.model_id, w.pending_instances
        );
        // A caller using --wait-ms is awaiting memory release, not merely
        // unload acknowledgement. Never report shell success until released.
        4
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_completed_returns_zero() {
        let w = ModelReleaseWaitResponse::new("m", true, 0);
        assert_eq!(report("m", &w), 0);
    }

    #[test]
    fn expired_wait_with_held_instance_returns_nonzero() {
        let w = ModelReleaseWaitResponse::new("m", false, 1);
        assert_eq!(report("m", &w), 4);
    }

    #[test]
    fn wrong_model_cannot_claim_released() {
        let w = ModelReleaseWaitResponse::new("different-model", true, 0);
        assert_eq!(report("m", &w), 1);
    }

    #[test]
    fn contradictory_release_receipts_fail_closed() {
        assert_eq!(report("m", &ModelReleaseWaitResponse::new("m", true, 1)), 1);
        assert_eq!(report("m", &ModelReleaseWaitResponse::new("m", false, 0)), 1);
    }
}
