//! A terminal's output on its way to the page: over a binary channel, paced
//! by what the page has drawn.
//!
//! Output used to go out as Tauri events: a base64 string in a JSON message
//! that the webview evaluates as a script, every batch, with nothing to stop
//! a fast stream (a busy `tail -f`, a `cat` of a big file) from queuing more
//! than the window could draw. On Windows each of those scripts also passes
//! through the window's own thread, which handles its input too, so a flood
//! could leave the whole app unresponsive.
//!
//! Now each terminal has a `Channel` (passed in by the page when it opens the
//! terminal): batches over a few KB travel as raw bytes, fetched by the page
//! rather than evaluated. And the page acknowledges what xterm has processed
//! (`terminal_output_ack`), so no more than IN_FLIGHT_MAX is ever waiting on
//! the page. The rest waits here. An ack names the channel the output came
//! over, so after a reconnect (a new channel for the same terminal) what the
//! page was still drawing from the old one doesn't count for the new one. The SSH channel keeps being read meanwhile
//! (it must: a reader that stops stalls the whole connection, see
//! ssh_manager::pty_pump), so when the stream stays faster than the window
//! for long, the oldest waiting output past PENDING_MAX is dropped, with a
//! line in the terminal saying how much. That's far more than the scrollback
//! holds, so what's dropped would have scrolled out of reach anyway.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::Notify;

/// Sent to the page and not yet processed by its terminal.
const IN_FLIGHT_MAX: usize = 1024 * 1024;
/// The most one message carries.
const SEND_MAX: usize = 256 * 1024;
/// Waiting here for the page; past this the oldest goes, down to PENDING_KEEP.
const PENDING_MAX: usize = 8 * 1024 * 1024;
const PENDING_KEEP: usize = 4 * 1024 * 1024;
/// Room kept for what waits once a backlog has gone: past this, it's given
/// back (a backlog leaves the buffer at up to twice PENDING_MAX).
const PENDING_SPARE: usize = 512 * 1024;

struct State {
    pending: Vec<u8>,
    in_flight: usize,
    /// The terminal has ended: send what's left and finish.
    closed: bool,
    /// Run once everything has been sent after `closed` (the closed event).
    after: Option<Box<dyn FnOnce() + Send>>,
    /// Dropped since the last line about it in the log.
    skipped_unlogged: usize,
    last_skip_log: Option<Instant>,
}

struct Inner {
    terminal_id: String,
    channel: Channel<InvokeResponseBody>,
    state: Mutex<State>,
    wake: Notify,
}

fn sinks() -> &'static Mutex<HashMap<String, Arc<Inner>>> {
    static SINKS: OnceLock<Mutex<HashMap<String, Arc<Inner>>>> = OnceLock::new();
    SINKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One open terminal's output. Dropping it ends it, as `finish` does
/// (whatever is waiting is still sent).
pub struct OutputSink(Arc<Inner>);

impl OutputSink {
    /// Output for terminal `terminal_id` to `channel`. Replaces an earlier
    /// sink of the same terminal (a reconnect opens it again).
    pub fn open(terminal_id: &str, channel: Channel<InvokeResponseBody>) -> OutputSink {
        let inner = Arc::new(Inner {
            terminal_id: terminal_id.to_string(),
            channel,
            state: Mutex::new(State {
                pending: Vec::new(),
                in_flight: 0,
                closed: false,
                after: None,
                skipped_unlogged: 0,
                last_skip_log: None,
            }),
            wake: Notify::new(),
        });
        if let Ok(mut m) = sinks().lock() {
            m.insert(terminal_id.to_string(), inner.clone());
        }
        tauri::async_runtime::spawn(send_loop(inner.clone()));
        OutputSink(inner)
    }

    /// Queue `buf` for the page; it's left empty.
    pub fn push(&self, buf: &mut Vec<u8>) {
        if buf.is_empty() {
            return;
        }
        let mut log_skip = None;
        if let Ok(mut s) = self.0.state.lock() {
            s.pending.append(buf);
            if let Some(skipped) = trim_pending(&mut s.pending) {
                s.skipped_unlogged += skipped;
                if s.last_skip_log.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
                    log_skip = Some(std::mem::take(&mut s.skipped_unlogged));
                    s.last_skip_log = Some(Instant::now());
                }
            }
        }
        buf.clear();
        self.0.wake.notify_one();
        if let Some(bytes) = log_skip {
            crate::applog::write_line(
                "terminal",
                &format!(
                    "{}: {} of output skipped, it came faster than the window could draw it",
                    self.0.terminal_id,
                    human(bytes)
                ),
            );
        }
    }

    /// The terminal has ended: send what's waiting, then run `after`.
    pub fn finish(&self, after: impl FnOnce() + Send + 'static) {
        if let Ok(mut s) = self.0.state.lock() {
            s.after = Some(Box::new(after));
            s.closed = true;
        }
        self.0.wake.notify_one();
    }
}

impl Drop for OutputSink {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.closed = true;
        }
        self.0.wake.notify_one();
        if let Ok(mut m) = sinks().lock() {
            if m.get(&self.0.terminal_id).is_some_and(|cur| Arc::ptr_eq(cur, &self.0)) {
                m.remove(&self.0.terminal_id);
            }
        }
    }
}

/// When `pending` is past PENDING_MAX: drop its oldest part, down to about
/// PENDING_KEEP and from the start of a line, and put a line saying so in its
/// place. How much was dropped, if any.
fn trim_pending(pending: &mut Vec<u8>) -> Option<usize> {
    if pending.len() <= PENDING_MAX {
        return None;
    }
    let mut cut = pending.len() - PENDING_KEEP;
    // At a line start, so neither a line nor an escape sequence is split.
    if let Some(nl) = pending[cut..].iter().position(|&b| b == b'\n') {
        cut += nl + 1;
    }
    pending.drain(..cut);
    let note = format!(
        "\r\n\x1b[0;2m[Submarine: {} of output skipped, it came faster than it could be shown]\x1b[0m\r\n",
        human(cut)
    );
    pending.splice(0..0, note.into_bytes());
    Some(cut)
}

/// Once `pending` has drained, the room a backlog left it with goes back.
fn release_spare(pending: &mut Vec<u8>) {
    if pending.is_empty() && pending.capacity() > PENDING_SPARE {
        pending.shrink_to(PENDING_SPARE);
    }
}

fn human(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", n / 1024)
    }
}

enum Next {
    Send(Vec<u8>),
    Done(Option<Box<dyn FnOnce() + Send>>),
    Wait,
}

async fn send_loop(inner: Arc<Inner>) {
    loop {
        let next = {
            let Ok(mut s) = inner.state.lock() else { return };
            // Once ended, the page isn't waited for: what's left (at most
            // PENDING_MAX) goes now.
            if !s.pending.is_empty() && (s.closed || s.in_flight < IN_FLIGHT_MAX) {
                let n = s.pending.len().min(SEND_MAX);
                let chunk: Vec<u8> = s.pending.drain(..n).collect();
                release_spare(&mut s.pending);
                s.in_flight += chunk.len();
                Next::Send(chunk)
            } else if s.closed && s.pending.is_empty() {
                Next::Done(s.after.take())
            } else {
                Next::Wait
            }
        };
        match next {
            Next::Send(chunk) => {
                crate::applog::count_output(&inner.terminal_id, chunk.len());
                if inner.channel.send(InvokeResponseBody::Raw(chunk)).is_err() {
                    // The page has let go of this terminal.
                    let after = inner.state.lock().ok().and_then(|mut s| s.after.take());
                    if let Some(after) = after {
                        after();
                    }
                    return;
                }
            }
            Next::Done(after) => {
                if let Some(after) = after {
                    after();
                }
                return;
            }
            Next::Wait => inner.wake.notified().await,
        }
    }
}

/// The page's terminal has processed `bytes` of `terminal_id`'s output, sent
/// over the channel `channel` (its id). An ack for an earlier channel of the
/// same terminal, from before a reconnect, is let go: that output was never
/// counted against this one.
#[tauri::command]
pub fn terminal_output_ack(terminal_id: String, channel: u32, bytes: usize) {
    let inner = sinks().lock().ok().and_then(|m| m.get(&terminal_id).cloned());
    if let Some(inner) = inner.filter(|i| i.channel.id() == channel) {
        if let Ok(mut s) = inner.state.lock() {
            s.in_flight = s.in_flight.saturating_sub(bytes);
        }
        inner.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_flight(id: &str) -> usize {
        let inner = sinks().lock().unwrap().get(id).cloned().unwrap();
        let n = inner.state.lock().unwrap().in_flight;
        n
    }

    fn wait_for(what: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !what() {
            assert!(Instant::now() < until, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn an_ack_from_before_a_reconnect_doesnt_count() {
        let id = "ack-generation-test";
        let old = OutputSink::open(id, Channel::new(|_| Ok(())));
        let old_channel = old.0.channel.id();
        let new = OutputSink::open(id, Channel::new(|_| Ok(())));
        assert_ne!(old_channel, new.0.channel.id());
        new.push(&mut vec![b'x'; IN_FLIGHT_MAX * 2]);
        wait_for(|| in_flight(id) >= IN_FLIGHT_MAX);
        // The page acks what it drew from the old channel: the new one
        // still waits for its own.
        terminal_output_ack(id.to_string(), old_channel, IN_FLIGHT_MAX);
        assert_eq!(in_flight(id), IN_FLIGHT_MAX);
        terminal_output_ack(id.to_string(), new.0.channel.id(), IN_FLIGHT_MAX);
        wait_for(|| new.0.state.lock().unwrap().pending.is_empty());
        drop(old);
        drop(new);
    }

    #[test]
    fn a_drained_backlog_gives_its_room_back() {
        let mut p: Vec<u8> = Vec::with_capacity(PENDING_MAX * 2);
        p.extend_from_slice(&[b'x'; 1024]);
        // Not while anything still waits.
        release_spare(&mut p);
        assert!(p.capacity() >= PENDING_MAX * 2);
        p.clear();
        release_spare(&mut p);
        assert!(p.capacity() <= PENDING_SPARE, "{}", p.capacity());
        // A small buffer is left as it is.
        let mut small: Vec<u8> = Vec::with_capacity(4096);
        release_spare(&mut small);
        assert_eq!(small.capacity(), 4096);
    }

    #[test]
    fn a_short_backlog_is_kept_whole() {
        let mut p = vec![b'x'; PENDING_MAX];
        assert_eq!(trim_pending(&mut p), None);
        assert_eq!(p.len(), PENDING_MAX);
    }

    #[test]
    fn a_long_backlog_drops_its_oldest_lines_with_a_note() {
        let line = b"0123456789abcdefghijklmnopqrstuvwxyz\r\n";
        let mut p: Vec<u8> = line.iter().copied().cycle().take(PENDING_MAX + 1024 * 1024).collect();
        let dropped = trim_pending(&mut p).expect("trimmed");
        let text = String::from_utf8_lossy(&p).into_owned();
        assert!(text.starts_with("\r\n\x1b[0;2m[Submarine: "), "{}", &text[..80]);
        assert!(text.contains("of output skipped"));
        // What's left starts at a line start, right after the note.
        let after_note = &text[text.find("\x1b[0m\r\n").unwrap() + 6..];
        assert!(after_note.starts_with("0123456789"), "{}", &after_note[..40]);
        assert!(p.len() < PENDING_KEEP + 200);
        assert!(dropped >= PENDING_MAX + 1024 * 1024 - PENDING_KEEP);
    }
}
