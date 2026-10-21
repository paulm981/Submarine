//! Terminals on this computer, for the Home view: a shell in a pseudo-terminal
//! (ConPTY on Windows, openpty elsewhere), started in the user's home folder.
//! Output reaches the page the way an SSH terminal's does — coalesced, over
//! the page's channel and paced by what it has drawn (term_output.rs), then
//! `terminal-closed-{id}` — so the same TerminalView draws both.
//!
//! The page only ever names a shell by id from [`shells`]: what each id runs
//! is decided here. The one free choice, a program of the user's own, is made
//! in a native dialog (as Live Edit's editor is), never passed in by the page.
//!
//! The AI tools heading (#32) lists the AI tools Home can start, connected to
//! Submarine, while AI tools are on in Settings (ai/mod.rs): an AI tab is a
//! Home terminal whose program is the AI tool, started with its channel's
//! address and its own token in its environment.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// Kept with Live Edit's editor (local_open::read_setting).
const CONFIG_FILE: &str = "home_terminal.json";

#[derive(Serialize, Deserialize, Default)]
struct Config {
    /// The shell new Home terminals start, by id.
    #[serde(default)]
    default: Option<String>,
    /// The program picked with "Choose program…".
    #[serde(default)]
    custom: Option<String>,
}

fn read_config(app: &tauri::AppHandle) -> Config {
    crate::local_open::read_setting(app, CONFIG_FILE).unwrap_or_default()
}

fn write_config(app: &tauri::AppHandle, config: &Config) -> Result<(), String> {
    crate::local_open::write_setting(app, CONFIG_FILE, config, "the Home terminal")
}

/// Variables the app sets for its own webview when it starts (lib.rs `run`,
/// webkit_sandbox.rs). They are for WebKit, not for whatever the user runs
/// from Home — software GL and an X11-only GTK would follow every program
/// started there — so a shell starts without them.
static WEBVIEW_ENV: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// Set a variable for the webview, and keep it from local shells.
pub(crate) fn set_webview_env(name: &'static str, value: &str) {
    std::env::set_var(name, value);
    if let Ok(mut names) = WEBVIEW_ENV.lock() {
        names.push(name);
    }
}

// The headings shells are listed under, the same on every system: what the
// computer itself runs, Linux under WSL (Windows only), the program the user
// picked.
#[cfg(windows)]
const GROUP_SYSTEM: &str = "Windows";
#[cfg(not(windows))]
const GROUP_SYSTEM: &str = "This computer";
#[cfg(windows)]
const GROUP_WSL: &str = "WSL";
const GROUP_CUSTOM: &str = "Your choice";
const GROUP_AI: &str = "AI tools";

/// A shell the Home view can start.
#[derive(Serialize, Clone, Debug)]
pub struct Shell {
    pub id: String,
    pub label: String,
    /// The heading it's listed under (GROUP_*).
    pub group: &'static str,
    /// What tells it apart: "default" for WSL's default distribution, "your
    /// login shell", or where the program is.
    pub detail: Option<String>,
    #[serde(skip)]
    program: PathBuf,
    #[serde(skip)]
    args: Vec<String>,
    /// Added to its environment: an AI tab's address and token.
    #[serde(skip)]
    env: Vec<(String, String)>,
}

fn shell(id: impl Into<String>, label: impl Into<String>, group: &'static str, program: PathBuf, args: &[&str]) -> Shell {
    Shell { id: id.into(), label: label.into(), group, detail: None, program, args: args.iter().map(|a| a.to_string()).collect(), env: Vec::new() }
}

impl Shell {
    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Something to say under a heading that has nothing to list — that WSL
/// isn't installed, or has no Linux yet — so an empty list isn't a puzzle.
#[derive(Serialize, Clone, Debug)]
pub struct Note {
    pub group: &'static str,
    pub text: String,
}

/// The program picked with "Choose program…", while it's still there. On
/// Windows that can be an app alias from the Store (wt.exe, pwsh.exe), which
/// only symlink_metadata sees.
fn custom_program(config: &Config) -> Option<PathBuf> {
    let p = PathBuf::from(config.custom.as_deref()?);
    let there = p.is_file() || (cfg!(windows) && std::fs::symlink_metadata(&p).is_ok_and(|m| !m.is_dir()));
    there.then_some(p)
}

/// The shells on this computer, the usual one first, and what to say about
/// a heading with none. Programs go by full path: a bare name would be
/// looked for next to submarine.exe first. `list_wsl`: also name each WSL
/// distribution, which asks wsl.exe.
fn shells(config: &Config, list_wsl: bool) -> (Vec<Shell>, Vec<Note>) {
    let (mut out, notes) = system_shells(list_wsl);
    if let Some(custom) = custom_program(config) {
        let name = custom.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let shown = custom.display().to_string();
        out.push(shell("custom", name, GROUP_CUSTOM, custom, &[]).detail(shown));
    }
    (out, notes)
}

/// The shell an id names, without listing WSL's distributions: a `wsl:` id
/// carries its distribution's name.
fn find_shell(config: &Config, id: &str) -> Option<Shell> {
    #[cfg(windows)]
    if let Some(name) = id.strip_prefix("wsl:") {
        let wsl = crate::local_open::system32().join("wsl.exe");
        return (wsl.is_file() && valid_distro(name)).then(|| wsl_shell(&wsl, Some(name)));
    }
    shells(config, false).0.into_iter().find(|s| s.id == id)
}

/// `wsl`: WSL's default distribution, whichever it is when it starts (the id
/// earlier builds listed); `wsl:<name>`: that distribution.
#[cfg(windows)]
fn wsl_shell(wsl: &std::path::Path, distro: Option<&str>) -> Shell {
    match distro {
        Some(d) => shell(format!("wsl:{}", d), d, GROUP_WSL, wsl.to_path_buf(), &["-d", d, "--cd", "~"]),
        None => shell("wsl", "WSL (default)", GROUP_WSL, wsl.to_path_buf(), &["--cd", "~"]),
    }
}

/// Each distribution by name, WSL's default first and marked so — the one a
/// bare `wsl` starts.
#[cfg(windows)]
fn wsl_entries(wsl: &std::path::Path, distros: &[String], default: Option<&str>) -> Vec<Shell> {
    order_default_first(distros, default)
        .into_iter()
        .map(|(d, is_default)| {
            let s = wsl_shell(wsl, Some(d));
            if is_default { s.detail("default") } else { s }
        })
        .collect()
}

/// The names with `default` (matched as WSL does, ignoring case) first,
/// each with whether it is the default.
#[cfg(any(windows, test))]
fn order_default_first<'a>(names: &'a [String], default: Option<&str>) -> Vec<(&'a str, bool)> {
    let is_default = |n: &str| default.is_some_and(|d| d.eq_ignore_ascii_case(n));
    let mut out: Vec<(&str, bool)> = names.iter().map(|n| (n.as_str(), is_default(n))).collect();
    out.sort_by_key(|(_, d)| !*d);
    out
}

/// WSL's default distribution, as it records it for this user: the GUID in
/// `Lxss\DefaultDistribution`, and that key's `DistributionName`. None when
/// there's none, or it can't be read.
#[cfg(windows)]
fn wsl_default_distro() -> Option<String> {
    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    let guid = user_registry_string(LXSS, "DefaultDistribution")?;
    if !guid.starts_with('{') || guid.len() > 40 {
        return None;
    }
    user_registry_string(&format!(r"{LXSS}\{guid}"), "DistributionName")
}

/// A string value under HKEY_CURRENT_USER.
#[cfg(windows)]
fn user_registry_string(key: &str, value: &str) -> Option<String> {
    use std::ffi::c_void;
    // (HKEY)(ULONG_PTR)(LONG)0x80000001, sign-extended as Windows does.
    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            bytes: *mut u32,
        ) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (key, value) = (wide(key), wide(value));
    let mut buf = vec![0u16; 260];
    let mut bytes = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut bytes,
        )
    };
    if rc != 0 {
        return None;
    }
    let units = (bytes as usize / 2).min(buf.len());
    let len = buf[..units].iter().position(|&c| c == 0).unwrap_or(units);
    let s = String::from_utf16_lossy(&buf[..len]);
    (!s.is_empty()).then_some(s)
}

#[cfg(windows)]
fn system_shells(list_wsl: bool) -> (Vec<Shell>, Vec<Note>) {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let sys = crate::local_open::system32();
    let mut out = Vec::new();
    let mut notes = Vec::new();
    // PowerShell 7 when it's installed: from its installer, or from the
    // Store, whose copy is an app alias only symlink_metadata can see.
    let pwsh = env("ProgramFiles")
        .map(|d| d.join(r"PowerShell\7\pwsh.exe"))
        .filter(|p| p.is_file())
        .or_else(|| {
            env("LOCALAPPDATA")
                .map(|d| d.join(r"Microsoft\WindowsApps\pwsh.exe"))
                .filter(|p| std::fs::symlink_metadata(p).is_ok())
        });
    if let Some(p) = pwsh {
        out.push(shell("pwsh", "PowerShell 7", GROUP_SYSTEM, p, &["-NoLogo"]));
    }
    out.push(shell("powershell", "Windows PowerShell", GROUP_SYSTEM, sys.join(r"WindowsPowerShell\v1.0\powershell.exe"), &["-NoLogo"]));
    out.push(shell("cmd", "Command Prompt", GROUP_SYSTEM, sys.join("cmd.exe"), &[]));
    // Git for Windows' bash itself, not git-bash.exe, which opens a window
    // of its own.
    let git_bash = [env("ProgramFiles"), env("ProgramFiles(x86)"), env("LOCALAPPDATA").map(|d| d.join("Programs"))]
        .into_iter()
        .flatten()
        .map(|d| d.join(r"Git\bin\bash.exe"))
        .find(|p| p.is_file());
    if let Some(p) = git_bash {
        out.push(shell("gitbash", "Git Bash", GROUP_SYSTEM, p, &["--login", "-i"]));
    }
    let wsl = sys.join("wsl.exe");
    let note = |text: &str| Note { group: GROUP_WSL, text: text.to_string() };
    if !wsl.is_file() {
        if list_wsl {
            notes.push(note("WSL isn't installed. To add Linux: run wsl --install in PowerShell as administrator, then restart."));
        }
    } else if !list_wsl {
        // Starting one by id needs no list: `wsl` is the default distribution.
        out.push(wsl_shell(&wsl, None));
    } else {
        match wsl_distros(&wsl) {
            Some(distros) if distros.is_empty() => notes.push(note(
                "WSL has no Linux distribution yet. To add one: run wsl --install -d Ubuntu in PowerShell.",
            )),
            Some(distros) => out.extend(wsl_entries(&wsl, &distros, wsl_default_distro().as_deref())),
            None => {
                // It may still be starting: offer its default all the same.
                out.push(wsl_shell(&wsl, None));
                notes.push(note("WSL didn't answer in time — it may still be starting. Open this list again to see each distribution."));
            }
        }
    }
    (out, notes)
}

/// A WSL distribution's name as it may go after `wsl.exe -d`: plain, and
/// starting with a letter or digit so it can't read as an option.
#[cfg(any(windows, test))]
fn valid_distro(name: &str) -> bool {
    name.len() <= 64
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// The WSL distributions installed, from `wsl.exe -l -q`, leaving out Docker
/// Desktop's, which aren't shells. None when it gives no answer within a few
/// seconds — wsl.exe can take that long while WSL itself is still starting.
/// It fails, rather than lists nothing, when there's no distribution (or WSL
/// isn't set up): that counts as an empty list.
#[cfg(windows)]
fn wsl_distros(wsl: &std::path::Path) -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(mut child) = std::process::Command::new(wsl)
        .args(["-l", "-q"])
        // UTF-16 is what it writes unless this asks for UTF-8.
        .env_remove("WSL_UTF8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    else {
        return None;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Some(Vec::new()),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let mut out = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_end(&mut stdout, &mut out);
    }
    Some(parse_wsl_list(&out))
}

/// `wsl -l -q` writes UTF-16 LE, one name per line — or UTF-8 when WSL_UTF8
/// was set where it ran, which shows as text with no NUL bytes.
#[cfg(any(windows, test))]
fn parse_wsl_list(raw: &[u8]) -> Vec<String> {
    let text = if raw.contains(&0) {
        String::from_utf16_lossy(&crate::local_open::utf16le_units(raw))
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    text.lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0').to_string())
        .filter(|n| valid_distro(n) && !n.starts_with("docker-desktop"))
        .collect()
}

#[cfg(not(windows))]
fn system_shells(_list_wsl: bool) -> (Vec<Shell>, Vec<Note>) {
    let login = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = vec![shell("login", name(&login), GROUP_SYSTEM, login.clone(), &["-l"]).detail("your login shell")];
    // The others /etc/shells lists, once each (it often names a shell
    // under both /bin and /usr/bin).
    let mut seen = vec![std::fs::canonicalize(&login).unwrap_or(login)];
    let listed = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    for p in etc_shells(&listed) {
        let real = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if !p.is_file() || seen.contains(&real) {
            continue;
        }
        seen.push(real);
        // Its path, which tells apart two of the same name (/bin/bash and
        // Homebrew's, say).
        let shown = p.display().to_string();
        out.push(shell(format!("shell:{}", p.display()), name(&p), GROUP_SYSTEM, p, &["-l"]).detail(shown));
    }
    (out, Vec::new())
}

/// The shells in /etc/shells, without the ones that only refuse a login.
#[cfg(any(not(windows), test))]
fn etc_shells(listed: &str) -> Vec<PathBuf> {
    listed
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/'))
        .filter(|l| !l.ends_with("/nologin") && !l.ends_with("/false") && !l.ends_with("/git-shell"))
        .map(PathBuf::from)
        .collect()
}

#[derive(Serialize, Default)]
pub struct LocalShells {
    shells: Vec<Shell>,
    /// What to say under a heading with nothing to list (Note).
    notes: Vec<Note>,
    /// The id new Home terminals start: the saved choice while it's among
    /// `shells`, else the first.
    default_id: String,
    /// The program picked with "Choose program…", if any, and whether it's
    /// gone (moved or uninstalled), so Settings can say so.
    custom: Option<String>,
    custom_missing: bool,
    /// The AI tools heading: what's installed, while AI tools are on. Never
    /// a default shell.
    ai_tools: Vec<Shell>,
}

fn ai_tools(app: &tauri::AppHandle) -> Vec<Shell> {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::ai::AiState>() else { return Vec::new() };
    crate::ai::home_tools(&state)
        .into_iter()
        .map(|(id, label, path)| {
            let shown = path.display().to_string();
            shell(id, label, GROUP_AI, path, &[]).detail(shown)
        })
        .collect()
}

fn local_shells_info(app: &tauri::AppHandle) -> LocalShells {
    let config = read_config(app);
    let (shells, notes) = shells(&config, true);
    // `wsl` (WSL's default, which earlier builds listed) is listed now under
    // its distribution's name.
    let saved = config.default.clone().map(|d| {
        if d != "wsl" {
            return d;
        }
        shells.iter().find(|s| s.id.starts_with("wsl:") && s.detail.as_deref() == Some("default")).map_or(d, |s| s.id.clone())
    });
    let default_id = saved
        .filter(|d| shells.iter().any(|s| &s.id == d))
        .or_else(|| shells.first().map(|s| s.id.clone()))
        .unwrap_or_default();
    let custom_missing = config.custom.is_some() && custom_program(&config).is_none();
    LocalShells { shells, notes, default_id, custom: config.custom, custom_missing, ai_tools: ai_tools(app) }
}

/// local_shells_info off the async runtime: listing WSL waits on wsl.exe.
async fn shells_info(app: &tauri::AppHandle) -> LocalShells {
    let app = app.clone();
    tokio::task::spawn_blocking(move || local_shells_info(&app)).await.unwrap_or_default()
}

/// The saved choice while that shell is still here, else the first.
fn default_shell(config: &Config) -> Option<Shell> {
    config
        .default
        .as_deref()
        .and_then(|d| find_shell(config, d))
        .or_else(|| shells(config, false).0.into_iter().next())
}

#[tauri::command]
pub async fn list_local_shells(app: tauri::AppHandle) -> LocalShells {
    shells_info(&app).await
}

/// The shell new Home terminals start: one of `list_local_shells`.
#[tauri::command]
pub async fn local_shell_set_default(app: tauri::AppHandle, id: String) -> Result<LocalShells, String> {
    let mut config = read_config(&app);
    if find_shell(&config, &id).is_none() {
        return Err("That shell isn't on this computer.".into());
    }
    config.default = Some(id);
    write_config(&app, &config)?;
    Ok(shells_info(&app).await)
}

/// Pick a program of your own in a native dialog; it becomes the default.
#[tauri::command]
pub async fn local_shell_pick_custom(app: tauri::AppHandle) -> Result<LocalShells, String> {
    #[cfg(target_os = "android")]
    {
        let _ = &app;
        Err("Local terminals aren't available on Android.".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut dialog = rfd::FileDialog::new().set_title("Choose the program for Home terminals");
        if cfg!(windows) {
            dialog = dialog.add_filter("Programs", &["exe"]);
        }
        let Some(path) = dialog.pick_file() else { return Ok(shells_info(&app).await) };
        let mut config = read_config(&app);
        config.custom = Some(path.to_string_lossy().into_owned());
        config.default = Some("custom".into());
        write_config(&app, &config)?;
        Ok(shells_info(&app).await)
    }
}

/// Forget the program picked with "Choose program…".
#[tauri::command]
pub async fn local_shell_clear_custom(app: tauri::AppHandle) -> Result<LocalShells, String> {
    let mut config = read_config(&app);
    config.custom = None;
    if config.default.as_deref() == Some("custom") {
        config.default = None;
    }
    write_config(&app, &config)?;
    Ok(shells_info(&app).await)
}

/// Ids come from the page: `local-` and a short plain name, so one can't be
/// mistaken for an SSH terminal's or carry anything odd into an event name.
fn valid_id(id: &str) -> bool {
    id.starts_with("local-") && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// portable-pty prefers a `conpty.dll` found by Windows' default search over
/// the one in the system, and that search starts with the folder
/// submarine.exe is in — for a portable copy, often Downloads. A conpty.dll
/// there or in the current folder would be loaded into the app, so local
/// terminals refuse to start while one is.
#[cfg(windows)]
fn planted_conpty() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    [exe_dir, std::env::current_dir().ok()]
        .into_iter()
        .flatten()
        .map(|d| d.join("conpty.dll"))
        .find(|p| p.exists())
}

#[cfg(not(target_os = "android"))]
mod pty {
    use super::*;
    use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
    use std::collections::{HashMap, HashSet};
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};
    use tauri::{Emitter, Manager};

    struct Term {
        master: Box<dyn MasterPty + Send>,
        /// Keystrokes for the writer thread: a shell that stops reading
        /// blocks that thread, never the command that queued them.
        input: mpsc::Sender<Vec<u8>>,
        killer: Box<dyn ChildKiller + Send + Sync>,
    }

    #[derive(Default)]
    struct Terms {
        open: HashMap<String, Term>,
        /// Ids whose shell is still starting, and those of them the page
        /// closed meanwhile: a close can arrive before the open it follows
        /// has finished, and that shell must not be left running unseen.
        starting: HashSet<String>,
        closed_while_starting: HashSet<String>,
    }

    /// The open local terminals, by terminal id.
    #[derive(Default, Clone)]
    pub struct LocalTerms(Arc<Mutex<Terms>>);

    impl LocalTerms {
        fn take(&self, id: &str) -> Option<Term> {
            self.0.lock().ok()?.open.remove(id)
        }
    }

    /// Output is coalesced as an SSH terminal's is (ssh_manager::pty_pump):
    /// a batch goes out 8ms after its first byte, or sooner once it passes
    /// the cap, so a firehose doesn't flood the page with events.
    const FLUSH_CAP: usize = 256 * 1024;
    const FLUSH_WINDOW: Duration = Duration::from_millis(8);

    /// Open a terminal for the page: output to `on_output`, paced by what
    /// the page has drawn (term_output.rs), then a line saying the shell has
    /// ended and `terminal-closed-{id}`.
    pub fn open(
        app: &tauri::AppHandle,
        terms: &LocalTerms,
        id: &str,
        shell: &Shell,
        cols: u16,
        rows: u16,
        on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
    ) -> Result<(), String> {
        let output = std::sync::Arc::new(crate::term_output::OutputSink::open(id, on_output));
        let end_output = output.clone();
        let (end_app, end_id) = (app.clone(), id.to_string());
        let ai_app = app.clone();
        spawn(
            terms,
            id,
            shell,
            app.path().home_dir().ok(),
            (cols, rows),
            move |batch| output.push(batch),
            move || {
                // An AI tab's access ends with its tool.
                if let Some(ai) = ai_app.try_state::<crate::ai::AiState>() {
                    ai.end_tab(&end_id);
                }
                end_output.push(
                    &mut b"\r\n\x1b[2m[The shell has ended. Close this tab, or open a new one with +.]\x1b[0m\r\n".to_vec(),
                );
                end_output.finish(move || {
                    let _ = end_app.emit(&format!("terminal-closed-{}", end_id), serde_json::json!({}));
                });
            },
        )
    }

    /// Start `shell` in `cwd` in a new pseudo-terminal and run it: each
    /// output batch goes to `flush`, and `ended` once, when the shell has
    /// exited (or its output is over). Apart from `open` so the tests run
    /// this, the real path.
    pub(super) fn spawn(
        terms: &LocalTerms,
        id: &str,
        shell: &Shell,
        cwd: Option<PathBuf>,
        (cols, rows): (u16, u16),
        flush: impl FnMut(&mut Vec<u8>) + Send + 'static,
        ended: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        if !valid_id(id) {
            return Err("[LOCAL_TERM] BAD_ID".into());
        }
        #[cfg(windows)]
        if let Some(dll) = planted_conpty() {
            return Err(format!(
                "[LOCAL_TERM] CONPTY_REFUSED: a conpty.dll is in {} — Submarine won't load it. Remove it to use Home terminals.",
                dll.parent().map(|d| d.display().to_string()).unwrap_or_default()
            ));
        }
        {
            let mut t = terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
            if t.open.contains_key(id) || !t.starting.insert(id.to_string()) {
                return Err("[LOCAL_TERM] ALREADY_OPEN".into());
            }
        }
        let started = start(shell, cwd, cols, rows);
        let mut t = terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        t.starting.remove(id);
        let (term, mut child, reader, writer) = started?;
        if t.closed_while_starting.remove(id) {
            drop(t);
            let mut term = term.0;
            let _ = term.killer.kill();
            std::thread::spawn(move || child.wait());
            return Ok(());
        }
        t.open.insert(id.to_string(), term.0);
        drop(t);

        // Whichever comes first — the shell exiting, or its output ending —
        // says it has ended; the other doesn't say it again.
        let ended = Arc::new(Mutex::new(Some(ended)));
        let end_once = {
            let ended = ended.clone();
            move || {
                if let Some(f) = ended.lock().ok().and_then(|mut f| f.take()) {
                    f();
                }
            }
        };
        let end_on_exit = end_once.clone();

        // A reader thread hands chunks to one that coalesces them; a writer
        // thread takes keystrokes.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let output_over = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || pump_reader(reader, tx));
        let over = output_over.clone();
        std::thread::spawn(move || {
            send_batches(rx, flush);
            over.store(true, Ordering::SeqCst);
            end_once();
        });
        std::thread::spawn(move || pump_writer(writer, term.1));

        // When the shell exits, let go of the terminal: on Windows the
        // output pipe only ends once the pseudo-console is closed, and on
        // Unix a job left in the background can keep it open. The pause
        // first lets the reader take what the shell wrote last.
        let (terms2, id2) = (terms.clone(), id.to_string());
        std::thread::spawn(move || {
            let _ = child.wait();
            std::thread::sleep(Duration::from_millis(200));
            drop(terms2.take(&id2));
            if !output_over.load(Ordering::SeqCst) {
                end_on_exit();
            }
        });
        Ok(())
    }

    type Started = ((Term, mpsc::Receiver<Vec<u8>>), Box<dyn portable_pty::Child + Send + Sync>, Box<dyn Read + Send>, Box<dyn Write + Send>);

    /// The pseudo-terminal and the shell in it.
    fn start(shell: &Shell, cwd: Option<PathBuf>, cols: u16, rows: u16) -> Result<Started, String> {
        let size = PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 };
        let pair = native_pty_system().openpty(size).map_err(|e| format!("[LOCAL_TERM] PTY_FAILED: {}", e))?;
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }
        for name in WEBVIEW_ENV.lock().map(|n| n.clone()).unwrap_or_default() {
            cmd.env_remove(name);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &shell.env {
            cmd.env(k, v);
        }
        // An app started from Finder gets no locale, and zsh then shows any
        // non-ASCII text as escapes.
        #[cfg(target_os = "macos")]
        if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("[LOCAL_TERM] START_FAILED: {}: {}", shell.label, e))?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| format!("[LOCAL_TERM] PTY_FAILED: {}", e))?;
        let writer = pair.master.take_writer().map_err(|e| format!("[LOCAL_TERM] PTY_FAILED: {}", e))?;
        let killer = child.clone_killer();
        let (input, keys) = mpsc::channel();
        Ok(((Term { master: pair.master, input, killer }, keys), child, reader, writer))
    }

    fn pump_reader(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }

    /// Ends when the terminal is let go of (its sender dropped) or the shell
    /// stops taking input.
    fn pump_writer(mut writer: Box<dyn Write + Send>, keys: mpsc::Receiver<Vec<u8>>) {
        while let Ok(data) = keys.recv() {
            if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    }

    fn send_batches(rx: mpsc::Receiver<Vec<u8>>, mut flush: impl FnMut(&mut Vec<u8>)) {
        let mut batch = Vec::new();
        while let Ok(first) = rx.recv() {
            batch.extend_from_slice(&first);
            let deadline = Instant::now() + FLUSH_WINDOW;
            while batch.len() < FLUSH_CAP {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                match rx.recv_timeout(left) {
                    Ok(more) => batch.extend_from_slice(&more),
                    Err(_) => break,
                }
            }
            flush(&mut batch);
            batch.clear();
        }
    }

    /// Queue keystrokes for the shell; never waits for it.
    pub fn write(terms: &LocalTerms, id: &str, data: &[u8]) -> Result<(), String> {
        let t = terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let term = t.open.get(id).ok_or("[LOCAL_TERM] NOT_OPEN")?;
        term.input.send(data.to_vec()).map_err(|_| "[LOCAL_TERM] NOT_OPEN".to_string())
    }

    pub fn resize(terms: &LocalTerms, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let t = terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let term = t.open.get(id).ok_or("[LOCAL_TERM] NOT_OPEN")?;
        term.master
            .resize(PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("[LOCAL_TERM] RESIZE_FAILED: {}", e))
    }

    pub fn close(terms: &LocalTerms, id: &str) {
        let term = {
            let Ok(mut t) = terms.0.lock() else { return };
            if t.starting.contains(id) {
                t.closed_while_starting.insert(id.to_string());
            }
            t.open.remove(id)
        };
        // Ended and dropped without the lock held: closing a pseudo-console
        // waits for its output to drain.
        if let Some(mut term) = term {
            let _ = term.killer.kill();
            drop(term);
        }
    }
}

#[cfg(not(target_os = "android"))]
pub use pty::LocalTerms;

#[cfg(target_os = "android")]
#[derive(Default)]
pub struct LocalTerms;

/// Start a shell (one of `list_local_shells`, or the default) in a new local
/// terminal. Returns the shell's label for the tab. Off the async runtime:
/// starting a process, and on Windows a pseudo-console, can take a moment.
#[tauri::command]
pub async fn local_terminal_open(
    app: tauri::AppHandle,
    terms: tauri::State<'_, LocalTerms>,
    terminal_id: String,
    shell: Option<String>,
    cols: u16,
    rows: u16,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &terms, terminal_id, shell, cols, rows, on_output);
        Err("Local terminals aren't available on Android.".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let terms = terms.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Some(tool) = shell.as_deref().filter(|id| id.starts_with("ai:")) {
                return open_ai_tool(&app, &terms, &terminal_id, tool, cols, rows, on_output);
            }
            let config = read_config(&app);
            let chosen = match shell {
                Some(id) => find_shell(&config, &id),
                None => default_shell(&config),
            }
            .ok_or("That shell isn't on this computer.")?;
            pty::open(&app, &terms, &terminal_id, &chosen, cols, rows, on_output)?;
            Ok(chosen.label)
        })
        .await
        .map_err(|e| format!("[LOCAL_TERM] OPEN_FAILED: {}", e))?
    }
}

/// An AI tool from the AI tools heading, connected: its label in the AI
/// panel is the tab's.
#[cfg(not(target_os = "android"))]
fn open_ai_tool(
    app: &tauri::AppHandle,
    terms: &LocalTerms,
    terminal_id: &str,
    tool: &str,
    cols: u16,
    rows: u16,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> Result<String, String> {
    use tauri::Manager;
    if !valid_id(terminal_id) {
        return Err("[LOCAL_TERM] BAD_ID".into());
    }
    let ai = app.state::<crate::ai::AiState>();
    let launch = crate::ai::home_launch(app, &ai, tool, terminal_id)?;
    let mut s = shell(tool, launch.label.clone(), GROUP_AI, launch.program, &[]);
    s.args = launch.args;
    s.env = launch.env;
    if let Err(e) = pty::open(app, terms, terminal_id, &s, cols, rows, on_output) {
        ai.end_tab(terminal_id);
        return Err(e);
    }
    Ok(launch.label)
}

#[tauri::command]
pub async fn local_terminal_write(terms: tauri::State<'_, LocalTerms>, terminal_id: String, data: Vec<u8>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (&terms, terminal_id, data);
        Err("Local terminals aren't available on Android.".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        pty::write(&terms, &terminal_id, &data)
    }
}

#[tauri::command]
pub async fn local_terminal_resize(terms: tauri::State<'_, LocalTerms>, terminal_id: String, cols: u16, rows: u16) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (&terms, terminal_id, cols, rows);
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        pty::resize(&terms, &terminal_id, cols, rows)
    }
}

#[tauri::command]
pub async fn local_terminal_close(
    terms: tauri::State<'_, LocalTerms>,
    ai: tauri::State<'_, crate::ai::AiState>,
    terminal_id: String,
) -> Result<(), String> {
    // Closing an AI tab ends its access at once, before its tool has gone.
    ai.end_tab(&terminal_id);
    #[cfg(target_os = "android")]
    {
        let _ = (&terms, terminal_id);
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        let terms = terms.inner().clone();
        let _ = tauri::async_runtime::spawn_blocking(move || pty::close(&terms, &terminal_id)).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_list_is_utf16_and_skips_docker() {
        let mut raw = vec![0xFF, 0xFE];
        for u in "Ubuntu\r\nDebian\r\ndocker-desktop\r\ndocker-desktop-data\r\n\r\nbad name;rm\r\n--exec\r\n".encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(parse_wsl_list(&raw), vec!["Ubuntu", "Debian"]);
        // With WSL_UTF8=1 where it ran.
        assert_eq!(parse_wsl_list(b"Ubuntu-22.04\nkali-linux\n"), vec!["Ubuntu-22.04", "kali-linux"]);
    }

    #[test]
    fn wsl_lists_its_default_first() {
        let names: Vec<String> = ["Ubuntu", "Debian", "kali-linux"].iter().map(|s| s.to_string()).collect();
        assert_eq!(order_default_first(&names, Some("debian")), vec![("Debian", true), ("Ubuntu", false), ("kali-linux", false)]);
        // Not known: as WSL listed them, none marked.
        assert_eq!(order_default_first(&names, None), vec![("Ubuntu", false), ("Debian", false), ("kali-linux", false)]);
    }

    #[test]
    fn etc_shells_leaves_out_the_ones_that_refuse_logins() {
        let listed = "# /etc/shells: valid login shells\n/bin/sh\n/usr/bin/bash\n/usr/sbin/nologin\n/bin/false\n/usr/bin/git-shell\n  /usr/bin/zsh  \nnot-a-path\n";
        assert_eq!(etc_shells(listed), vec![PathBuf::from("/bin/sh"), PathBuf::from("/usr/bin/bash"), PathBuf::from("/usr/bin/zsh")]);
    }

    #[test]
    fn ids_are_local_and_plain() {
        assert!(valid_id("local-1712345678-ab12"));
        assert!(!valid_id("term-1"));
        assert!(!valid_id("local-a b"));
        assert!(!valid_id(&format!("local-{}", "x".repeat(80))));
    }

    #[test]
    fn a_custom_program_only_counts_while_it_exists() {
        let config = Config { default: None, custom: Some("/no/such/program".into()) };
        assert!(!shells(&config, false).0.iter().any(|s| s.id == "custom"));
        let here = std::env::current_exe().unwrap();
        let config = Config { default: None, custom: Some(here.to_string_lossy().into_owned()) };
        assert!(shells(&config, false).0.iter().any(|s| s.id == "custom"));
        assert_eq!(find_shell(&config, "custom").map(|s| s.program), Some(here));
    }

    #[cfg(unix)]
    fn run_shell(terms: &pty::LocalTerms, id: &str) -> (std::sync::mpsc::Receiver<Vec<u8>>, std::sync::mpsc::Receiver<()>) {
        let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (end_tx, end_rx) = std::sync::mpsc::channel::<()>();
        let sh = shell("t", "sh", GROUP_SYSTEM, PathBuf::from("/bin/sh"), &[]);
        pty::spawn(
            terms,
            id,
            &sh,
            Some(std::env::temp_dir()),
            (80, 24),
            move |batch| { let _ = out_tx.send(batch.clone()); },
            move || { let _ = end_tx.send(()); },
        )
        .unwrap();
        (out_rx, end_rx)
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_starts_where_asked_runs_what_it_is_sent_and_ends() {
        use std::time::Duration;
        set_webview_env("SUBMARINE_TEST_WEBVIEW_ONLY", "1");
        let terms = pty::LocalTerms::default();
        let (out_rx, end_rx) = run_shell(&terms, "local-test");
        let sh = shell("t", "sh", GROUP_SYSTEM, PathBuf::from("/bin/sh"), &[]);
        assert!(pty::spawn(&terms, "local-test", &sh, None, (80, 24), |_| {}, || {}).is_err(), "one shell per id");
        pty::resize(&terms, "local-test", 100, 30).unwrap();
        pty::write(&terms, "local-test", b"echo sub$((40+2))marine $TERM [${SUBMARINE_TEST_WEBVIEW_ONLY:-unset}]; pwd; exit\n").unwrap();
        end_rx.recv_timeout(Duration::from_secs(10)).expect("the shell ended");
        assert!(end_rx.recv_timeout(Duration::from_millis(600)).is_err(), "said once");
        let seen: Vec<u8> = out_rx.try_iter().flatten().collect();
        let seen = String::from_utf8_lossy(&seen);
        assert!(seen.contains("sub42marine xterm-256color [unset]"), "{seen}");
        let dir = std::env::temp_dir();
        let real = std::fs::canonicalize(&dir).unwrap();
        assert!(seen.contains(&*real.to_string_lossy()) || seen.contains(&*dir.to_string_lossy()), "{seen}");
        // The exit watcher lets go of the terminal shortly after the shell
        // is gone.
        let t = std::time::Instant::now();
        while pty::write(&terms, "local-test", b"").is_ok() && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(pty::write(&terms, "local-test", b"x").is_err(), "closed");
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_that_wont_read_doesnt_hold_up_writes() {
        use std::time::{Duration, Instant};
        let terms = pty::LocalTerms::default();
        let (_out, _end) = run_shell(&terms, "local-busy");
        // `sleep` reads nothing: far more than a pseudo-terminal buffers.
        pty::write(&terms, "local-busy", b"sleep 30\n").unwrap();
        let t = Instant::now();
        for _ in 0..64 {
            pty::write(&terms, "local-busy", &[b'x'; 4096]).unwrap();
        }
        assert!(t.elapsed() < Duration::from_secs(2), "writes waited on the shell");
        pty::close(&terms, "local-busy");
        assert!(pty::write(&terms, "local-busy", b"x").is_err(), "closed");
    }
}
