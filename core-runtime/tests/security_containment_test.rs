//! Physical path confinement (RC acceptance gate): filesystem indirection
//! must not let a model load read outside `models/` / `tokenizers/`.
//!
//! Lexical normalization is covered by `security_path_traversal_test`; these
//! tests plant real symlinks under a temporary base path.

use gg_core::models::{LoadError, ModelLoader};

struct Base {
    dir: tempfile::TempDir,
}

impl Base {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("models")).unwrap();
        std::fs::create_dir_all(dir.path().join("outside")).unwrap();
        std::fs::write(dir.path().join("outside/secret.bin"), b"outside bytes").unwrap();
        std::fs::write(dir.path().join("models/real.gguf"), b"inside bytes").unwrap();
        Self { dir }
    }
    fn loader(&self) -> ModelLoader {
        ModelLoader::new(self.dir.path().to_path_buf())
    }
    #[cfg(unix)]
    fn path(&self, rel: &str) -> std::path::PathBuf {
        self.dir.path().join(rel)
    }
}

#[cfg(unix)]
fn assert_not_allowed(result: Result<gg_core::models::ModelPath, LoadError>, what: &str) {
    match result {
        Err(LoadError::PathNotAllowed(shown)) => {
            // The rejection must not disclose where the link pointed.
            assert!(
                !shown.to_string_lossy().contains("outside"),
                "{what}: leaked target {shown:?}"
            );
        }
        other => panic!("{what}: expected PathNotAllowed, got {other:?}"),
    }
}

#[test]
fn regular_file_inside_models_is_allowed() {
    let base = Base::new();
    let p = base.loader().validate_path("models/real.gguf").unwrap();
    assert!(p.as_path().ends_with("models/real.gguf"));
}

#[test]
fn absent_file_passes_validation_and_fails_as_not_found() {
    let base = Base::new();
    let loader = base.loader();
    let p = loader.validate_path("models/missing.gguf").unwrap();
    assert!(matches!(
        loader.load_metadata(&p),
        Err(LoadError::NotFound(_))
    ));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinked_file_pointing_outside_is_rejected() {
        let base = Base::new();
        symlink(
            base.path("outside/secret.bin"),
            base.path("models/link.gguf"),
        )
        .unwrap();
        assert_not_allowed(
            base.loader().validate_path("models/link.gguf"),
            "file symlink",
        );
    }

    #[test]
    fn symlinked_directory_pointing_outside_is_rejected() {
        let base = Base::new();
        symlink(base.path("outside"), base.path("models/sub")).unwrap();
        assert_not_allowed(
            base.loader().validate_path("models/sub/secret.bin"),
            "directory symlink",
        );
    }

    #[test]
    fn absolute_symlink_to_system_file_is_rejected() {
        let base = Base::new();
        symlink("/etc/hostname", base.path("models/hostname.gguf")).unwrap();
        let r = base.loader().validate_path("models/hostname.gguf");
        assert!(matches!(r, Err(LoadError::PathNotAllowed(_))), "got {r:?}");
    }

    #[test]
    fn dangling_symlink_is_rejected() {
        let base = Base::new();
        symlink(
            base.path("outside/nope.bin"),
            base.path("models/dangling.gguf"),
        )
        .unwrap();
        assert_not_allowed(
            base.loader().validate_path("models/dangling.gguf"),
            "dangling",
        );
    }

    #[test]
    fn symlink_that_stays_inside_models_is_allowed() {
        let base = Base::new();
        symlink(
            base.path("models/real.gguf"),
            base.path("models/alias.gguf"),
        )
        .unwrap();
        assert!(base.loader().validate_path("models/alias.gguf").is_ok());
    }

    #[test]
    fn symlinked_models_root_itself_is_honored() {
        // Deployments may point `models/` at another volume: both sides are
        // resolved, so files under the real target remain contained.
        let dir = tempfile::tempdir().unwrap();
        let volume = dir.path().join("volume");
        std::fs::create_dir_all(&volume).unwrap();
        std::fs::write(volume.join("m.gguf"), b"bytes").unwrap();
        let base = dir.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        symlink(&volume, base.join("models")).unwrap();
        assert!(ModelLoader::new(base)
            .validate_path("models/m.gguf")
            .is_ok());
    }

    #[test]
    fn symlink_swapped_in_after_validation_is_refused_by_the_open() {
        // TOCTOU: validation sees a regular file; the entry is then replaced
        // by a symlink before the mmap open. O_NOFOLLOW refuses it.
        let base = Base::new();
        let loader = base.loader();
        let validated = loader.validate_path("models/real.gguf").unwrap();
        std::fs::remove_file(base.path("models/real.gguf")).unwrap();
        symlink(
            base.path("outside/secret.bin"),
            base.path("models/real.gguf"),
        )
        .unwrap();
        assert!(loader.load_mapped(&validated).is_err());
        let digest = gg_core::models::integrity::file_sha256_hex(validated.as_path());
        assert!(digest.unwrap_err().to_string().contains("symbolic link"));
    }
}
