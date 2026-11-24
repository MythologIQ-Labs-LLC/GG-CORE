//! Narrow Kubernetes socket-volume preparation command.
//! Runs only in the chart's dedicated, bounded-capability init container.
//! The normal non-root daemon never changes directory ownership or mode.
#![cfg(unix)]

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

const SOCKET_VOLUME: &str = "/var/run/gg-core";

fn parse_target_uid(args: &[String]) -> Result<libc::uid_t, String> {
    if args.len() != 4 || args[2] != "--uid" {
        return Err("expected: prepare-k8s-socket-dir --uid <non-root-uid>".into());
    }
    match args[3].parse::<u32>() {
        Ok(uid) if uid != 0 => Ok(uid as libc::uid_t),
        _ => Err("init socket owner must be a nonzero Unix UID".into()),
    }
}

fn validate_socket_path(path: &str) -> Result<(), String> {
    let socket = Path::new(path);
    if !socket.is_absolute()
        || socket.parent() != Some(Path::new(SOCKET_VOLUME))
        || socket.file_name().is_none()
    {
        return Err("Helm socket must be a file directly in /var/run/gg-core".into());
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    let uid = parse_target_uid(args)?;
    let socket = std::env::var("GG_CORE_SOCKET_PATH")
        .map_err(|_| "GG_CORE_SOCKET_PATH required for Helm volume preparation")?;
    validate_socket_path(&socket)?;
    if unsafe { libc::geteuid() } != 0 {
        return Err("Helm volume initializer must run as root with only CAP_CHOWN".into());
    }

    // Open the fixed chart volume itself, never an arbitrary operator path.
    // Work on the file descriptor so a swapped pathname cannot redirect
    // the chmod/chown to a different host directory.
    let name = CString::new(SOCKET_VOLUME).expect("static Unix mount path");
    let raw = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(format!(
            "open chart socket mount: {}",
            std::io::Error::last_os_error()
        ));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let stat_dir = || -> Result<libc::stat, String> {
        let mut state: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd.as_raw_fd(), &mut state) } != 0 {
            return Err(format!(
                "stat chart socket mount: {}",
                std::io::Error::last_os_error()
            ));
        }
        if (state.st_mode & libc::S_IFMT) != libc::S_IFDIR {
            return Err("chart socket mount is not a directory".into());
        }
        Ok(state)
    };
    let before = stat_dir()?;
    if before.st_uid != 0 && before.st_uid != uid {
        return Err("chart socket mount belongs to an unexpected Unix UID".into());
    }
    if before.st_uid == uid && before.st_mode & 0o777 == 0o700 {
        return Ok(());
    }
    if before.st_uid != 0 {
        return Err("non-root chart mount owner must already have mode 0700".into());
    }
    if unsafe { libc::fchmod(fd.as_raw_fd(), 0o700) } != 0 {
        return Err(format!(
            "chmod chart socket mount: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Preserve the volume's existing GID. Mode 0700 denies its group access.
    if unsafe { libc::fchown(fd.as_raw_fd(), uid, !0 as libc::gid_t) } != 0 {
        return Err(format!(
            "chown chart socket mount: {}",
            std::io::Error::last_os_error()
        ));
    }
    let after = stat_dir()?;
    if after.st_uid != uid || after.st_mode & 0o777 != 0o700 {
        return Err("chart socket mount owner/mode postcondition failed".into());
    }
    Ok(())
}

/// Only the chart initializer should invoke the privileged filesystem step.
pub fn cli_exit(args: &[String]) -> std::process::ExitCode {
    match run(args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Helm socket directory preparation refused: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_uid_or_cli_shape() {
        for uid in ["0", "-1", "not-an-id", "4294967296"] {
            assert!(parse_target_uid(&[
                "gg-core".into(),
                "prepare-k8s-socket-dir".into(),
                "--uid".into(),
                uid.into()
            ])
            .is_err());
        }
        assert_eq!(
            parse_target_uid(&[
                "gg-core".into(),
                "prepare-k8s-socket-dir".into(),
                "--uid".into(),
                "1000".into()
            ])
            .unwrap(),
            1000
        );
    }

    #[test]
    fn refuses_arbitrary_chown_target_and_path_traversal() {
        for invalid in [
            "/tmp/gg-core.sock",
            "/var/run/gg-core/../other/gg-core.sock",
            "/var/run/gg-core/private/gg-core.sock",
            "gg-core.sock",
            "/var/run/gg-core/",
        ] {
            assert!(validate_socket_path(invalid).is_err(), "{invalid}");
        }
        validate_socket_path("/var/run/gg-core/gg-core.sock").unwrap();
    }
}
