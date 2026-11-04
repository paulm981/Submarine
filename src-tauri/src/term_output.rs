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
            // The page drew some.
            _ = page.news.notified() => {
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
}
