//! `version` command: exact runtime version and build provenance.
//!
//! The plain form (`GG-CORE <semver>`) is a stable one-line contract that
//! the release workflow checks byte-for-byte. `--json` adds what a consumer
//! needs to bind a binary to a source revision and a capability set.

/// Compile-time provenance recorded by `build.rs`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = env!("VERGEN_GIT_SHA");
pub const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");
pub const RUSTC: &str = env!("VERGEN_RUSTC_SEMVER");

/// Cargo features compiled into this binary that change its capability set.
pub fn compiled_features() -> Vec<&'static str> {
    let mut f = Vec::new();
    if cfg!(feature = "gguf") {
        f.push("gguf");
    }
    if cfg!(feature = "onnx") {
        f.push("onnx");
    }
    if cfg!(feature = "ffi") {
        f.push("ffi");
    }
    if cfg!(feature = "python") {
        f.push("python");
    }
    if cfg!(feature = "advanced") {
        f.push("advanced");
    }
    if cfg!(feature = "cuda") {
        f.push("cuda");
    }
    if cfg!(feature = "metal") {
        f.push("metal");
    }
    f
}

/// JSON provenance document (stable keys; additive changes only).
pub fn version_json() -> String {
    let features: Vec<String> = compiled_features()
        .into_iter()
        .map(|f| format!("\"{f}\""))
        .collect();
    format!(
        concat!(
            "{{\"name\":\"GG-CORE\",\"version\":\"{}\",\"commit\":\"{}\",",
            "\"build_date\":\"{}\",\"rust_version\":\"{}\",\"features\":[{}],",
            "\"protocol_version\":\"{}\",\"target\":\"{}-{}\"}}"
        ),
        VERSION,
        COMMIT,
        BUILD_DATE,
        RUSTC,
        features.join(","),
        format!(
            "{:?}",
            gg_core::ipc::protocol_types::CURRENT_PROTOCOL_VERSION
        ),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Print the version line, or the JSON document with `--json`.
pub fn print_version(json: bool) {
    if json {
        println!("{}", version_json());
    } else {
        println!("GG-CORE {}", VERSION);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_json_is_well_formed_and_carries_provenance() {
        let doc: serde_json::Value = serde_json::from_str(&version_json()).unwrap();
        assert_eq!(doc["version"], VERSION);
        assert_eq!(doc["name"], "GG-CORE");
        assert!(doc["commit"].as_str().unwrap().len() >= 7 || doc["commit"] == "unknown");
        assert!(doc["features"].is_array());
        assert_eq!(doc["protocol_version"], "V1");
        assert!(doc["target"].as_str().unwrap().contains('-'));
    }

    #[test]
    fn gguf_feature_is_reported_when_compiled() {
        assert_eq!(
            compiled_features().contains(&"gguf"),
            cfg!(feature = "gguf")
        );
    }
}
