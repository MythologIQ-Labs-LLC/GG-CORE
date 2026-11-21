//! Typed CLI negotiation and reply contract regression tests.

use super::*;
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn negotiated_feature_requires_exact_name() {
        assert!(ensure_typed_feature(&["typed_inference_v1".into()]).is_ok());
        for features in [
            Vec::<String>::new(),
            vec!["text_generation".into()],
            vec!["typed_inference_v2".into()],
            vec!["typed_inference_v1_extra".into()],
        ] {
            assert!(matches!(
                ensure_typed_feature(&features),
                Err(CliError::Protocol(message)) if message.contains("typed_inference_v1")
            ));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_daemon_does_not_receive_unnegotiated_typed_request() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = tempfile::tempdir().expect("socket directory");
        let path = dir.path().join("typed.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind mock daemon");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut header = [0u8; 4];
            socket.read_exact(&mut header).await.expect("handshake header");
            let mut payload = vec![0; u32::from_le_bytes(header) as usize];
            socket.read_exact(&mut payload).await.expect("handshake payload");
            assert!(matches!(
                decode_message(&payload).expect("decode handshake"),
                IpcMessage::Handshake { .. }
            ));

            let ack = encode_message(&IpcMessage::HandshakeAck {
                session_id: "legacy-session".into(),
                protocol_version: Default::default(),
                features: Vec::new(),
            })
            .expect("encode legacy acknowledgement");
            socket
                .write_all(&(ack.len() as u32).to_le_bytes())
                .await
                .expect("ack header");
            socket.write_all(&ack).await.expect("ack payload");
            socket.flush().await.expect("flush acknowledgement");

            // Client should close without sending any typed frame at all.
            let mut next = [0u8; 4];
            let n = tokio::time::timeout(
                Duration::from_secs(3),
                socket.read(&mut next),
            )
            .await
            .expect("client must close promptly")
            .expect("read after acknowledgement");
            assert_eq!(n, 0, "client sent an unnegotiated typed request");
        });

        let request = TypedInferenceRequest {
            request_id: RequestId(7),
            model_id: "model".into(),
            task: TypedTask::Embedding,
            input: TypedInput::Single("hello".into()),
            timeout_ms: Some(1_000),
        };
        let client = CliIpcClient::new(path.to_string_lossy().into_owned());
        let err = client.send_typed(request).await.expect_err("old daemon must refuse");
        assert!(matches!(
            err, CliError::Protocol(message) if message.contains("typed_inference_v1")
        ));
        server.await.expect("mock daemon must complete");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn negotiated_daemon_receives_typed_request_and_returns_typed_result() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = tempfile::tempdir().expect("socket directory");
        let path = dir.path().join("typed.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind mock daemon");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            for step in 0..2 {
                let mut header = [0u8; 4];
                socket.read_exact(&mut header).await.expect("frame header");
                let mut payload = vec![0; u32::from_le_bytes(header) as usize];
                socket.read_exact(&mut payload).await.expect("frame data");
                let reply = if step == 0 {
                    assert!(matches!(
                        decode_message(&payload).expect("handshake"),
                        IpcMessage::Handshake { .. }
                    ));
                    IpcMessage::HandshakeAck {
                        session_id: "typed-session".into(),
                        protocol_version: Default::default(),
                        features: vec![TYPED_INFERENCE_FEATURE.into()],
                    }
                } else {
                    assert!(matches!(
                        decode_message(&payload).expect("typed request"),
                        IpcMessage::TypedInferenceRequest(ref req)
                            if req.request_id == RequestId(8)
                    ));
                    use crate::ipc::protocol_typed::{Embedding, TypedInferenceResponse};
                    IpcMessage::TypedInferenceResponse(TypedInferenceResponse::success(
                        RequestId(8),
                        TypedResult::Embedding(Embedding {
                            dimensions: 2,
                            vector: vec![0.6, 0.8],
                        }),
                    ))
                };
                let encoded = encode_message(&reply).expect("encode response");
                socket
                    .write_all(&(encoded.len() as u32).to_le_bytes())
                    .await
                    .expect("reply header");
                socket.write_all(&encoded).await.expect("reply body");
                socket.flush().await.expect("flush response");
            }
        });

        let request = TypedInferenceRequest {
            request_id: RequestId(8),
            model_id: "model".into(),
            task: TypedTask::Embedding,
            input: TypedInput::Single("hello".into()),
            timeout_ms: Some(1_000),
        };
        let client = CliIpcClient::new(path.to_string_lossy().into_owned());
        let result = client.send_typed(request).await.expect("negotiated result");
        assert_eq!(
            result,
            TypedResult::Embedding(crate::ipc::protocol_typed::Embedding {
                dimensions: 2,
                vector: vec![0.6, 0.8],
            })
        );
        server.await.expect("mock daemon must complete");
    }

    #[test]
    fn typed_success_requires_matching_request_id() {
        use crate::ipc::protocol_typed::{Embedding, TypedInferenceResponse};
        let result = TypedResult::Embedding(Embedding {
            dimensions: 1,
            vector: vec![1.0],
        });
        let response = IpcMessage::TypedInferenceResponse(TypedInferenceResponse::success(
            RequestId(9),
            result.clone(),
        ));
        let bytes = encode_message(&response).unwrap();
        assert_eq!(decode_typed_reply(&bytes, RequestId(9)).unwrap(), result);
        let err = decode_typed_reply(&bytes, RequestId(8)).unwrap_err();
        assert!(matches!(err, CliError::Protocol(msg) if msg.contains("ID mismatch")));
    }

    #[test]
    fn typed_error_requires_matching_request_id() {
        use crate::ipc::protocol::InferenceErrorCode;
        use crate::ipc::protocol_typed::TypedInferenceResponse;
        let response = IpcMessage::TypedInferenceResponse(TypedInferenceResponse::error(
            RequestId(9),
            "model refused".into(),
            InferenceErrorCode::UnsupportedTask,
        ));
        let bytes = encode_message(&response).unwrap();
        let err = decode_typed_reply(&bytes, RequestId(9)).unwrap_err();
        assert!(matches!(err, CliError::Protocol(msg) if msg == "model refused"));
        let err = decode_typed_reply(&bytes, RequestId(8)).unwrap_err();
        assert!(matches!(err, CliError::Protocol(msg) if msg.contains("ID mismatch")));
    }

    #[test]
    fn typed_empty_and_wrong_frame_remain_refused() {
        use crate::ipc::protocol_typed::TypedInferenceResponse;
        let empty = IpcMessage::TypedInferenceResponse(TypedInferenceResponse {
            request_id: RequestId(9),
            result: None,
            error: None,
            error_code: None,
        });
        let bytes = encode_message(&empty).unwrap();
        assert!(decode_typed_reply(&bytes, RequestId(9)).is_err());
        let bytes = encode_message(&IpcMessage::ModelsRequest).unwrap();
        assert!(decode_typed_reply(&bytes, RequestId(9)).is_err());
    }

    #[test]
    fn parses_single_and_batch_texts() {
        let a = parse_typed_args(&args(&["--model", "m", "--text", "a", "--text", "b"])).unwrap();
        assert_eq!(a.model_id, "m");
        assert_eq!(a.texts, vec!["a", "b"]);
        assert_eq!(a.timeout_ms, DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn rejects_missing_or_unknown_arguments() {
        assert!(parse_typed_args(&args(&["--text", "a"])).is_err());
        assert!(parse_typed_args(&args(&["--model", "m"])).is_err());
        assert!(parse_typed_args(&args(&["--model", "m", "--text"])).is_err());
        assert!(
            parse_typed_args(&args(&["--model", "m", "--text", "a", "--backend", "x"])).is_err()
        );
        assert!(parse_typed_args(&args(&["--model", "m", "--model", "n", "--text", "a"])).is_err());
        assert!(
            parse_typed_args(&args(&["--model", "m", "--text", "a", "--timeout-ms", "0"])).is_err()
        );
    }
