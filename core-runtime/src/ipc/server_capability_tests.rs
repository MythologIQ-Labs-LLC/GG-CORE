//! `capabilities_request` through the real connection loop (#49/#50):
//! authenticated, read-only, output-only, and refusing client-sent claims.

use std::sync::Arc;

use tokio::io::DuplexStream;

use super::super::handle_connection;
use super::super::release::TEST_LOCK;
use super::super::release_stub::{receive, send};
use super::{daemon, Daemon, TOKEN};
use crate::ipc::protocol::*;

async fn open(d: &Daemon, auth: bool) -> DuplexStream {
    let guard = d.pool.try_acquire_owned().expect("slot");
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    tokio::spawn(handle_connection(server, d.handler.clone(), guard, 1 << 20));
    if auth {
        let hello = IpcMessage::Handshake {
            token: TOKEN.into(),
            protocol_version: None,
        };
        send(&mut client, &hello).await;
        assert!(matches!(
            receive(&mut client).await,
            IpcMessage::HandshakeAck { .. }
        ));
    }
    client
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthenticated_capabilities_request_is_refused_and_closed() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon().await;
    let mut c = open(&d, false).await;
    send(&mut c, &IpcMessage::CapabilitiesRequest).await;
    let reply = receive(&mut c).await;
    assert!(
        matches!(reply, IpcMessage::Error { code: 401, .. }),
        "{reply:?}"
    );
    let next = super::super::read_frame(&mut c, 1 << 20).await;
    assert!(next.is_err(), "the server closes after a 401");
    d.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_snapshot_reports_the_loaded_stub_truthfully() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon().await;
    let mut c = open(&d, true).await;
    send(&mut c, &IpcMessage::CapabilitiesRequest).await;
    let IpcMessage::CapabilitiesResponse(r) = receive(&mut c).await else {
        panic!("expected capabilities_response")
    };
    let s = &r.snapshot;
    assert_eq!(s["schema_version"], 1);
    assert_eq!(s["qualified"], serde_json::json!([]));
    assert_eq!(s["operator"]["auth_required"], true);
    assert_eq!(s["operator"]["network_listener"], false);
    let m = &s["loaded"][0];
    assert_eq!(m["model_id"], "m");
    assert!(
        m["backend"].is_null(),
        "the Blocking stub is no backend type"
    );
    assert_eq!(m["paths"], serde_json::json!([]));
    let declared = serde_json::json!([{ "task": "text_generation", "target": "cpu" }]);
    assert_eq!(m["declared_only_paths"], declared);
    d.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_sent_capability_claim_is_refused_and_changes_nothing() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon().await;
    let mut c = open(&d, true).await;
    let forged =
        serde_json::json!({ "qualified": [{ "task": "text_generation", "target": "cuda" }] });
    send(
        &mut c,
        &IpcMessage::CapabilitiesResponse(CapabilitiesResponse::new(forged)),
    )
    .await;
    let reply = receive(&mut c).await;
    assert!(
        matches!(reply, IpcMessage::Error { code: 400, .. }),
        "{reply:?}"
    );
    send(&mut c, &IpcMessage::CapabilitiesRequest).await; // connection stays open
    let IpcMessage::CapabilitiesResponse(r) = receive(&mut c).await else {
        panic!("expected capabilities_response")
    };
    assert_eq!(
        r.snapshot["qualified"],
        serde_json::json!([]),
        "no claim was ingested"
    );
    d.shutdown.cancel();
}

#[tokio::test]
async fn with_auth_disabled_the_frame_is_open_and_says_so() {
    let rt = crate::Runtime::new(crate::RuntimeConfig {
        auth_token: TOKEN.into(),
        ..Default::default()
    });
    let handler = crate::ipc::IpcHandler::new(
        Arc::clone(&rt.ipc_handler.auth),
        rt.request_queue.clone(),
        crate::ipc::IpcHandlerConfig {
            require_auth: false,
            ..Default::default()
        },
        rt.shutdown.clone(),
        rt.health.clone(),
        rt.model_registry.clone(),
        rt.metrics_store.clone(),
        Arc::clone(&rt.inference_engine),
        Arc::clone(&rt.model_lifecycle),
        Arc::clone(&rt.model_loader),
    );
    let request = encode_message(&IpcMessage::CapabilitiesRequest).unwrap();
    let (bytes, _) = handler
        .process(&request, None)
        .await
        .expect("no session needed");
    let IpcMessage::CapabilitiesResponse(r) = decode_message(&bytes).unwrap() else {
        panic!("expected capabilities_response")
    };
    assert_eq!(r.snapshot["operator"]["auth_required"], false);
}
