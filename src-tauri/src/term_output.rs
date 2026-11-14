//! A terminal's output on its way to the page, over a binary channel.
//!
//! The original project sends a terminal's output as `terminal-output-{id}`
//! events: a base64 string in a JSON message that the webview evaluates as a
//! script. On Windows each of those scripts also passes through the window's
//! own thread, which handles its input too. Here each terminal has a
//! `Channel` instead (passed in by the page when it opens the terminal), and
//! a batch travels as raw bytes, fetched by the page rather than evaluated.
//!
//! What reaches the page is paced as in the original project
//! (ssh_manager::Outflow, with the page's `terminal_drawn` reports); this is
//! only the last step, the `flush` a pump hands each batch to.

use tauri::ipc::{Channel, InvokeResponseBody};

/// Hand `buf` to the page as one message; it's left empty.
pub fn send(terminal_id: &str, channel: &Channel<InvokeResponseBody>, buf: &mut Vec<u8>) {
    if buf.is_empty() {
        return;
    }
    crate::applog::count_output(terminal_id, buf.len());
    let _ = channel.send(InvokeResponseBody::Raw(std::mem::take(buf)));
}

/// Paces a terminal's output to the page as an SSH terminal's is (Outflow),
/// for one whose output comes from elsewhere: a shell on this computer
/// (local_term.rs), read on threads of its own. Each batch comes through
/// `batches`; an empty one, or the senders going, ends it: what's left goes
/// and `after` runs.
pub async fn pace(
    terminal_id: String,
    channel: Channel<InvokeResponseBody>,
    mut batches: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    after: impl FnOnce(),
) {
    use crate::ssh_manager::{page_end, page_start, Outflow};
    let page = page_start(&terminal_id);
    let mut flow = Outflow::default();
    let mut flush = |b: &mut Vec<u8>| send(&terminal_id, &channel, b);
    let park = || tokio::time::Instant::now() + std::time::Duration::from_secs(24 * 3600);
    let timer = tokio::time::sleep_until(park());
    tokio::pin!(timer);
    let mut out: Vec<u8> = Vec::new();
    loop {
        tokio::select! {
            batch = batches.recv() => match batch {
                Some(mut b) if !b.is_empty() => {
                    out.append(&mut b);
                    let next = if flow.send(&mut out, &page, &mut flush) { park() } else { flow.wait_until() };
                    timer.as_mut().reset(next);
                }
                _ => break,
            },
            _ = &mut timer => {
                let next = if flow.send(&mut out, &page, &mut flush) { park() } else { flow.wait_until() };
                timer.as_mut().reset(next);
            }
            // The page drew some, or keys were typed (local_term's writes:
            // ssh_manager::page_keys).
            _ = page.news.notified() => {
                if page.take_typed() {
                    flow.typed(&page);
                }
                if page.take_interrupt() {
                    flow.interrupted(&mut out, &page);
                }
                if flow.holds() {
                    let next = if flow.send(&mut out, &page, &mut flush) { park() } else { flow.wait_until() };
                    timer.as_mut().reset(next);
                }
            }
        }
    }
    flow.finish(&mut out, &page, &mut flush);
    page_end(&terminal_id, &page);
    after();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_batch_goes_as_raw_bytes_and_leaves_the_buffer_empty() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&got);
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(b) = body {
                seen.lock().unwrap().push(b);
            }
            Ok(())
        });
        let mut buf = b"hello".to_vec();
        send("t-send", &channel, &mut buf);
        assert!(buf.is_empty());
        // Nothing to send sends nothing.
        send("t-send", &channel, &mut buf);
        assert_eq!(*got.lock().unwrap(), vec![b"hello".to_vec()]);
    }

    /// What a shell wrote reaches the page in order, all of it once the page
    /// draws, and the end comes after the last of it.
    #[tokio::test]
    async fn paced_output_arrives_whole_then_ends() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&got);
        let id = "t-pace";
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(b) = body {
                // The page draws what it gets.
                crate::ssh_manager::page_drew("t-pace", b.len() as u64);
                seen.lock().unwrap().extend_from_slice(&b);
            }
            Ok(())
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let ended = Arc::new(Mutex::new(false));
        let end = Arc::clone(&ended);
        let pump = tokio::spawn(pace(id.into(), channel, rx, move || *end.lock().unwrap() = true));
        let mut want = Vec::new();
        for i in 0..64u8 {
            let b = vec![i; 200 << 10];
            want.extend_from_slice(&b);
            tx.send(b).unwrap();
        }
        tx.send(Vec::new()).unwrap();
        pump.await.unwrap();
        assert!(*ended.lock().unwrap());
        assert_eq!(got.lock().unwrap().len(), want.len());
        assert!(*got.lock().unwrap() == want, "in order, nothing cut");
    }

    /// Ctrl+C typed into a Home terminal behind on a flood cuts what waits,
    /// as in an SSH terminal (local_term's writes go to page_keys).
    #[tokio::test(start_paused = true)]
    async fn ctrl_c_cuts_what_waits_for_the_page() {
        let id = "t-pace-ctrl-c";
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&got);
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(b) = body {
                seen.lock().unwrap().extend_from_slice(&b);
            }
            Ok(())
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let pump = tokio::spawn(pace(id.into(), channel, rx, || {}));
        let settle = || async {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        };
        tx.send(b"hello\n".to_vec()).unwrap();
        settle().await;
        crate::ssh_manager::page_drew(id, 6);
        // 16 MB of lines while the page draws nothing more.
        let lines: Vec<u8> = (0..1_600_000u32).flat_map(|i| format!("line {i:05}\n").into_bytes()).collect();
        for part in lines.chunks(256 << 10) {
            tx.send(part.to_vec()).unwrap();
        }
        settle().await;
        crate::ssh_manager::page_keys(id, b"\x03");
        settle().await;
        let drawn = got.lock().unwrap().len() as u64;
        crate::ssh_manager::page_drew(id, drawn - 6);
        tx.send(Vec::new()).unwrap();
        pump.await.unwrap();
        let shown = String::from_utf8_lossy(&got.lock().unwrap()).into_owned();
        assert!(shown.contains("of output skipped"), "what waited was cut");
        assert!(shown.len() < 6 + lines.len(), "not all of it went to the page");
        assert!(shown.trim_end().ends_with("line 1599999"), "the newest lines are kept");
    }

    // ── Speed ─────────────────────────────────────────────────────────

    /// Made-up output, the same every time: log lines with a colored word in
    /// each (`colored`), or plain ones.
    fn lines(bytes: usize, colored: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes + 128);
        let mut i = 0u32;
        while out.len() < bytes {
            let level = if colored { "\x1b[1;32mINFO\x1b[0m" } else { "INFO" };
            out.extend_from_slice(format!("2026-11-07 10:41:{:02} {level} api request {i} handled from 192.0.2.{} in {} ms\r\n", i % 60, i % 250, i % 900).as_bytes());
            i += 1;
        }
        out
    }

    /// What the backend does to a megabyte of output on its way to the page:
    /// the filter (term_filter.rs), in reads the size SSH hands them over,
    /// and the batch to the channel. Returns megabytes a second, the best of
    /// `tries` (a busy machine slows some of them, never all).
    fn megabytes_a_second(data: &[u8], tries: u32) -> f64 {
        let channel = Channel::new(|_| Ok(()));
        let mut best = f64::MAX;
        for _ in 0..tries {
            let mut limiter = crate::term_filter::CsiLimiter::default();
            limiter.set_size(120, 40);
            let mut out = Vec::new();
            let mut passed = 0;
            let t = std::time::Instant::now();
            for read in data.chunks(32 * 1024) {
                limiter.filter(read, &mut out);
                if out.len() >= 256 * 1024 {
                    passed += out.len();
                    send("t-speed", &channel, &mut out);
                }
            }
            passed += out.len();
            send("t-speed", &channel, &mut out);
            best = best.min(t.elapsed().as_secs_f64());
            assert_eq!(passed, data.len(), "real output passes whole");
        }
        data.len() as f64 / (1024.0 * 1024.0) / best
    }

    /// A budget for the backend's work on output, about a tenth of
    /// what it does today, so only a change that makes it many times slower
    /// fails, never a busy machine: an unoptimized build (as the tests are)
    /// does about 220 MB/s of plain lines and 135 of colored ones on a slow
    /// machine; a release build 2000 and 870. The page draws 5 to 15 MB/s,
    /// so while this passes the backend isn't what's slow.
    #[test]
    fn filtering_and_sending_output_stays_fast() {
        let plain = megabytes_a_second(&lines(4 << 20, false), 5);
        let colored = megabytes_a_second(&lines(4 << 20, true), 5);
        println!("plain {plain:.0} MB/s, colored {colored:.0} MB/s");
        assert!(plain >= PLAIN_MB_S, "plain output: {plain:.0} MB/s, under the budget of {PLAIN_MB_S}");
        assert!(colored >= COLORED_MB_S, "colored output: {colored:.0} MB/s, under the budget of {COLORED_MB_S}");
    }
    const PLAIN_MB_S: f64 = 20.0;
    const COLORED_MB_S: f64 = 12.0;
}
