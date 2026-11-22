//! A server stopping cuts its calls in flight short (#278). `jetcarp mcp`, over stdio or HTTP,
//! answers one call at a time, so a call still running held up its stop: a `job_status` with
//! `wait_s` may wait 600 s, and a session at logout ends what still runs after 90 s. Once the
//! server is to stop (a signal, Ctrl-C, or the HTTP server's stop flag), every call in flight is
//! cancelled as a client's own `notifications/cancelled` cancels it, and its answer says the
//! server is stopping. The calls then have [`GRACE`] to give their answers; a call that does not
//! watch its cancel flag is left, and the server ends without it.
//!
//! The jobs keep what the stop gives them: the server's own jobs stop at their next checkpoint,
//! paused and resumable, and jobs in processes of their own run on.

use crate::ToolOutput;
use crate::stdio::Canceller;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Set once this server is stopping.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// How long the calls in flight have, once the server is to stop, to give their answers before it
/// ends regardless.
pub const GRACE: Duration = Duration::from_secs(2);

/// How an answer cut short by the stop begins.
pub const STOPPING_WORDS: &str = "the server is stopping";

/// Whether this server is stopping: a call cut short says so in its answer.
pub fn is_stopping() -> bool {
    STOPPING.load(Ordering::SeqCst)
}

/// Watch `stop` on a thread of its own. Once it is set, the server is stopping: every call in
/// flight in `cancellers()` is cancelled (here, and in the app for a session attached to it),
/// and again through the [`GRACE`], so a call taken up meanwhile is cancelled too. `settled` is
/// set by the server once it has answered its last call; if it is not set by the end of the
/// grace (a call that does not watch its cancel flag still runs), `end` runs on this thread: what
/// the server does after its last call, and the end of the process.
pub(crate) fn watch(
    stop: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    cancellers: impl Fn() -> Vec<Canceller> + Send + 'static,
    end: impl FnOnce() + Send + 'static,
) {
    watch_with(&STOPPING, stop, settled, cancellers, end);
}

/// [`watch`], setting `stopping` (its tests' own, so the rest of the tests' process is not
/// stopping).
fn watch_with(
    stopping: &'static AtomicBool,
    stop: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    cancellers: impl Fn() -> Vec<Canceller> + Send + 'static,
    end: impl FnOnce() + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("jetcarp-mcp-stopping".into())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if settled.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            stopping.store(true, Ordering::SeqCst);
            let t0 = Instant::now();
            while t0.elapsed() < GRACE {
                for c in cancellers() {
                    c.cancel_all();
                }
                if settled.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !settled.load(Ordering::SeqCst) {
                end();
            }
        });
}

/// A tool's answer cut short by the stop: an error, which says so first (unless the tool's own
/// first words do).
pub(crate) fn cut_short(mut out: ToolOutput) -> ToolOutput {
    out.is_error = true;
    let says = out
        .content
        .first()
        .and_then(|c| c["text"].as_str())
        .is_some_and(|t| t.starts_with(STOPPING_WORDS));
    if !says {
        out.content.insert(
            0,
            json!({"type": "text", "text": format!(
                "{STOPPING_WORDS}, so this call was cut short; call it again once the server runs again"
            )}),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_cut_short_says_so_once() {
        let o = cut_short(ToolOutput::text("42 cells"));
        assert!(o.is_error);
        assert_eq!(o.content.len(), 2);
        assert!(
            o.content[0]["text"]
                .as_str()
                .unwrap()
                .starts_with(STOPPING_WORDS)
        );
        let own = format!("{STOPPING_WORDS}, so this wait ended early");
        let o = cut_short(ToolOutput::text(own.clone()));
        assert!(o.is_error);
        assert_eq!(o.content.len(), 1);
        assert_eq!(o.content[0]["text"], own.as_str());
    }

    /// A call that never returns: the server ends [`GRACE`] after the stop, not before.
    #[test]
    fn the_server_ends_after_the_grace_when_a_call_still_runs() {
        let (stop, settled) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        static STOPPING: AtomicBool = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::channel();
        watch_with(&STOPPING, stop.clone(), settled, Vec::new, move || {
            let _ = tx.send(Instant::now());
        });
        std::thread::sleep(Duration::from_millis(100));
        let t0 = Instant::now();
        stop.store(true, Ordering::SeqCst);
        let ended = rx.recv_timeout(GRACE * 3).expect("the end runs");
        let after = ended - t0;
        assert!(
            after >= GRACE && after < GRACE + Duration::from_secs(1),
            "{after:?}"
        );
        assert!(STOPPING.load(Ordering::SeqCst));
    }

    /// The last call answered within the grace: the server goes on to its own stop.
    #[test]
    fn no_end_once_the_last_call_is_answered() {
        let (stop, settled) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        static STOPPING: AtomicBool = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::channel();
        watch_with(
            &STOPPING,
            stop.clone(),
            settled.clone(),
            Vec::new,
            move || {
                let _ = tx.send(());
            },
        );
        stop.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        settled.store(true, Ordering::SeqCst);
        assert!(rx.recv_timeout(GRACE * 2).is_err(), "the end ran");
    }
}
