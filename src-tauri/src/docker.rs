// Docker management commands. Each runs through a fresh exec channel on
// the live SSH session, so the user's PTY is untouched. All actions are
// non-destructive — no remove / rm / down here per product directive.
// Where the server refuses the user's own `docker` (they aren't in the
// docker group), the commands run as root through sudo or dzdo, the way the
// Info panel's actions do (lib.rs `run_as_root`): without a password where
// the server allows it, `dzdo su -` included, else with the one the user
// types, kept for the tab.
// The cross-platform story: every command uses the standard `docker` CLI,
// which behaves identically on Linux / macOS / Windows hosts. The two
// places we diverge are (a) `docker compose` (modern plugin) vs the old
// `docker-compose` script, and (b) the in-container shell for an exec
// terminal — both handled inline below.

use crate::ssh_manager::SshState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex as TokioMutex;
use tokio::task::AbortHandle;

const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024; // 4 MB cap for logs

// State for active streaming sessions (live logs, etc.) keyed by a
// frontend-generated stream id so the user can stop them individually.
#[derive(Default)]
pub struct DockerStreams {
    pub streams: Arc<TokioMutex<HashMap<String, AbortHandle>>>,
}
impl DockerStreams {
    pub fn new() -> Self { Self::default() }
}

// ---- helpers ----------------------------------------------------------------

// POSIX-safe quoting (remote_sh::quote). Works on the typical Linux/macOS
// docker host. Windows servers running docker almost always use WSL2
// (POSIX), so this is the right default.
use crate::remote_sh::quote as shq;

// Validate a docker object name (container/image/volume/network/project).
// Docker's accepted character set is roughly [a-zA-Z0-9_.-], plus colon
// and slash for image:tag and repo paths. Reject anything else so we never
// hand a shell-metachar-laced string to the remote shell.
fn is_safe_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 512 {
        return false;
    }
    // A leading '-' would be read as a flag even though the value is quoted:
    // `docker inspect '-f{{...}}'` is still a flag to docker, not a name.
    // No real container/image/volume/network name starts with one.
    if s.starts_with('-') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | ':' | '/'))
}

// A compose file path can include spaces and unusual characters; we
// validate only against shell-injection risk (no $`;|&<>(){}newline).
fn is_safe_path(s: &str) -> bool {
    if s.is_empty() || s.len() > 4096 {
        return false;
    }
    // A leading '-' would be read as a flag where the path is a bare argument
    // (e.g. `head … <path>`). A real path can always be written `./-name`.
    if s.starts_with('-') {
        return false;
    }
    !s.chars().any(|c| matches!(c,
        '\0' | '\n' | '\r' | ';' | '|' | '&' | '`' | '$' |
        '<' | '>' | '(' | ')' | '{' | '}' | '*' | '?' | '!' | '\\'
    ))
}

async fn run_capture(
    state: &SshState,
    session_id: &str,
    cmd: &str,
    timeout_secs: u64,
) -> Result<(i32, String), String> {
    let session_arc = {
        let connections = state.connections.lock().await;
        connections
            .get(session_id)
            .map(Arc::clone)
            .ok_or_else(|| "Session not connected".to_string())?
    };
    let channel = {
        let session = session_arc.lock().await;
        session
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?
    };
    let wrapped = format!("{}\necho __DOCK_EXIT:$?", cmd);
    channel
        .exec(true, wrapped.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = channel.into_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let read_fut = async {
        let mut tmp = [0u8; 8192];
        loop {
            match stream.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > MAX_OUTPUT_BYTES {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(timeout_secs), read_fut)
        .await
        .map_err(|_| format!("docker command timed out after {}s", timeout_secs))?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (code, out) = if let Some(idx) = text.rfind("__DOCK_EXIT:") {
        let after = &text[idx + "__DOCK_EXIT:".len()..];
        let code: i32 = after
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("1")
            .parse()
            .unwrap_or(1);
        (code, text[..idx].trim_end_matches('\n').to_string())
    } else {
        (1, text.trim_end_matches('\n').to_string())
    };
    Ok((code, out))
}

/// A docker command, a word per argument: `docker …`, or compose's words
/// (after `docker compose`, or `docker-compose` where the plugin isn't
/// there). As words, it runs the same through the user's shell and as root.
enum Cmd {
    Docker(Vec<String>),
    Compose(Vec<String>),
}

fn words(first: &[&str], rest: &[String]) -> Vec<String> {
    first.iter().map(|w| w.to_string()).chain(rest.iter().cloned()).collect()
}

/// `docker` and these words.
fn docker(args: &[&str]) -> Cmd {
    Cmd::Docker(words(&["docker"], &args.iter().map(|w| w.to_string()).collect::<Vec<_>>()))
}

impl Cmd {
    /// As the user's shell runs it.
    fn shell(&self) -> String {
        let join = |w: &[String]| w.iter().map(|w| shq(w)).collect::<Vec<_>>().join(" ");
        match self {
            Cmd::Docker(w) => join(w),
            Cmd::Compose(w) => format!("{} {}", compose_prefix(), join(w)),
        }
    }
}

/// The daemon's socket refused the user (it's root's and the docker
/// group's): root may do it. Only that: a daemon that's down, or a
/// "permission denied" from inside a container, isn't root's to fix.
fn daemon_refused(out: &str) -> bool {
    let lower = out.to_lowercase();
    lower.contains("permission denied while trying to connect to the docker")
        || lower.contains("docker.sock: connect: permission denied")
}

/// Run a docker command as the user, or as root where the server refuses
/// the user's own (and, once root has worked, from then on for this tab,
/// SshState::docker_root). Returns (success,
/// output, as root); output is stdout and stderr together. `Err` with a
/// [SUDO] code when root needs a password (the tab asks for it and calls
/// again with `password`) or isn't allowed.
async fn run_docker_smart(
    state: &SshState,
    session_id: &str,
    cmd: &Cmd,
    timeout: u64,
    password: Option<String>,
) -> Result<(bool, String, bool), String> {
    let as_root = password.is_some() || state.docker_root.lock().await.contains(session_id);
    if !as_root {
        let (code, out) = run_capture(state, session_id, &format!("{} 2>&1", cmd.shell()), timeout).await?;
        if code == 0 || !daemon_refused(&out) {
            return Ok((code == 0, out, false));
        }
    }
    let root = |args: Vec<String>, password: Option<String>| async move {
        crate::run_as_root(state, session_id, &args, password, timeout)
            .await
            .map(|(code, out, _)| (code, out))
    };
    let (code, out) = match cmd {
        Cmd::Docker(w) => root(w.clone(), password).await?,
        Cmd::Compose(w) => {
            let (code, out) = root(words(&["docker", "compose"], w), password.clone()).await?;
            if code != 0 && out.contains("is not a docker command") {
                root(words(&["docker-compose"], w), password).await?
            } else {
                (code, out)
            }
        }
    };
    state.docker_root.lock().await.insert(session_id.to_string());
    Ok((code == 0, out, true))
}

#[derive(Serialize)]
pub struct DockerActionResult {
    pub success: bool,
    pub output: String,
    pub used_sudo: bool,
}

#[derive(Serialize)]
pub struct DockerTextResult {
    pub success: bool,
    pub data: String,
    pub used_sudo: bool,
}

// ---- container actions -----------------------------------------------------

#[tauri::command]
pub async fn ssh_docker_container_action(
    state: tauri::State<'_, SshState>,
    session_id: String,
    container: String,
    action: String,
    password: Option<String>,
) -> Result<DockerActionResult, String> {
    if !is_safe_name(&container) {
        return Err("invalid container name".into());
    }
    // Allow-list: no remove / rm — explicit product directive.
    let valid = matches!(
        action.as_str(),
        "start" | "stop" | "restart" | "pause" | "unpause" | "kill"
    );
    if !valid {
        return Err(format!("invalid action: {}", action));
    }
    let cmd = docker(&[&action, &container]);
    let (success, output, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 60, password).await?;
    Ok(DockerActionResult {
        success,
        output,
        used_sudo,
    })
}

// ---- inspect / stats / logs (one-shot) -------------------------------------

#[tauri::command]
pub async fn ssh_docker_inspect(
    state: tauri::State<'_, SshState>,
    session_id: String,
    kind: String,
    name: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    if !is_safe_name(&name) {
        return Err("invalid name".into());
    }
    let prefix = match kind.as_str() {
        "container" => "container",
        "image" => "image",
        "volume" => "volume",
        "network" => "network",
        _ => return Err("invalid inspect kind".into()),
    };
    let cmd = docker(&[prefix, "inspect", &name]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 20, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

#[tauri::command]
pub async fn ssh_docker_stats(
    state: tauri::State<'_, SshState>,
    session_id: String,
    // Optional container filter. When Some, we ask docker for just that
    // one container's stats — an ~80 ms probe on a quiet daemon and a
    // few hundred bytes on the wire. When None, keep the full-list
    // behaviour so callers that render a resource panel across all
    // containers still work unchanged.
    container: Option<String>,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    // Lean field list (only what the Stats panel renders).
    const STATS_FMT: &str = "{{.Name}}|{{.CPUPerc}}|{{.MemUsage}}|{{.MemPerc}}|{{.NetIO}}|{{.BlockIO}}|{{.PIDs}}";
    let cmd = match container.as_deref() {
        Some(name) => {
            if !is_safe_name(name) {
                return Err("invalid container name".into());
            }
            docker(&["stats", "--no-stream", "--format", STATS_FMT, name])
        }
        None => docker(&["stats", "--no-stream", "--format", STATS_FMT]),
    };
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 20, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

#[tauri::command]
pub async fn ssh_docker_logs(
    state: tauri::State<'_, SshState>,
    session_id: String,
    container: String,
    tail: u32,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    if !is_safe_name(&container) {
        return Err("invalid container name".into());
    }
    let cap_tail = tail.min(50_000).to_string();
    let cmd = docker(&["logs", "--tail", &cap_tail, "--timestamps", &container]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 30, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

// ---- networks --------------------------------------------------------------

#[tauri::command]
pub async fn ssh_docker_networks(
    state: tauri::State<'_, SshState>,
    session_id: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    let cmd = docker(&["network", "ls", "--format", "{{.ID}}|{{.Name}}|{{.Driver}}|{{.Scope}}"]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 15, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

// ---- lightweight per-resource list commands --------------------------------
// Separate from the umbrella `ssh_info_probe_section "docker"` so the Docker
// tab can ask for just what the active sub-tab needs — no `docker stats`,
// `docker system df`, or `docker images` round-trips on first paint when the
// user only wanted the container list. Each one runs a single docker call so
// the round-trip stays in the ~100–300 ms range on a quiet daemon.

#[tauri::command]
pub async fn ssh_docker_containers(
    state: tauri::State<'_, SshState>,
    session_id: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    // Lean field list (only what the UI renders) instead of `{{json .}}` — skips
    // the unused, often-bulky .Labels/.Mounts/.Networks/.Command per container.
    let cmd = docker(&["ps", "-a", "--format", "{{.ID}}|{{.Image}}|{{.Names}}|{{.Status}}|{{.State}}|{{.Ports}}"]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 15, password).await?;
    Ok(DockerTextResult { success, data, used_sudo })
}

#[tauri::command]
pub async fn ssh_docker_images_list(
    state: tauri::State<'_, SshState>,
    session_id: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    let cmd = docker(&["images", "--format", "{{.Repository}}|{{.Tag}}|{{.ID}}|{{.Size}}|{{.CreatedSince}}"]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 15, password).await?;
    Ok(DockerTextResult { success, data, used_sudo })
}

#[tauri::command]
pub async fn ssh_docker_volumes_list(
    state: tauri::State<'_, SshState>,
    session_id: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    let cmd = docker(&["volume", "ls", "--format", "{{.Name}}|{{.Driver}}|{{.Mountpoint}}"]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 15, password).await?;
    Ok(DockerTextResult { success, data, used_sudo })
}

// ---- compose ---------------------------------------------------------------

// Compose plugin first (`docker compose`); fall back to legacy script if
// the plugin isn't installed. Caller never has to care which is present.
fn compose_prefix() -> &'static str {
    "if docker compose version >/dev/null 2>&1; then CCMD='docker compose'; else CCMD='docker-compose'; fi; $CCMD"
}

#[tauri::command]
pub async fn ssh_docker_compose_list(
    state: tauri::State<'_, SshState>,
    session_id: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    // Plain tabular output (NAME / STATUS / CONFIG FILES) — `--format json`
    // turned out to be flaky across docker versions: some emit a JSON array,
    // others JSONL, others prefix deprecation warnings to the JSON which
    // breaks `JSON.parse`. The text columns are stable since the compose
    // plugin was added and are trivially parseable on the frontend.
    let cmd = Cmd::Compose(vec!["ls".into(), "--all".into()]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 20, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

// Per-container compose info. Reads docker's own labels — every container
// started by compose carries `com.docker.compose.project`, `…service`, and
// `…project.config_files`. Output is pipe-separated, one container per line:
//   <name>|<project>|<config_files>|<service>|<state>
// Containers without compose labels still appear (project + service empty)
// so the frontend can filter them out without a second round-trip.
#[tauri::command]
pub async fn ssh_docker_compose_per_container(
    state: tauri::State<'_, SshState>,
    session_id: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    // The format holds double quotes (Go template's `{{.Label "key"}}`);
    // as one word it's quoted whole for the shell.
    let cmd = docker(&[
        "ps",
        "-a",
        "--no-trunc",
        "--format",
        r#"{{.Names}}|{{.Label "com.docker.compose.project"}}|{{.Label "com.docker.compose.project.config_files"}}|{{.Label "com.docker.compose.service"}}|{{.State}}"#,
    ]);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 15, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

#[tauri::command]
pub async fn ssh_docker_compose_services(
    state: tauri::State<'_, SshState>,
    session_id: String,
    compose_file: String,
    password: Option<String>,
) -> Result<DockerTextResult, String> {
    if !is_safe_path(&compose_file) {
        return Err("invalid compose file path".into());
    }
    let mut args = vec!["-f".to_string(), compose_file];
    args.extend(["ps", "--all", "--format", "json"].map(String::from));
    let cmd = Cmd::Compose(args);
    let (success, data, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 30, password).await?;
    Ok(DockerTextResult {
        success,
        data,
        used_sudo,
    })
}

#[tauri::command]
pub async fn ssh_docker_compose_action(
    state: tauri::State<'_, SshState>,
    session_id: String,
    compose_file: String,
    action: String,
    service: Option<String>,
    password: Option<String>,
) -> Result<DockerActionResult, String> {
    if !is_safe_path(&compose_file) {
        return Err("invalid compose file path".into());
    }
    // No `down` — product directive: no delete-like operations.
    let valid = matches!(action.as_str(), "up" | "restart" | "pull" | "start" | "stop");
    if !valid {
        return Err(format!("invalid compose action: {}", action));
    }
    if let Some(s) = service.as_deref() {
        if !is_safe_name(s) {
            return Err("invalid service name".into());
        }
    }
    let mut args = vec!["-f".to_string(), compose_file];
    // `up` runs detached; without `-d` it'd hang the channel waiting on
    // the foreground process group.
    match action.as_str() {
        "up" => args.extend(["up", "-d", "--no-deps"].map(String::from)),
        other => args.push(other.to_string()),
    }
    args.extend(service);
    let (success, output, used_sudo) = run_docker_smart(&state, &session_id, &Cmd::Compose(args), 300, password).await?;
    Ok(DockerActionResult {
        success,
        output,
        used_sudo,
    })
}

#[tauri::command]
pub async fn ssh_docker_compose_view(
    state: tauri::State<'_, SshState>,
    session_id: String,
    compose_file: String,
) -> Result<DockerTextResult, String> {
    if !is_safe_path(&compose_file) {
        return Err("invalid compose file path".into());
    }
    // Plain cat, no docker — works even if the file's project is stopped.
    // Hard-cap via head so a misconfigured path can't dump a binary at us.
    let cmd = format!("head -c 524288 {} 2>&1", shq(&compose_file));
    let (code, data) = run_capture(&state, &session_id, &cmd, 10).await?;
    Ok(DockerTextResult {
        success: code == 0,
        data,
        used_sudo: false,
    })
}

// ---- prune (safe defaults only) --------------------------------------------

// We expose ONLY the conservative variants — `prune` without `-a` or
// `--volumes`. That removes stopped containers, dangling images, unused
// networks, and the build cache. Named volumes, tagged images, and
// running containers are never touched. Anything more aggressive would
// risk silent data loss and is excluded by product directive.
#[tauri::command]
pub async fn ssh_docker_prune(
    state: tauri::State<'_, SshState>,
    session_id: String,
    scope: String,
    password: Option<String>,
) -> Result<DockerActionResult, String> {
    let kind = match scope.as_str() {
        "containers" => "container",
        "images" => "image", // dangling only — no -a
        "networks" => "network",
        "builder" => "builder",
        "system" => "system", // safe defaults
        _ => return Err("invalid prune scope".into()),
    };
    let cmd = docker(&[kind, "prune", "-f"]);
    let (success, output, used_sudo) = run_docker_smart(&state, &session_id, &cmd, 120, password).await?;
    Ok(DockerActionResult {
        success,
        output,
        used_sudo,
    })
}

// ---- live logs streaming ---------------------------------------------------

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ssh_docker_logs_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, SshState>,
    streams: tauri::State<'_, DockerStreams>,
    session_id: String,
    stream_id: String,
    container: String,
    tail: u32,
    password: Option<String>,
) -> Result<bool, String> {
    if !is_safe_name(&container) {
        return Err("invalid container name".into());
    }
    let cap_tail = tail.min(50_000).to_string();
    // -f for follow; we own the channel for the lifetime of the stream.
    let Cmd::Docker(args) = docker(&["logs", "-f", "--tail", &cap_tail, "--timestamps", &container]) else {
        unreachable!()
    };
    // A stream can't switch to root partway, so whether it needs root is
    // known first: from this tab's other docker commands, or a quick look.
    let mut as_root = password.is_some() || state.docker_root.lock().await.contains(&session_id);
    if !as_root {
        let probe = format!("docker inspect --type=container {} 2>&1", shq(&container));
        let (code, out) = run_capture(&state, &session_id, &probe, 10).await.unwrap_or((1, String::new()));
        as_root = code != 0 && daemon_refused(&out);
    }
    let (channel, marker) = if as_root {
        let (channel, marker, _) = crate::root_channel(&state, &session_id, &args, password, None).await?;
        state.docker_root.lock().await.insert(session_id.clone());
        (channel, marker)
    } else {
        let session_arc = {
            let connections = state.connections.lock().await;
            connections
                .get(&session_id)
                .map(Arc::clone)
                .ok_or_else(|| "Session not connected".to_string())?
        };
        let channel = {
            let session = session_arc.lock().await;
            session
                .channel_open_session()
                .await
                .map_err(|e| e.to_string())?
        };
        let cmd = format!("{} 2>&1", Cmd::Docker(args).shell());
        channel
            .exec(true, cmd.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        (channel, None)
    };
    let mut stream = channel.into_stream();

    let app_clone = app.clone();
    let stream_id_clone = stream_id.clone();
    let event_name = format!("docker-logs-{}", stream_id_clone);
    let close_event = format!("docker-logs-closed-{}", stream_id_clone);
    let streams_map = streams.streams.clone();
    // A root shell that no longer gives root is forgotten, so the next try
    // looks again (as run_as_root does).
    let root_access = state.root_access.clone();
    let root_session = session_id.clone();

    // tokio::spawn (not tauri::async_runtime::spawn) so we get a native
    // JoinHandle and its `.abort_handle()` — we want to be able to stop
    // the stream from another command later via DockerStreams.
    let handle = tokio::spawn(async move {
        // Coalesce log output: a chatty container's `docker logs -f` can emit a
        // firehose, and one event per 8 KB read flooded the frontend (each event
        // triggered a setLogs + full re-render of the growing <pre>). Accumulate
        // and flush at most every ~30 ms, or sooner past a size cap.
        // `stream.read` is cancel-safe, so racing it against the flush timer in
        // select! never drops data.
        const FLUSH_CAP: usize = 256 * 1024;
        let mut buf = [0u8; 8192];
        let mut out: Vec<u8> = Vec::new();
        let mut flush = tokio::time::interval(std::time::Duration::from_millis(30));
        flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Emit only the complete-UTF-8 PREFIX per streaming flush and KEEP any
        // trailing incomplete multibyte sequence in `out` so the next read can
        // finish it — decoding the whole buffer lossily each flush would turn a
        // character split across a flush boundary (the 30ms tick or the size cap)
        // into replacement chars. A genuinely invalid byte is drained lossily so
        // a malformed stream can't stall the buffer. `final_flush` emits the whole
        // remainder (the stream is ending; a trailing partial will never arrive).
        let flush_logs = |out: &mut Vec<u8>, final_flush: bool| {
            if out.is_empty() { return; }
            if final_flush {
                let s = String::from_utf8_lossy(&out[..]).into_owned();
                let _ = app_clone.emit(&event_name, s);
                out.clear();
                return;
            }
            let take = match std::str::from_utf8(&out[..]) {
                Ok(_) => out.len(),
                Err(e) => match e.error_len() {
                    // Incomplete tail: keep it buffered for the next read.
                    None => e.valid_up_to(),
                    // Invalid bytes never complete — emit through them lossily and
                    // drain so `out` can't stall/grow on a malformed stream.
                    Some(bad) => {
                        let end = e.valid_up_to() + bad;
                        let s = String::from_utf8_lossy(&out[..end]).into_owned();
                        let _ = app_clone.emit(&event_name, s);
                        out.drain(..end);
                        return;
                    }
                },
            };
            if take == 0 { return; } // only a partial char buffered so far
            let s = String::from_utf8_lossy(&out[..take]).into_owned();
            let _ = app_clone.emit(&event_name, s);
            out.drain(..take);
        };
        // Through a root shell, the logs start after its marker line (the
        // shell's own start-up comes first), which says who it is.
        let mut head: Option<Vec<u8>> = marker.map(|_| Vec::new());
        let no_root = |said: &[u8]| {
            let said = String::from_utf8_lossy(said);
            let said: String = said.trim().chars().take(500).collect();
            format!("Couldn't get root for the logs{}{}\n", if said.is_empty() { "." } else { ": " }, said)
        };
        loop {
            tokio::select! {
                r = stream.read(&mut buf) => {
                    match r {
                        Ok(0) | Err(_) if head.is_some() => {
                            let _ = app_clone.emit(&event_name, no_root(head.as_deref().unwrap_or_default()));
                            root_access.lock().await.remove(&root_session);
                            break;
                        }
                        Ok(0) => { flush_logs(&mut out, true); break; }
                        Ok(n) => {
                            if let (Some(h), Some(m)) = (head.as_mut(), marker) {
                                h.extend_from_slice(&buf[..n]);
                                let Some(at) = h.windows(m.len()).position(|w| w == m.as_bytes()) else {
                                    if h.len() > 64 * 1024 { head = None; }
                                    continue;
                                };
                                let Some(nl) = h[at..].iter().position(|&b| b == b'\n') else { continue };
                                if String::from_utf8_lossy(&h[at + m.len()..at + nl]).trim() != "0" {
                                    let _ = app_clone.emit(&event_name, no_root(&[]));
                                    root_access.lock().await.remove(&root_session);
                                    break;
                                }
                                let rest = h.split_off(at + nl + 1);
                                out.extend_from_slice(&rest);
                                head = None;
                            } else {
                                out.extend_from_slice(&buf[..n]);
                            }
                            if out.len() >= FLUSH_CAP { flush_logs(&mut out, false); }
                        }
                        Err(_) => { flush_logs(&mut out, true); break; }
                    }
                }
                _ = flush.tick() => { flush_logs(&mut out, false); }
            }
        }
        let _ = app_clone.emit(&close_event, serde_json::json!({}));
        // Drop our own entry on natural close so the map doesn't grow.
        let mut map = streams_map.lock().await;
        map.remove(&stream_id_clone);
    });

    streams
        .streams
        .lock()
        .await
        .insert(stream_id, handle.abort_handle());
    // Started (the page tells this from a cancelled password prompt, which
    // is null).
    Ok(true)
}

#[tauri::command]
pub async fn ssh_docker_logs_stop(
    streams: tauri::State<'_, DockerStreams>,
    stream_id: String,
) -> Result<(), String> {
    if let Some(h) = streams.streams.lock().await.remove(&stream_id) {
        h.abort();
    }
    Ok(())
}

// ---- terminal exec into container ------------------------------------------

// Opens a PTY-backed exec channel running `docker exec -it <name> <shell>`
// with shell auto-detect: try bash → sh. Wires into the same terminal-IO
// pipeline (output over the page's channel, term_output.rs, + write/resize/close commands)
// the regular shell terminal uses, so xterm.js on the frontend works
// without any branch — just a different exec target.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn open_container_terminal(
    app: tauri::AppHandle,
    state: tauri::State<'_, SshState>,
    session_id: String,
    terminal_id: String,
    container: String,
    cols: u32,
    rows: u32,
    use_sudo: bool,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> Result<(), String> {
    use crate::ssh_manager::TerminalCommand;

    if !is_safe_name(&container) {
        return Err("invalid container name".into());
    }
    // Shell auto-detect runs inside the container: prefer bash, fall back to
    // sh, fall back to ash (Alpine). Anything more exotic and the user can
    // jump in via a regular ssh terminal and figure it out.
    let inner = r#"if command -v bash >/dev/null 2>&1; then exec bash; elif command -v sh >/dev/null 2>&1; then exec sh; elif command -v ash >/dev/null 2>&1; then exec ash; else echo 'no shell found in container' && exit 1; fi"#;
    let Cmd::Docker(args) = docker(&["exec", "-it", &container, "sh", "-c", inner]) else {
        unreachable!()
    };
    // As root as this tab's other docker commands are (run_docker_smart).
    let as_root = use_sudo || state.docker_root.lock().await.contains(&session_id);
    let (channel, early) = if as_root {
        let (channel, _, early) = crate::root_channel(&state, &session_id, &args, None, Some((cols, rows))).await?;
        (channel, early)
    } else {
        let session_arc = {
            let connections = state.connections.lock().await;
            connections
                .get(&session_id)
                .map(Arc::clone)
                .ok_or_else(|| "Session not connected".to_string())?
        };
        let channel = {
            let session = session_arc.lock().await;
            session
                .channel_open_session()
                .await
                .map_err(|e| e.to_string())?
        };
        channel
            .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|e| e.to_string())?;
        channel
            .exec(true, Cmd::Docker(args).shell().as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        (channel, Vec::new())
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<TerminalCommand>(32);
    let (resize_tx, resize_rx) = tokio::sync::watch::channel(crate::ssh_manager::PtySize {
        cols,
        rows,
    });
    state
        .terminal_txs
        .lock()
        .await
        .insert(terminal_id.clone(), tx);
    state
        .resize_txs
        .lock()
        .await
        .insert(terminal_id.clone(), resize_tx);

    // Same pump as the SSH terminal (see open_terminal): coalesced output,
    // paced to the page (term_output.rs), and a split read/write channel
    // whose read side is always drained — required by russh's channel
    // backpressure.
    let output = crate::term_output::OutputSink::open(&terminal_id, on_output);
    // What the container's shell wrote while the password prompt was
    // looked for (root_channel) comes first, through the pump's own filter
    // (term_filter.rs), so a sequence split across the two is cut down too.
    tauri::async_runtime::spawn(crate::ssh_manager::run_pty_pump(
        app.clone(),
        terminal_id.clone(),
        channel,
        rx,
        resize_rx,
        output,
        early,
    ));

    Ok(())
}

#[cfg(test)]
mod cmd_tests {
    use super::{daemon_refused, docker, Cmd};

    /// What a POSIX shell (the server's) makes of a command's words.
    fn argv(shell: &str) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s|' {}", shell.trim_start_matches("'docker' ")))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn each_word_reaches_docker_as_one_argument() {
        let fmt = r#"{{.Names}}|{{.Label "com.docker.compose.project"}}|{{.State}}"#;
        let cmd = docker(&["ps", "-a", "--format", fmt, "it's"]);
        let Cmd::Docker(words) = &cmd else { panic!() };
        assert_eq!(words[0], "docker");
        assert!(cmd.shell().starts_with("'docker' 'ps'"));
        if cfg!(unix) {
            assert_eq!(argv(&cmd.shell()), format!("ps|-a|--format|{}|it's|", fmt));
        }
    }

    #[test]
    fn compose_runs_the_plugin_or_the_old_script() {
        let cmd = Cmd::Compose(vec!["-f".into(), "/srv/my app/compose.yml".into(), "ps".into()]);
        let shell = cmd.shell();
        assert!(shell.ends_with("$CCMD '-f' '/srv/my app/compose.yml' 'ps'"), "{shell}");
    }

    #[test]
    fn refused_by_the_daemon_is_told_from_other_failures() {
        assert!(daemon_refused("permission denied while trying to connect to the docker API at unix:///var/run/docker.sock"));
        assert!(daemon_refused("Got permission denied while trying to connect to the Docker daemon socket"));
        assert!(!daemon_refused("Error response from daemon: No such container: web"));
        // Not root's to fix: the daemon is down, or a container's own error.
        assert!(!daemon_refused("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"));
        assert!(!daemon_refused("OCI runtime create failed: runc create failed: exec: \"/app/run.sh\": permission denied: unknown"));
    }
}

#[cfg(test)]
mod name_tests {
    use super::{is_safe_name, is_safe_path};

    #[test]
    fn name_rejects_leading_dash_flags() {
        // Even shell-quoted, `docker inspect '-foo'` passes `-foo` as one argv
        // token, which docker reads as a flag. A real object name never starts
        // with '-'.
        for bad in ["-f", "--privileged", "-H", "-", "-v/etc:/host"] {
            assert!(!is_safe_name(bad), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn name_accepts_real_object_names() {
        for ok in ["nginx", "my_app-1", "ghcr.io/org/app:1.2", "redis:7", "web@sha256", "proj_default"] {
            assert!(is_safe_name(ok), "{ok:?} must be accepted");
        }
    }

    #[test]
    fn name_rejects_empty_oversize_and_metachars() {
        assert!(!is_safe_name(""));
        assert!(!is_safe_name(&"a".repeat(513)));
        assert!(is_safe_name(&"a".repeat(512)));
        for bad in ["a;b", "a b", "a|b", "a$(id)", "a`b`"] {
            assert!(!is_safe_name(bad), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn path_rejects_leading_dash_flags() {
        // A compose path is a bare argument to `head`; a leading '-' would be a
        // flag. Legit paths can be written `./-name`.
        for bad in ["-rf", "--help", "-", "-c1"] {
            assert!(!is_safe_path(bad), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn path_accepts_real_paths() {
        for ok in ["/srv/app/docker-compose.yml", "./compose.yaml", "~/stack/compose.yml",
                   "/home/op/my project/compose.yml"] {
            assert!(is_safe_path(ok), "{ok:?} must be accepted");
        }
    }

    #[test]
    fn path_still_rejects_shell_metachars() {
        for bad in ["a;b", "a|b", "a`b`", "a$(id)", "a>b", "a\\b"] {
            assert!(!is_safe_path(bad), "{bad:?} must be rejected");
        }
    }
}
