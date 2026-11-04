// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Build script for GG-CORE
//!
//! - Always: records build provenance (git commit, build date, rustc) as
//!   `VERGEN_*` compile-time env vars consumed by `version --json` and
//!   `status --json`, so a consumer can bind a running binary to an exact
//!   source revision. `GITHUB_SHA` (CI) takes precedence over `git`.
//! - With the `ffi` feature: generates the C header via cbindgen.

fn main() {
    emit_provenance();

    // Only run cbindgen when ffi feature is enabled
    #[cfg(feature = "ffi")]
    {
        if let Err(e) = generate_c_header() {
            println!("cargo:warning=Failed to generate C header: {}", e);
        }
    }
}

fn emit_provenance() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=GG_CORE_BUILD_COMMIT");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    let commit = std::env::var("GG_CORE_BUILD_COMMIT")
        .or_else(|_| std::env::var("GITHUB_SHA"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(git_head)
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=VERGEN_GIT_SHA={}", commit.trim());

    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=VERGEN_BUILD_DATE={}", utc_date(secs));

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let semver = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().nth(1).map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=VERGEN_RUSTC_SEMVER={}", semver);
}

fn git_head() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (sha.len() == 40).then_some(sha)
}

/// YYYY-MM-DD for a Unix timestamp (proleptic Gregorian, UTC) — no chrono
/// in the build script.
fn utc_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(feature = "ffi")]
fn generate_c_header() -> Result<(), Box<dyn std::error::Error>> {
    use std::env;
    use std::path::PathBuf;

    let crate_dir = env::var("CARGO_MANIFEST_DIR")?;
    let output_dir = PathBuf::from(&crate_dir).join("include");

    // Ensure include directory exists
    std::fs::create_dir_all(&output_dir)?;

    let output_file = output_dir.join("gg_core.h");
    let config_path = PathBuf::from(&crate_dir).join("cbindgen.toml");

    if config_path.exists() {
        let config = cbindgen::Config::from_file(&config_path)?;

        if let Ok(bindings) = cbindgen::Builder::new()
            .with_crate(&crate_dir)
            .with_config(config)
            .generate()
        {
            bindings.write_to_file(&output_file);
            println!("cargo:rerun-if-changed=cbindgen.toml");
            println!("cargo:rerun-if-changed=src/ffi/");
        } else {
            println!("cargo:warning=cbindgen could not generate bindings");
        }
    }

    Ok(())
}
