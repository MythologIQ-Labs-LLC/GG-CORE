//! Standalone daemon startup model preload for issue #106.

use gg_core::models::load_model_dispatch;
use gg_core::Runtime;

/// One model requested for preload when the standalone daemon starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModelSpec {
    pub path: String,
    pub model_id: Option<String>,
}

/// Extract startup model arguments from `gg-core-cli serve`.
///
/// This first #106 slice deliberately supports one startup model. Dynamic
/// load/unload IPC can reuse the same canonical loader/lifecycle seam later.
pub fn parse(args: &[String]) -> Result<Option<StartupModelSpec>, String> {
    let mut path = None;
    let mut model_id = None;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--model" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| "Missing value for --model".to_string())?;
                if path.is_some() {
                    return Err("Only one startup --model is supported in this release".into());
                }
                path = Some(value.clone());
                i += 2;
            }
            "--model-id" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| "Missing value for --model-id".to_string())?;
                if value.trim().is_empty() {
                    return Err("--model-id cannot be empty".into());
                }
                model_id = Some(value.clone());
                i += 2;
            }
            _ => i += 1,
        }
    }

    if model_id.is_some() && path.is_none() {
        return Err("--model-id requires --model <PATH>".into());
    }

    Ok(path.map(|path| StartupModelSpec { path, model_id }))
}

/// Load a startup model through the canonical contained lifecycle:
/// path validation -> metadata -> backend dispatch -> atomic registry/engine load.
pub async fn preload(
    runtime: &Runtime,
    spec: &StartupModelSpec,
) -> Result<String, Box<dyn std::error::Error>> {
    let validated = runtime.model_loader.validate_path(&spec.path)?;
    let metadata = runtime.model_loader.load_metadata(&validated)?;
    let model_id = spec
        .model_id
        .clone()
        .unwrap_or_else(|| metadata.name.clone());
    let model = load_model_dispatch(validated.as_path(), &model_id)?;

    runtime
        .model_lifecycle
        .load(model_id.clone(), metadata, model)
        .await?;

    Ok(model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn parses_startup_model_with_explicit_id() {
        let parsed = parse(&args(&[
            "gg-core-cli",
            "serve",
            "--model",
            "models/qwen.gguf",
            "--model-id",
            "local-qwen",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Some(StartupModelSpec {
                path: "models/qwen.gguf".into(),
                model_id: Some("local-qwen".into()),
            })
        );
    }

    #[test]
    fn parses_startup_model_without_explicit_id() {
        let parsed = parse(&args(&[
            "gg-core-cli",
            "serve",
            "--model",
            "models/qwen.gguf",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            Some(StartupModelSpec {
                path: "models/qwen.gguf".into(),
                model_id: None,
            })
        );
    }

    #[test]
    fn rejects_model_id_without_model_path() {
        let error = parse(&args(&["gg-core-cli", "serve", "--model-id", "orphan"])).unwrap_err();
        assert!(error.contains("requires --model"));
    }

    #[test]
    fn rejects_multiple_startup_models() {
        let error = parse(&args(&[
            "gg-core-cli",
            "serve",
            "--model",
            "models/a.gguf",
            "--model",
            "models/b.gguf",
        ]))
        .unwrap_err();
        assert!(error.contains("Only one startup --model"));
    }

    #[tokio::test]
    async fn preload_rejects_path_outside_contained_model_directories() {
        let runtime = Runtime::new(Default::default());
        let spec = StartupModelSpec {
            path: "../escape.gguf".into(),
            model_id: Some("escape".into()),
        };

        let error = preload(&runtime, &spec).await.unwrap_err().to_string();
        assert!(error.contains("Model path not allowed"));
        assert_eq!(runtime.model_lifecycle.count().await, 0);
    }
}
