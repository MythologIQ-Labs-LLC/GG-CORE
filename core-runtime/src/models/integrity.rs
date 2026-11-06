//! Model file integrity: manifest hash verification and load-time identity.
//!
//! The file is hashed through a handle opened without following a final
//! symlink (Unix `O_NOFOLLOW`), and the handle's identity is captured so the
//! dispatcher can confirm, after the backend has loaded the model by path,
//! that the path still names the very file that was hashed. A swap between
//! hash and load (rename/symlink substitution) is therefore detected and the
//! load is failed; an in-place rewrite of an already-mmapped file is outside
//! this check and requires write access to the models directory.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::engine::error::InferenceError;
use crate::models::manifest::ModelManifest;
use crate::telemetry::{log_security_event, SecurityEvent};

/// Identity of the file that was hashed: device + inode on Unix; volume
/// serial + NTFS file index on Windows (the handle-level identity that
/// survives renames and differs between distinct files). Size on both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    len: u64,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(windows)]
    volume: u32,
    #[cfg(windows)]
    index: u64,
}

impl FileIdentity {
    /// Identity of an open handle.
    fn of_open(file: &File) -> Result<Self, InferenceError> {
        let md = file
            .metadata()
            .map_err(|e| InferenceError::ModelError(format!("cannot stat model: {}", e)))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                len: md.len(),
                dev: md.dev(),
                ino: md.ino(),
            })
        }
        #[cfg(windows)]
        {
            let (volume, index) = windows_file_id(file)?;
            Ok(Self {
                len: md.len(),
                volume,
                index,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self { len: md.len() })
        }
    }

    /// Identity of whatever `path` names now. Unix: `lstat` semantics, so a
    /// symlink swapped in never matches a regular file. Windows: the opened
    /// object's identity, so a link to a *different* file never matches.
    fn at_path(path: &Path) -> Result<Self, InferenceError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let md = std::fs::symlink_metadata(path)
                .map_err(|e| InferenceError::ModelError(format!("cannot re-stat model: {}", e)))?;
            Ok(Self {
                len: md.len(),
                dev: md.dev(),
                ino: md.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            let file = File::open(path)
                .map_err(|e| InferenceError::ModelError(format!("cannot re-open model: {}", e)))?;
            Self::of_open(&file)
        }
    }
}

/// Volume serial number + 64-bit file index of an open handle.
#[cfg(windows)]
fn windows_file_id(file: &File) -> Result<(u32, u64), InferenceError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    // SAFETY: `file` is an open, valid handle for the duration of the call and
    // a zeroed BY_HANDLE_FILE_INFORMATION is a valid out-parameter.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(InferenceError::ModelError(
            "cannot query model file identity".into(),
        ));
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.dwVolumeSerialNumber, index))
}

/// Open a model file for reading without following a symlink at the final
/// path component (Unix O_NOFOLLOW; Windows FILE_FLAG_OPEN_REPARSE_POINT plus
/// a reparse-attribute check). A symlinked model file is refused outright.
pub fn open_nofollow(path: &Path) -> Result<File, InferenceError> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    // Windows: never traverse a final-component reparse point (symlink,
    // junction, mount point, ...). The handle then refers to the reparse
    // point itself and is refused below, closing the check/open race.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        opts.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = opts.open(path).map_err(|e| {
        #[cfg(unix)]
        if e.raw_os_error() == Some(libc::ELOOP) {
            return InferenceError::ModelError(format!(
                "model path is a symbolic link (refused): {}",
                path.display()
            ));
        }
        InferenceError::ModelError(format!("cannot open model for hashing: {}", e))
    })?;
    #[cfg(windows)]
    refuse_windows_reparse_point(&file, path)?;
    Ok(file)
}

/// Fail closed on any reparse point at the opened path (opened, not followed,
/// via `FILE_FLAG_OPEN_REPARSE_POINT`). This includes cloud placeholders,
/// which must be hydrated into regular files before use as model inputs.
#[cfg(windows)]
fn refuse_windows_reparse_point(file: &File, path: &Path) -> Result<(), InferenceError> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    let attributes = file
        .metadata()
        .map_err(|e| InferenceError::ModelError(format!("cannot stat model: {}", e)))?
        .file_attributes();
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(InferenceError::ModelError(format!(
            "model path is a reparse point such as a symbolic link (refused): {}",
            path.display()
        )));
    }
    Ok(())
}

/// Streaming SHA-256 of the file plus the identity of the handle hashed.
pub fn hash_file(path: &Path) -> Result<(String, FileIdentity), InferenceError> {
    let mut file = open_nofollow(path)?;
    let identity = FileIdentity::of_open(&file)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| InferenceError::ModelError(format!("hashing read failed: {}", e)))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok((hex::encode(hasher.finalize()), identity))
}

/// Streaming SHA-256 hex digest of the file at `path`.
pub fn file_sha256_hex(path: &Path) -> Result<String, InferenceError> {
    hash_file(path).map(|(hex, _)| hex)
}

/// Verify the file's digest against the manifest's declared `sha256`.
/// Returns the identity of the hashed file for a post-load re-check.
pub fn verify_manifest_hash(
    model_path: &Path,
    manifest: &ModelManifest,
) -> Result<FileIdentity, InferenceError> {
    let (actual, identity) = hash_file(model_path)?;
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        log_security_event(
            SecurityEvent::ModelHashMismatch,
            "model file hash does not match its manifest",
            &[
                ("model_id", &manifest.model_id),
                ("path", &model_path.display().to_string()),
                ("declared", &manifest.sha256),
                ("actual", &actual),
            ],
        );
        return Err(InferenceError::ModelError(format!(
            "model hash mismatch for '{}': manifest declares sha256 {} but the file hashes to {}",
            manifest.model_id, manifest.sha256, actual
        )));
    }
    Ok(identity)
}

/// Confirm that `path` still names the file whose identity was captured at
/// hash time (no rename/symlink substitution since). `lstat` semantics: a
/// symlink swapped in never matches a regular file's identity.
pub fn check_identity_unchanged(path: &Path, hashed: &FileIdentity) -> Result<(), InferenceError> {
    let now = FileIdentity::at_path(path)?;
    if &now != hashed {
        log_security_event(
            SecurityEvent::ModelHashMismatch,
            "model file was replaced between hash verification and load",
            &[("path", &path.display().to_string())],
        );
        return Err(InferenceError::ModelError(format!(
            "model file changed during load (hash no longer attests the loaded bytes): {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "integrity_tests.rs"]
mod tests;
