// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Build script for GG-CORE
//!
//! - Always: records build provenance (git commit, build date, rustc) as
//!   `VERGEN_*` compile-time env vars consumed by `version --json` and
//!   `status --json`, so a consumer can bind a running binary to an exact
//!   source revision. Source Git HEAD wins over an unrelated consumer CI SHA.
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
    // GITHUB_SHA identifies the invoking CI project, not necessarily this crate.
    println!("cargo:rerun-if-env-changed=GG_CORE_BUILD_COMMIT");
    // .git/HEAD usually contains a symbolic ref (e.g. refs/heads/main).
    // Its contents do not change when new commits advance that same branch.
    // Track the actual branch tip as well, including linked worktree paths.
    for path in git_watch_paths() {
        println!("cargo:rerun-if-changed={path}");
    }
    let explicit = std::env::var("GG_CORE_BUILD_COMMIT").ok();
    let checkout_head = git_head();
    let commit = source_commit(explicit.as_deref(), checkout_head.as_deref());
    println!("cargo:rustc-env=VERGEN_GIT_SHA={commit}");

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

/// Only an explicitly producer-authorized source commit may override Git.
fn source_commit(explicit: Option<&str>, checkout_head: Option<&str>) -> String {
    if let Some(raw) = explicit.filter(|raw| !raw.trim().is_empty()) {
        return canonical_commit(raw)
            .expect("GG_CORE_BUILD_COMMIT must be a full 40-character hexadecimal Git commit");
    }
    // A Git dependency builds from its own checkout, not the consumer repo.
    checkout_head
        .and_then(canonical_commit)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Accept only a full Git SHA-1 commit, never arbitrary text or short refs.
/// Lowercasing gives a stable, JSON-safe source identity for version/status.
fn canonical_commit(raw: &str) -> Option<String> {
    let sha = raw.trim();
    (sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| sha.to_ascii_lowercase())
}

fn git_output_in(directory: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .current_dir(directory)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn git_output(args: &[&str]) -> Option<String> {
    git_output_in(std::path::Path::new("."), args)
}

fn git_head() -> Option<String> {
    canonical_commit(&git_output(&["rev-parse", "HEAD"])?)
}

/// Git path resolution handles ordinary checkouts and linked worktrees.
/// For attached HEAD, watching the branch ref is essential: HEAD itself
/// remains unchanged while the branch advances. A missing loose ref path
/// is still watched so a formerly packed ref can become loose.
fn git_watch_paths_in(directory: &std::path::Path) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(path) = git_output_in(directory, &["rev-parse", "--git-path", "HEAD"]) {
        paths.push(path);
    }
    if let Some(reference) = git_output_in(directory, &["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(path) = git_output_in(directory, &["rev-parse", "--git-path", &reference]) {
            paths.push(path);
        }
    }
    if let Some(path) = git_output_in(directory, &["rev-parse", "--git-path", "packed-refs"]) {
        if directory.join(&path).exists() {
            paths.push(path);
        }
    }
    paths
}

fn git_watch_paths() -> Vec<String> {
    git_watch_paths_in(std::path::Path::new("."))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_identity_accepts_only_full_ascii_hex_sha() {
        let lower = "a".repeat(40);
        let upper = "A".repeat(40);
        assert_eq!(canonical_commit(&lower), Some(lower.clone()));
        assert_eq!(canonical_commit(&upper), Some(lower.clone()));
        assert_eq!(canonical_commit(&format!("  {lower}\n")), Some(lower));
        for invalid in [
            String::new(),
            "unknown".into(),
            "a".repeat(39),
            "a".repeat(41),
            "g".repeat(40),
            "İ".repeat(20),
            format!("{}\nmalicious", "a".repeat(40)),
        ] {
            assert!(canonical_commit(&invalid).is_none(), "invalid SHA accepted");
        }
    }

    #[test]
    fn checkout_identity_is_separate_from_consumer_ci_identity() {
        let producer = "a".repeat(40);
        let consumer_ci = "b".repeat(40);
        assert_ne!(producer, consumer_ci);
        // No GITHUB_SHA argument exists: only GG-CORE's source checkout counts.
        assert_eq!(source_commit(None, Some(&producer)), producer);
        assert_eq!(source_commit(None, None), "unknown");
        assert_eq!(source_commit(Some(""), Some(&producer)), producer);
    }

    #[test]
    fn explicit_archive_commit_is_validated_and_overrides_checkout() {
        let source = "a".repeat(40);
        let override_sha = "B".repeat(40);
        assert_eq!(
            source_commit(Some(&override_sha), Some(&source)),
            "b".repeat(40)
        );
        assert!(std::panic::catch_unwind(|| {
            source_commit(Some("not-a-valid-commit"), None)
        })
        .is_err());
    }

    #[test]
    fn attached_branch_tip_moves_while_head_pointer_stays_constant() {
        use std::path::PathBuf;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gg-core-provenance-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("create isolated Git test dir");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .current_dir(&root)
            .status()
            .expect("Git installed in provenance CI");
        assert!(initialized.success(), "initialize test Git repository");

        let paths = git_watch_paths_in(&root);
        let head_path = git_output_in(&root, &["rev-parse", "--git-path", "HEAD"])
            .expect("resolve worktree HEAD");
        let branch_path = git_output_in(&root, &["rev-parse", "--git-path", "refs/heads/main"])
            .expect("resolve actual main branch ref");
        assert!(paths.contains(&head_path), "must watch HEAD");
        assert!(paths.contains(&branch_path), "must watch branch ref");

        let head = root.join(&head_path);
        let branch = root.join(PathBuf::from(&branch_path));
        let head_before = std::fs::read(&head).expect("read symbolic HEAD");
        std::fs::create_dir_all(branch.parent().expect("branch ref directory"))
            .expect("create branch directory");
        std::fs::write(&branch, format!("{}\n", "a".repeat(40))).expect("set first synthetic ref");
        let first = std::fs::read(&branch).expect("read initial branch tip");
        std::fs::write(&branch, format!("{}\n", "b".repeat(40))).expect("advance same-branch tip");
        assert_ne!(
            std::fs::read(&branch).expect("read advanced branch tip"),
            first
        );
        assert_eq!(
            std::fs::read(&head).expect("read unchanged symbolic HEAD"),
            head_before
        );
        std::fs::remove_dir_all(&root).expect("clean synthetic repository");
    }

    #[test]
    fn current_checkout_tracks_both_head_and_branch_tip() {
        // Source archives without a .git directory cannot supply Git refs.
        let Some(head_path) = git_output(&["rev-parse", "--git-path", "HEAD"]) else {
            return;
        };
        let paths = git_watch_paths();
        assert!(
            paths.contains(&head_path),
            "HEAD change must retrigger build"
        );
        if let Some(reference) = git_output(&["symbolic-ref", "--quiet", "HEAD"]) {
            let ref_path = git_output(&["rev-parse", "--git-path", &reference])
                .expect("current branch ref path");
            assert!(
                paths.contains(&ref_path),
                "same-branch commits must retrigger build provenance"
            );
        }
    }
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
