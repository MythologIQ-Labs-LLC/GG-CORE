//! `embed` and `classify` CLI commands over the governed typed IPC protocol.
//!
//! The CLI never executes a model itself: it authenticates with
//! `CORE_AUTH_TOKEN`, sends one `typed_inference_request`, and prints the
//! typed JSON result. All policy is enforced by the daemon.

use std::time::Duration;

use super::{get_socket_path, CliError, CliIpcClient};
use crate::ipc::protocol::{decode_message, encode_message, IpcMessage, RequestId};
use crate::ipc::protocol_typed::{TypedInferenceRequest, TypedInput, TypedResult, TypedTask};

/// Client-side grace beyond the daemon-enforced deadline (as for `infer`).
const CLIENT_GRACE_MS: u64 = 5_000;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

impl CliIpcClient {
    /// Send one authenticated typed request and return its typed result.
    pub async fn send_typed(
        &self,
        request: TypedInferenceRequest,
    ) -> Result<TypedResult, CliError> {
        let bytes = encode_message(&IpcMessage::TypedInferenceRequest(request))
            .map_err(|e| CliError::Protocol(e.to_string()))?;
        let reply = self.send_receive_authenticated(&bytes).await?;
        match decode_message(&reply).map_err(|e| CliError::Protocol(e.to_string()))? {
            IpcMessage::TypedInferenceResponse(resp) => match (resp.result, resp.error) {
                (Some(result), None) => Ok(result),
                (_, Some(err)) => Err(CliError::Protocol(err)),
                (None, None) => Err(CliError::Protocol("empty typed response".into())),
            },
            IpcMessage::Error { message, .. } => Err(CliError::Protocol(message)),
            _ => Err(CliError::Protocol("Unexpected response type".into())),
        }
    }
}

/// Parsed `embed`/`classify` arguments.
#[derive(Debug, PartialEq)]
pub struct TypedArgs {
    pub model_id: String,
    pub texts: Vec<String>,
    pub timeout_ms: u64,
}

/// Parse `--model ID --text T [--text T2 ...] [--timeout-ms MS]`.
pub fn parse_typed_args(args: &[String]) -> Result<TypedArgs, String> {
    let (mut model_id, mut texts, mut timeout_ms) = (None, Vec::new(), DEFAULT_TIMEOUT_MS);
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned();
        match (args[i].as_str(), value) {
            ("--model", Some(v)) if model_id.is_none() => model_id = Some(v),
            ("--text", Some(v)) => texts.push(v),
            ("--timeout-ms", Some(v)) => {
                timeout_ms = v
                    .parse()
                    .ok()
                    .filter(|ms| *ms > 0)
                    .ok_or("--timeout-ms must be > 0")?
            }
            (flag, _) => return Err(format!("unexpected or incomplete argument: {flag}")),
        }
        i += 2;
    }
    let model_id = model_id.ok_or("--model is required")?;
    if texts.is_empty() {
        return Err("at least one --text is required".into());
    }
    Ok(TypedArgs {
        model_id,
        texts,
        timeout_ms,
    })
}

/// Run `embed` or `classify`. Returns a process exit code.
pub async fn run_typed(task: TypedTask, args: &[String]) -> i32 {
    let parsed = match parse_typed_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}\nUsage: gg-core-cli embed|classify --model ID --text T [--text T2 ...] [--timeout-ms MS]");
            return 1;
        }
    };
    let input = match parsed.texts.len() {
        1 => TypedInput::Single(parsed.texts.into_iter().next().unwrap_or_default()),
        _ => TypedInput::Batch(parsed.texts),
    };
    let request = TypedInferenceRequest {
        request_id: RequestId(1),
        model_id: parsed.model_id,
        task,
        input,
        timeout_ms: Some(parsed.timeout_ms),
    };
    let client = CliIpcClient::new(get_socket_path()).with_timeout(Duration::from_millis(
        parsed.timeout_ms.saturating_add(CLIENT_GRACE_MS),
    ));
    match client.send_typed(request).await {
        Ok(result) => match serde_json::to_string(&result) {
            Ok(json) => {
                println!("{json}");
                0
            }
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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
}
