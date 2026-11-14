//! Opt-in stop-stage trace for the daemon (#184, `N7_STOP_STAGE_TRACE_PLAN.md`).
//!
//! `GG_CORE_STOP_TRACE=1` installs one global subscriber that writes only
//! `gg_core::stop` events and spans (exact target match) as JSON lines to
//! stderr, with a monotonic uptime timestamp. Lines go through a bounded
//! channel to one writer thread: an emitting thread never blocks on stderr,
//! and a full channel drops the line and counts it ([`dropped`]). Off by
//! default; no file path is accepted and no other target is ever emitted.
//! Stage events carry integers, booleans and fixed strings only.

use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::{self, MakeWriter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

/// The only target the stop trace emits.
pub const STOP_TARGET: &str = "gg_core::stop";
/// Environment switch; exactly `1` enables the trace.
pub const ENV: &str = "GG_CORE_STOP_TRACE";
const CHANNEL_LINES: usize = 1024;

static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Lines dropped so far because the writer fell behind.
pub fn dropped() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

/// `true` only for the exact value `1`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Install the trace when `GG_CORE_STOP_TRACE=1`. Returns `false` when the
/// switch is off or a global subscriber already exists (embedders).
pub fn init_from_env() -> bool {
    if !enabled(std::env::var(ENV).ok().as_deref()) {
        return false;
    }
    install_with(io::stderr())
}

fn install_with<S: Write + Send + 'static>(mut sink: S) -> bool {
    let (writer, lines) = channel(CHANNEL_LINES);
    if tracing::subscriber::set_global_default(subscriber(writer)).is_err() {
        return false;
    }
    let spawned = std::thread::Builder::new()
        .name("gg-stop-trace".into())
        .spawn(move || {
            for line in lines {
                let _ = sink.write_all(&line);
            }
        });
    if spawned.is_err() {
        return false;
    }
    started();
    true
}

/// S0: maps the uptime clock of every later line to unix milliseconds.
fn started() {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    tracing::info!(target: STOP_TARGET, unix_ms, dropped = dropped(), "stop trace started");
}

/// The stop-trace subscriber over any line writer (tests capture it).
pub(crate) fn subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let layer = fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .with_timer(fmt::time::Uptime::default())
        .with_writer(writer)
        .with_filter(filter_fn(|meta| meta.target() == STOP_TARGET));
    tracing_subscriber::registry().with(layer)
}

/// A bounded line channel: the writer side never blocks.
pub(crate) fn channel(capacity: usize) -> (ChannelWriter, Receiver<Vec<u8>>) {
    let (tx, rx) = sync_channel(capacity);
    (ChannelWriter { tx }, rx)
}

/// `MakeWriter` whose writers send one formatted line each, without blocking.
#[derive(Clone)]
pub(crate) struct ChannelWriter {
    tx: SyncSender<Vec<u8>>,
}

impl<'a> MakeWriter<'a> for ChannelWriter {
    type Writer = LineWriter;
    fn make_writer(&'a self) -> LineWriter {
        LineWriter {
            buf: Vec::new(),
            tx: self.tx.clone(),
        }
    }
}

/// Buffers one event; sends it on drop, or counts it dropped.
pub(crate) struct LineWriter {
    buf: Vec<u8>,
    tx: SyncSender<Vec<u8>>,
}

impl Write for LineWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        if self.tx.try_send(std::mem::take(&mut self.buf)).is_err() {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Test-only process-wide capture: installs the stop-trace subscriber as
/// the global default (once) over an in-memory sink, so events emitted on
/// any thread (tokio workers, blocking pool) are visible. Lines are kept,
/// never consumed; tests filter by their own unique ids.
#[cfg(test)]
pub(crate) mod test_capture {
    use std::sync::{Mutex, Once};

    use super::{subscriber, MakeWriter};

    static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static INIT: Once = Once::new();

    struct Sink;
    pub(crate) struct SinkWriter(Vec<u8>);

    impl<'a> MakeWriter<'a> for Sink {
        type Writer = SinkWriter;
        fn make_writer(&'a self) -> SinkWriter {
            SinkWriter(Vec::new())
        }
    }

    impl std::io::Write for SinkWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for SinkWriter {
        fn drop(&mut self) {
            let line = String::from_utf8_lossy(&self.0).into_owned();
            LINES.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
    }

    /// Install the global capture (idempotent). Panics if another global
    /// subscriber won, so a test can never pass on silently missing events.
    pub(crate) fn install() {
        INIT.call_once(|| {
            tracing::subscriber::set_global_default(subscriber(Sink))
                .expect("stop-trace test capture must be the global subscriber");
        });
    }

    /// Every captured line so far, parsed.
    pub(crate) fn events() -> Vec<serde_json::Value> {
        let lines = LINES.lock().unwrap_or_else(|e| e.into_inner()).clone();
        lines
            .iter()
            .filter_map(|l| serde_json::from_str(l.trim()).ok())
            .collect()
    }

    /// Every captured line so far, raw (for leak checks).
    pub(crate) fn raw() -> String {
        LINES.lock().unwrap_or_else(|e| e.into_inner()).concat()
    }
}

#[cfg(test)]
#[path = "stop_trace_tests.rs"]
mod tests;
