//! Console shutdown signals for GG-CORE's standalone daemon (#219).
//!
//! Windows group-scoped graceful shutdown uses CTRL_BREAK_EVENT; unlike
//! CTRL_C_EVENT it can be targeted to a process group via Win32
//! GenerateConsoleCtrlEvent. Keep interactive Ctrl+C working unchanged.
//! This is not a substitute for native active-inference drain acceptance.

#[cfg(windows)]
pub(crate) async fn wait() -> std::io::Result<()> {
    // Register both handlers before waiting. Tokio treats Ctrl+C and
    // Ctrl+Break as distinct console events on Windows.
    let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
    let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
    tokio::select! {
        result = ctrl_c.recv() => received(result, "Ctrl+C"),
        result = ctrl_break.recv() => received(result, "Ctrl+Break"),
    }
}

#[cfg(windows)]
fn received(event: Option<()>, name: &str) -> std::io::Result<()> {
    event.ok_or_else(|| std::io::Error::other(format!("{name} signal listener closed")))
}

#[cfg(not(windows))]
pub(crate) async fn wait() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn missing_console_event_is_not_graceful_shutdown_success() {
        assert!(received(None, "Ctrl+Break").is_err());
        assert!(received(Some(()), "Ctrl+Break").is_ok());
        assert!(received(Some(()), "Ctrl+C").is_ok());
    }
}
