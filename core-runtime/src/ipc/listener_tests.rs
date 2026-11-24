//! Unix IPC socket ownership: no destructive startup unlink and no stale
//! cleanup of a replacement pathname. Model-free, no external services.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::{bind_owned_socket, remove_owned_socket, SOCKET_MODE};

#[tokio::test]
async fn occupied_regular_file_survives_server_bind_failure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("serve.sock");
    std::fs::write(&path, b"operator-owned sentinel").unwrap();
    let error = bind_owned_socket(path.to_str().unwrap())
        .expect_err("occupied non-socket path must refuse startup");
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    assert_eq!(std::fs::read(&path).unwrap(), b"operator-owned sentinel");
}

#[tokio::test]
async fn active_socket_survives_second_daemon_bind_attempt() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("active.sock");
    let path_str = path.to_str().unwrap();
    let (first, first_identity) = bind_owned_socket(path_str).unwrap();
    let error = bind_owned_socket(path_str)
        .expect_err("live daemon pathname must not be unlinked");
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    let metadata = std::fs::symlink_metadata(&path).unwrap();
    assert_eq!((metadata.dev(), metadata.ino()), first_identity);
    assert_eq!(metadata.mode() & 0o777, SOCKET_MODE);
    let connected = tokio::net::UnixStream::connect(&path).await;
    assert!(connected.is_ok(), "existing live socket must still accept");
    drop(first);
    remove_owned_socket(path_str, first_identity).unwrap();
    assert!(!path.exists());
    let (second, second_identity) = bind_owned_socket(path_str).unwrap();
    drop(second);
    remove_owned_socket(path_str, second_identity).unwrap();
}

#[tokio::test]
async fn shutdown_never_removes_replacement_regular_file_or_symlink() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("serve.sock");
    let target = root.path().join("do-not-delete");
    let path_str = path.to_str().unwrap();
    std::fs::write(&target, b"important").unwrap();

    let (first, first_identity) = bind_owned_socket(path_str).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"replacement").unwrap();
    remove_owned_socket(path_str, first_identity).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
    drop(first);

    std::fs::remove_file(&path).unwrap();
    let (second, second_identity) = bind_owned_socket(path_str).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    remove_owned_socket(path_str, second_identity).unwrap();
    assert!(std::fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&target).unwrap(), b"important");
    drop(second);
}

#[tokio::test]
async fn shutdown_does_not_unlink_successor_daemon_socket() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("active.sock");
    let path_str = path.to_str().unwrap();
    let (first, first_identity) = bind_owned_socket(path_str).unwrap();
    std::fs::remove_file(&path).unwrap();
    let (second, second_identity) = bind_owned_socket(path_str).unwrap();
    assert_ne!(first_identity, second_identity);
    remove_owned_socket(path_str, first_identity).unwrap();
    assert!(Path::new(path_str).exists());
    assert!(tokio::net::UnixStream::connect(&path).await.is_ok());
    drop(first);
    drop(second);
    remove_owned_socket(path_str, second_identity).unwrap();
    assert!(!path.exists());
}
