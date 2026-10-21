//! MCP over the local channel: newline-delimited JSON-RPC 2.0, as an MCP
//! client (Claude Code) speaks it to a stdio server — the helper passes the
//! lines through unchanged. Only what Submarine needs: `initialize`, `ping`,
//! `tools/list`, `tools/call`, and `notifications/cancelled`.

use super::AiState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};

/// The protocol versions this server knows; a client asking for another gets
/// the newest of these.
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_MESSAGE: usize = 1024 * 1024;

const INSTRUCTIONS: &str = "Submarine runs commands on servers the user has shared with you, over the SSH connections it already holds. \
You never get credentials. Every command waits until the user clicks Allow in Submarine's AI panel, and may be denied. \
Call list_shared to see the servers you can use. Output you get back is filtered: secrets it recognises are replaced with [hidden by Submarine].";

pub(crate) fn tools() -> Value {
    json!([
        {
            "name": "list_shared",
            "description": "List the servers the user has shared with you in Submarine. Only these can be used with run_command.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "run_command",
            "description": "Run a shell command on a shared server, as the user's SSH login, once the user allows it in Submarine. \
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
            }
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

/// Serve one connection for the AI tab `tab_id` until the client goes, or
/// the tab's access ends (`closed`).
pub(crate) async fn serve<R, W>(state: AiState, tab_id: String, mut closed: watch::Receiver<bool>, mut reader: BufReader<R>, mut writer: W)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    let write_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let mut line = msg.to_string();
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
    });
    // Requests still being answered, by their JSON-RPC id, so the client can
    // cancel one (and leaving cancels them all).
    let calls: Arc<Mutex<HashMap<String, watch::Sender<bool>>>> = Arc::default();
    loop {
        let line = tokio::select! {
            l = super::channel::read_line(&mut reader, MAX_MESSAGE) => l,
            _ = closed.wait_for(|c| *c) => break,
        };
        let Ok(Some(line)) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let _ = out_tx.send(error(&Value::Null, -32700, "Parse error"));
                continue;
            }
        };
        let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_string) else {
            // A response to something we never asked: nothing to do.
            continue;
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = msg.get("id").cloned() else {
            if method == "notifications/cancelled" {
                if let Some(rid) = params.get("requestId") {
                    if let Some(tx) = calls.lock().ok().and_then(|mut c| c.remove(&rid.to_string())) {
                        let _ = tx.send(true);
                    }
                }
            }
            continue;
        };
        match method.as_str() {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = if VERSIONS.contains(&asked) { asked } else { VERSIONS[0] };
                let _ = out_tx.send(reply(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "submarine", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": INSTRUCTIONS,
                    }),
                ));
            }
            "ping" => {
                let _ = out_tx.send(reply(&id, json!({})));
            }
            "tools/list" => {
                let _ = out_tx.send(reply(&id, json!({ "tools": tools() })));
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let key = id.to_string();
                if let Ok(mut c) = calls.lock() {
                    c.insert(key.clone(), cancel_tx);
                }
                let (state, tab, out, calls) = (state.clone(), tab_id.clone(), out_tx.clone(), calls.clone());
                tokio::spawn(async move {
                    let r = state.call_tool(&tab, &name, &args, cancel_rx).await;
                    let was_cancelled = calls.lock().map(|mut c| c.remove(&key).is_none()).unwrap_or(false);
                    // A cancelled request gets no answer (MCP's rule).
                    if !was_cancelled {
                        let _ = out.send(reply(&id, json!({ "content": [{ "type": "text", "text": r.text }], "isError": r.is_error })));
                    }
                });
            }
            _ => {
                let _ = out_tx.send(error(&id, -32601, "Method not found"));
            }
        }
    }
    // Leaving cancels what's still waiting.
    if let Ok(mut c) = calls.lock() {
        for (_, tx) in c.drain() {
            let _ = tx.send(true);
        }
    }
    drop(out_tx);
    // What's already answered goes out before the connection closes.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), write_task).await;
}
