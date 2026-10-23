//! MCP over the local channel: newline-delimited JSON-RPC 2.0, as an MCP
//! client (Claude Code) speaks it to a stdio server — the helper passes the
//! lines through unchanged. Only what Submarine needs: `initialize`, `ping`,
//! `tools/list`, `tools/call`, and `notifications/cancelled`.
//!
//! What a client can make it hold is bounded: answers waiting to go out
//! (QUEUE, then reading waits), the `id`s it echoes (a string or a number,
//! MAX_ID long at most), and the calls it answers at once (MAX_CALLS).

use super::AiState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch, Semaphore};

/// The protocol versions this server knows; a client asking for another gets
/// the newest of these.
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_MESSAGE: usize = 1024 * 1024;

const INSTRUCTIONS: &str = "Submarine runs commands on servers the user has shared with you, over the SSH connections it already holds. \
You never get credentials. Depending on the approval level the user chose for each server, a command runs at once or waits until \
the user clicks Allow in Submarine, and may be denied, sometimes with a note saying why. \
Call list_shared to see the servers you can use. Output you get back is filtered: secrets it recognises are replaced with [hidden by Submarine].";

/// The tools, with MCP's standard hints (ToolAnnotations, from 2025-03-26):
/// `list_shared` changes nothing; `run_command` may change anything, on
/// servers out in the world. A client may show them, never trust them.
pub(crate) fn tools() -> Value {
    json!([
        {
            "name": "list_shared",
            "description": "List the servers the user has shared with you in Submarine. Only these can be used with run_command.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "run_command",
            "description": "Run a shell command on a shared server, as the user's SSH login, once the user allows it in Submarine \
(or at once, where the user's settings for that server say so). \
No terminal is attached and nothing is on stdin, so interactive programs and pagers won't work; use non-interactive forms \
(e.g. `journalctl -n 50 --no-pager`, `top -bn1`). The command is stopped at its timeout or past 1 MB of output. \
Returns the exit code and the output (stdout and stderr together), with secrets replaced by [hidden by Submarine].",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "server": { "type": "string", "description": "The server's name, as list_shared gives it." },
                    "command": { "type": "string", "description": "The command line, run by the account's login shell on the server." },
                    "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": super::MAX_TIMEOUT, "description": "Stop it after this many seconds (default 60)." }
                },
                "required": ["server", "command"],
                "additionalProperties": false
            },
            "annotations": { "destructiveHint": true, "openWorldHint": true }
        }
    ])
}

/// A tool's answer: text, and whether it's an error.
pub(crate) struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    pub(crate) fn ok(text: impl Into<String>) -> ToolResult {
        ToolResult { text: text.into(), is_error: false }
    }
    pub(crate) fn err(text: impl Into<String>) -> ToolResult {
        ToolResult { text: text.into(), is_error: true }
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Answers waiting to go out, at most. Past it the connection's reading
/// waits for the client to take some: a client that sends and never reads
/// can't pile them up.
const QUEUE: usize = 16;
/// The longest JSON-RPC `id` taken, written out.
const MAX_ID: usize = 128;
/// `tools/call` requests being answered at once on one connection.
const MAX_CALLS: usize = 8;
/// After the connection ends, how long what's already answered may take to
/// go out.
const LAST_ANSWERS: std::time::Duration = std::time::Duration::from_secs(2);

/// A request's `id` as JSON-RPC allows it, and short: a string or a number,
/// at most MAX_ID long written out. Its written form is its key.
fn valid_id(id: &Value) -> Option<String> {
    match id {
        Value::String(_) | Value::Number(_) => Some(id.to_string()).filter(|k| k.len() <= MAX_ID),
        _ => None,
    }
}

/// The calls being answered on a connection: by id, each call's own number
/// and its cancel signal.
type Calls = HashMap<String, (u64, watch::Sender<bool>)>;

/// Serve one connection for the AI tab `tab_id` until the client goes, or
/// the tab's access ends (`closed`). `held` is dropped as soon as nothing
/// more is read, before what's answered goes out: the AI tab's claim on its
/// one live connection, so a new one (Claude Code reconnecting) may start.
pub(crate) async fn serve<R, W, H>(state: AiState, tab_id: String, mut closed: watch::Receiver<bool>, mut reader: BufReader<R>, mut writer: W, held: H)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (out_tx, mut out_rx) = mpsc::channel::<Value>(QUEUE);
    let mut write_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let mut line = msg.to_string();
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
    });
    // Requests still being answered, by their JSON-RPC id, so the client can
    // cancel one (and leaving cancels them all). Each with a number of its
    // own: once one is cancelled its id may be used again, and the first
    // call's task must not take the new call's entry for its own.
    let calls: Arc<Mutex<Calls>> = Arc::default();
    let mut next_seq: u64 = 0;
    let slots = Arc::new(Semaphore::new(MAX_CALLS));
    let read_loop = async {
        loop {
            let Ok(Some(line)) = super::channel::read_line(&mut reader, MAX_MESSAGE).await else { break };
            if line.trim().is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => {
                    if out_tx.send(error(&Value::Null, -32700, "Parse error")).await.is_err() {
                        break;
                    }
                    continue;
                }
            };
            drop(line);
            let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_string) else {
                // A response to something we never asked: nothing to do.
                continue;
            };
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let Some(id) = msg.get("id") else {
                if method == "notifications/cancelled" {
                    if let Some(key) = params.get("requestId").and_then(valid_id) {
                        if let Some((_, tx)) = calls.lock().ok().and_then(|mut c| c.remove(&key)) {
                            let _ = tx.send(true);
                        }
                    }
                }
                continue;
            };
            // Anything else is answered without being echoed back.
            let Some(key) = valid_id(id) else {
                if out_tx.send(error(&Value::Null, -32600, "Invalid Request: the id must be a string or a number, of at most 128 characters.")).await.is_err() {
                    break;
                }
                continue;
            };
            let id = id.clone();
            drop(msg);
            let answer = match method.as_str() {
                "initialize" => {
                    let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                    let version = if VERSIONS.contains(&asked) { asked } else { VERSIONS[0] };
                    reply(
                        &id,
                        json!({
                            "protocolVersion": version,
                            "capabilities": { "tools": { "listChanged": false } },
                            "serverInfo": { "name": "submarine", "version": env!("CARGO_PKG_VERSION") },
                            "instructions": INSTRUCTIONS,
                        }),
                    )
                }
                "ping" => reply(&id, json!({})),
                "tools/list" => reply(&id, json!({ "tools": tools() })),
                "tools/call" => {
                    let Ok(slot) = slots.clone().try_acquire_owned() else {
                        if out_tx.send(error(&id, -32000, "Too many requests at once: wait for an answer first.")).await.is_err() {
                            break;
                        }
                        continue;
                    };
                    let (cancel_tx, cancel_rx) = watch::channel(false);
                    next_seq += 1;
                    let seq = next_seq;
                    let taken = calls.lock().map(|mut c| {
                        if c.contains_key(&key) {
                            false
                        } else {
                            c.insert(key.clone(), (seq, cancel_tx));
                            true
                        }
                    }).unwrap_or(false);
                    if !taken {
                        if out_tx.send(error(&id, -32600, "Invalid Request: that id is already in use.")).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let args = params.get("arguments").cloned().unwrap_or(json!({}));
                    let (state, tab, out, calls) = (state.clone(), tab_id.clone(), out_tx.clone(), calls.clone());
                    tokio::spawn(async move {
                        let _slot = slot;
                        let r = state.call_tool(&tab, &name, &args, cancel_rx).await;
                        // Still ours (same number): answered. Gone, or another
                        // call's under the same id now: this one was cancelled.
                        let was_cancelled = calls
                            .lock()
                            .map(|mut c| {
                                let ours = c.get(&key).is_some_and(|(s, _)| *s == seq);
                                if ours {
                                    c.remove(&key);
                                }
                                !ours
                            })
                            .unwrap_or(false);
                        // A cancelled request gets no answer (MCP's rule).
                        if !was_cancelled {
                            let _ = out.send(reply(&id, json!({ "content": [{ "type": "text", "text": r.text }], "isError": r.is_error }))).await;
                        }
                    });
                    continue;
                }
                _ => error(&id, -32601, "Method not found"),
            };
            if out_tx.send(answer).await.is_err() {
                break;
            }
        }
    };
    // The tab's access ending ends the connection, even while an answer
    // waits for room.
    tokio::select! {
        _ = read_loop => {}
        _ = closed.wait_for(|c| *c) => {}
    }
    drop(held);
    // Leaving cancels what's still waiting.
    if let Ok(mut c) = calls.lock() {
        for (_, (_, tx)) in c.drain() {
            let _ = tx.send(true);
        }
    }
    drop(out_tx);
    // What's already answered goes out before the connection closes, if the
    // client takes it in time; then the writer goes, with what it holds.
    if tokio::time::timeout(LAST_ANSWERS, &mut write_task).await.is_err() {
        write_task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::exec::Exec;
    use crate::ai::Executor;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, DuplexStream};

    /// An AiState with one AI tab sharing `sess-1`, whose commands run until
    /// stopped, and a connection to it at the MCP level: what the client
    /// writes, and reads.
    fn connection(pipe: usize) -> (AiState, DuplexStream, BufReader<DuplexStream>, tokio::task::JoinHandle<()>) {
        let exec: Executor = Arc::new(|_, _, _, mut stop| {
            Box::pin(async move {
                let _ = stop.wait_for(|s| *s).await;
                // Ending it takes a moment, as on a server.
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok(Exec { stopped: true, ..Exec::default() })
            })
        });
        let state = AiState::new(exec, Box::new(|| {}), None);
        state.turn_on().unwrap();
        state.start_tab("local-1", "Claude").unwrap();
        state.share("local-1", "sess-1", "xyz").unwrap();
        let (client_w, server_r) = tokio::io::duplex(pipe);
        let (server_w, client_r) = tokio::io::duplex(pipe);
        let closed = state.closed_of("local-1");
        let task = tokio::spawn(serve(state.clone(), "local-1".into(), closed, BufReader::new(server_r), server_w, ()));
        (state, client_w, BufReader::new(client_r), task)
    }

    async fn send(w: &mut DuplexStream, msg: Value) {
        let mut line = msg.to_string();
        line.push('\n');
        w.write_all(line.as_bytes()).await.unwrap();
    }

    async fn recv(r: &mut BufReader<DuplexStream>) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), r.read_line(&mut line)).await.expect("an answer").unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    async fn only_short_string_or_number_ids_and_never_echoed() {
        let (_s, mut w, mut r, _t) = connection(1 << 20);
        for bad in [json!("x".repeat(1000)), json!({ "a": 1 }), json!([1, 2]), json!(true), json!(null)] {
            send(&mut w, json!({ "jsonrpc": "2.0", "id": bad, "method": "ping" })).await;
            let a = recv(&mut r).await;
            assert_eq!(a["id"], Value::Null);
            assert_eq!(a["error"]["code"], -32600);
            assert!(a.to_string().len() < 300);
        }
        send(&mut w, json!({ "jsonrpc": "2.0", "id": "abc", "method": "ping" })).await;
        assert_eq!(recv(&mut r).await["id"], "abc");
        send(&mut w, json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" })).await;
        assert_eq!(recv(&mut r).await["id"], 7);
    }

    #[tokio::test]
    async fn at_most_eight_calls_at_once() {
        let (s, mut w, mut r, _t) = connection(1 << 20);
        // On two servers: at most four run at once on one (MAX_RUNNING).
        s.share("local-1", "sess-2", "abc").unwrap();
        let call = |id: u64| {
            let server = if id % 2 == 1 { "xyz" } else { "abc" };
            json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": "run_command", "arguments": { "server": server, "command": "sleep 1" } } })
        };
        // Eight in flight: six allowed and running, two waiting for you.
        let mut sent = 0;
        while sent < MAX_CALLS as u64 {
            let wave = (MAX_CALLS as u64 - sent).min(3);
            for _ in 0..wave {
                sent += 1;
                send(&mut w, call(sent)).await;
            }
            let t = std::time::Instant::now();
            while s.snapshot().pending.len() < wave as usize {
                assert!(t.elapsed() < Duration::from_secs(10));
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if sent < MAX_CALLS as u64 {
                for p in s.snapshot().pending {
                    s.decide(p.id, true).unwrap();
                }
            }
        }
        assert_eq!((s.snapshot().running.len(), s.snapshot().pending.len()), (6, 2));
        // A ninth is turned down at once.
        send(&mut w, call(99)).await;
        let a = recv(&mut r).await;
        assert_eq!(a["id"], 99);
        assert_eq!(a["error"]["code"], -32000);
        // A ping still gets through.
        send(&mut w, json!({ "jsonrpc": "2.0", "id": 100, "method": "ping" })).await;
        assert_eq!(recv(&mut r).await["id"], 100);
    }

    #[tokio::test]
    async fn a_reused_id_is_turned_down() {
        let (s, mut w, mut r, _t) = connection(1 << 20);
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "run_command", "arguments": { "server": "xyz", "command": "x" } } });
        send(&mut w, call.clone()).await;
        let t = std::time::Instant::now();
        while s.snapshot().pending.is_empty() {
            assert!(t.elapsed() < Duration::from_secs(10));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        send(&mut w, call).await;
        let a = recv(&mut r).await;
        assert_eq!(a["error"]["code"], -32600);
        assert_eq!(s.snapshot().pending.len(), 1, "the first still waits");
    }

    async fn until(what: &str, mut f: impl FnMut() -> bool) {
        let t = std::time::Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(10), "waited for {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// An id used again after its call was cancelled, while the cancelled
    /// call is still ending: the cancelled one gets no answer, and the new
    /// one gets its own.
    #[tokio::test]
    async fn an_id_reused_after_a_cancel_gets_its_own_answer() {
        let (s, mut w, mut r, _t) = connection(1 << 20);
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "run_command", "arguments": { "server": "xyz", "command": "x" } } });
        send(&mut w, call.clone()).await;
        until("the first request", || s.snapshot().pending.len() == 1).await;
        s.decide(s.snapshot().pending[0].id, true).unwrap();
        until("it to run", || s.snapshot().running.len() == 1).await;
        send(&mut w, json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 1 } })).await;
        send(&mut w, call).await;
        until("the second request", || s.snapshot().pending.len() == 1).await;
        // The first ends (it takes a moment) while the second waits.
        until("the first to end", || s.snapshot().running.is_empty() && s.snapshot().history.len() == 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        s.decide(s.snapshot().pending[0].id, false).unwrap();
        let a = recv(&mut r).await;
        assert_eq!(a["id"], 1);
        assert_eq!(a["result"]["content"][0]["text"], "Denied by the user.", "{a}");
    }

    /// A client that sends without reading is made to wait, rather than
    /// answers piling up; and its access ending still ends the connection.
    #[tokio::test]
    async fn a_client_that_never_reads_is_held_back() {
        let (s, mut w, r, task) = connection(4096);
        let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }).to_string() + "\n";
        let flood = async {
            loop {
                w.write_all(ping.as_bytes()).await.unwrap();
            }
        };
        assert!(tokio::time::timeout(Duration::from_secs(2), flood).await.is_err(), "the writes were made to wait");
        s.end_tab("local-1");
        tokio::time::timeout(Duration::from_secs(5), task).await.expect("the connection ended").unwrap();
        drop(r);
    }
}
