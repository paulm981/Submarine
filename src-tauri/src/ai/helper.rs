//! `submarine --mcp`: the helper an AI tool started from Home's menu runs as
//! its MCP server. No window and no app: it finds the running Submarine
//! through the channel's address in its environment, introduces itself with
//! its AI tab's token, and then passes MCP's lines between its stdin/stdout
//! and the app unchanged.
//!
//! main.rs runs it before anything else, so the app's own capture of
//! stdout and stderr (applog.rs) never starts and stdout stays the MCP
//! channel. On Windows the release build is a windowed program; its stdin and
//! stdout are still the pipes the AI tool hands it.
//!
//! Started any other way (by hand, or by an AI tool outside Submarine) it
//! has no token and connects to nothing.

use super::channel;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// The helper's whole run; the process's exit code.
pub fn main() -> i32 {
    let addr = std::env::var(channel::ENV_ADDR).unwrap_or_default();
    let token = std::env::var(channel::ENV_TOKEN).unwrap_or_default();
    if addr.is_empty() || token.is_empty() {
        eprintln!("submarine --mcp: only an AI tool started from Submarine's Home menu can connect to Submarine.");
        return 2;
    }
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("submarine --mcp: {e}");
            return 1;
        }
    };
    let result = rt.block_on(relay(tokio::io::stdin(), tokio::io::stdout(), &addr, &token));
    // stdin's reader thread may still be blocked on a read: don't wait for it.
    rt.shutdown_background();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("submarine --mcp: {e}");
            1
        }
    }
}

/// Connect to the app at `addr` as the AI tab `token` names, then pass
/// `input` to it and what it sends to `output`, until either side ends.
pub(crate) async fn relay<I, O>(mut input: I, mut output: O, addr: &str, token: &str) -> Result<(), String>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
{
    let stream = channel::connect(addr)
        .await
        .map_err(|e| format!("Couldn't reach Submarine ({e}). Is it running, with AI tools on in Settings?"))?;
    let mut stream = BufReader::new(stream);
    channel::hello(&mut stream, token).await?;
    let (mut from_app, mut to_app) = tokio::io::split(stream);
    let up = async {
        let _ = tokio::io::copy(&mut input, &mut to_app).await;
        let _ = to_app.shutdown().await;
    };
    let down = async {
        let _ = tokio::io::copy(&mut from_app, &mut output).await;
        let _ = output.flush().await;
    };
    // The app closing ends the helper; the AI tool closing stdin ends it too,
    // once the app has answered what it had.
    tokio::select! {
        _ = down => {}
        _ = async { up.await; std::future::pending::<()>().await } => {}
    }
    Ok(())
}
