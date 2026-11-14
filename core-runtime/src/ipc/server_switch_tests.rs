//! Authenticated cold-switch IPC negative and idempotency contract tests.
//!
//! Uses the existing real server connection loop and an unrecorded stub
//! model. These tests MUST NOT be cited as installed-model switching proof.

use std::time::Duration;

use tokio::io::DuplexStream;

use super::handle_connection;
use super::release::TEST_LOCK;
use super::release_daemon::{daemon, Daemon, TOKEN};
use super::release_stub::{receive, send};
use crate::ipc::protocol::*;

async fn connect(d: &Daemon, authenticated: bool) -> (DuplexStream, tokio::task::JoinHandle<()>) {
    let guard = d.pool.try_acquire_owned().expect("connection slot");
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(handle_connection(server, d.handler.clone(), guard, 1 << 20));
    if authenticated {
        send(
            &mut client,
            &IpcMessage::Handshake {
                token: TOKEN.into(),
                protocol_version: None,
            },
        )
        .await;
        assert!(matches!(receive(&mut client).await, IpcMessage::HandshakeAck { .. }));
    }
    (client, task)
}

fn request(handle: u64) -> ModelSwitchRequest {
    ModelSwitchRequest::new(
        "1".repeat(64),
        "m".into(),
        handle,
        "models/b.gguf".into(),
        "b".into(),
        1000,
    )
}

#[tokio::test]
async fn switch_and_status_require_authentication() {
    let _serial = TEST_LOCK.lock().await;
    let d = daemon().await;
    let handle = d.lifecycle.get_handle("m").await.unwrap().id();

    let (mut client, server) = connect(&d, false).await;
    send(&mut client, &IpcMessage::ModelSwitchRequest(request(handle))).await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::Error { code: 401, .. }
    ));
    drop(client);
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("server terminates after auth failure")
        .expect("server task");
    assert!(d.lifecycle.is_loaded("m").await);

    let (mut client, _server) = connect(&d, false).await;
    send(
        &mut client,
        &IpcMessage::ModelSwitchStatusRequest(ModelSwitchStatusRequest::new("1".repeat(64))),
    )
    .await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::Error { code: 401, .. }
    ));
    assert!(d.lifecycle.is_loaded("m").await);
    d.shutdown.cancel();
}

#[tokio::test]
async fn disconnected_caller_cannot_accidentally_start_a_second_switch() {
    let _serial = TEST_LOCK.lock().await;
    let d = daemon().await;
    let observed = d.lifecycle.get_handle("m").await.unwrap();
    let original = request(observed.id());
    let (mut client, _server) = connect(&d, true).await;

    send(&mut client, &IpcMessage::ModelSwitchRequest(original.clone())).await;
    assert!(matches!(receive(&mut client).await, IpcMessage::ModelSwitchResponse(_)));
    drop(client);

    // A reconnect needs the normal handshake and its original operation ID.
    let (mut retry, _server) = connect(&d, true).await;
    let mut result = None;
    for _ in 0..100 {
        send(
            &mut retry,
            &IpcMessage::ModelSwitchStatusRequest(ModelSwitchStatusRequest::new(
                original.operation_id.clone(),
            )),
        )
        .await;
        match receive(&mut retry).await {
            IpcMessage::ModelSwitchStatusResponse(status) => {
                if status.phase == ModelSwitchPhase::Refused {
                    result = Some(status);
                    break;
                }
            }
            other => panic!("unexpected status reply: {other:?}"),
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let result = result.expect("unrecorded source refused, not retried");
    assert!(!result.ready);
    assert_eq!(result.to_handle, None);
    assert!(d.lifecycle.validate_handle(&observed).await);

    send(&mut retry, &IpcMessage::ModelSwitchRequest(original.clone())).await;
    match receive(&mut retry).await {
        IpcMessage::ModelSwitchResponse(s) => assert_eq!(s, result),
        other => panic!("retry must read old outcome: {other:?}"),
    }
    let mut changed = original;
    changed.to_id = "substituted".into();
    send(&mut retry, &IpcMessage::ModelSwitchRequest(changed)).await;
    assert!(matches!(
        receive(&mut retry).await,
        IpcMessage::Error { code: 409, .. }
    ));

    assert!(d.lifecycle.is_loaded("m").await);
    assert_eq!(d.lifecycle.get_handle("m").await, Some(observed));
    d.shutdown.cancel();
}

#[tokio::test]
async fn bad_switch_token_is_refused_before_any_mutation() {
    let _serial = TEST_LOCK.lock().await;
    let d = daemon().await;
    let old = d.lifecycle.get_handle("m").await.unwrap();
    let (mut client, _server) = connect(&d, true).await;
    let mut req = request(old.id());
    req.operation_id = "short".into();
    send(&mut client, &IpcMessage::ModelSwitchRequest(req)).await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::Error { code: 400, .. }
    ));
    assert!(d.lifecycle.validate_handle(&old).await);
    d.shutdown.cancel();
}
