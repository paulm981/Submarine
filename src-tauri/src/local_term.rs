//! Terminals on this computer, for the Home view: a shell in a pseudo-terminal
//! (ConPTY on Windows, openpty elsewhere), started in the user's home folder.
//! Output reaches the page the way an SSH terminal's does — coalesced,
//! base64 `terminal-output-{id}` events, then `terminal-closed-{id}` — so the
//! same TerminalView draws both.
//!
//! The page only ever names a shell by id from [`shells`]: what each id runs
//! is decided here. The one free choice, a program of the user's own, is made
//! in a native dialog (as Live Edit's editor is), never passed in by the page.

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

/// A shell the Home view can start.
#[derive(Serialize, Clone, Debug)]
pub struct Shell {
    pub id: String,
    pub label: String,
    #[serde(skip)]
    program: PathBuf,
    #[serde(skip)]
    args: Vec<String>,
}

fn shell(id: impl Into<String>, label: impl Into<String>, program: PathBuf, args: &[&str]) -> Shell {
    Shell { id: id.into(), label: label.into(), program, args: args.iter().map(|a| a.to_string()).collect() }
}

/// The program picked with "Choose program…", while it's still there. On
/// Windows that can be an app alias from the Store (wt.exe, pwsh.exe), which
/// only symlink_metadata sees.
fn custom_program(config: &Config) -> Option<PathBuf> {
    let p = PathBuf::from(config.custom.as_deref()?);
    let there = p.is_file() || (cfg!(windows) && std::fs::symlink_metadata(&p).is_ok_and(|m| !m.is_dir()));
    there.then_some(p)
}

/// The shells on this computer, the usual one first. Programs go by full
/// path: a bare name would be looked for next to submarine.exe first.
/// `list_wsl`: also name each WSL distribution, which asks wsl.exe.
fn shells(config: &Config, list_wsl: bool) -> Vec<Shell> {
    let mut out = system_shells(list_wsl);
    if let Some(custom) = custom_program(config) {
        let name = custom.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(shell("custom", format!("{} (your choice)", name), custom, &[]));
    }
    out
}

/// The shell an id names, without listing WSL's distributions: a `wsl:` id
/// carries its distribution's name.
fn find_shell(config: &Config, id: &str) -> Option<Shell> {
    #[cfg(windows)]
    if let Some(name) = id.strip_prefix("wsl:") {
        let wsl = crate::local_open::system32().join("wsl.exe");
        return (wsl.is_file() && valid_distro(name)).then(|| wsl_shell(&wsl, Some(name)));
    }
    shells(config, false).into_iter().find(|s| s.id == id)
}

#[cfg(windows)]
fn wsl_shell(wsl: &std::path::Path, distro: Option<&str>) -> Shell {
    match distro {
        Some(d) => shell(format!("wsl:{}", d), format!("WSL: {}", d), wsl.to_path_buf(), &["-d", d, "--cd", "~"]),
        None => shell("wsl", "WSL", wsl.to_path_buf(), &["--cd", "~"]),
    }
}

#[cfg(windows)]
fn system_shells(list_wsl: bool) -> Vec<Shell> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let sys = crate::local_open::system32();
    let mut out = Vec::new();
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
        out.push(shell("pwsh", "PowerShell 7", p, &["-NoLogo"]));
    }
    out.push(shell("powershell", "Windows PowerShell", sys.join(r"WindowsPowerShell\v1.0\powershell.exe"), &["-NoLogo"]));
    out.push(shell("cmd", "Command Prompt", sys.join("cmd.exe"), &[]));
    // Git for Windows' bash itself, not git-bash.exe, which opens a window
    // of its own.
    let git_bash = [env("ProgramFiles"), env("ProgramFiles(x86)"), env("LOCALAPPDATA").map(|d| d.join("Programs"))]
        .into_iter()
        .flatten()
        .map(|d| d.join(r"Git\bin\bash.exe"))
        .find(|p| p.is_file());
    if let Some(p) = git_bash {
        out.push(shell("gitbash", "Git Bash", p, &["--login", "-i"]));
    }
    let wsl = sys.join("wsl.exe");
    if wsl.is_file() {
        let distros = if list_wsl { wsl_distros(&wsl) } else { Vec::new() };
        // The default distribution, and each one by name when there are
        // several to tell apart.
        if !list_wsl || !distros.is_empty() {
            out.push(wsl_shell(&wsl, None));
        }
        if distros.len() > 1 {
            out.extend(distros.iter().map(|d| wsl_shell(&wsl, Some(d))));
        }
    }
    out
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
/// Desktop's, which aren't shells. Given up on after a few seconds: wsl.exe
/// can take that long to answer when WSL itself is still starting.
#[cfg(windows)]
fn wsl_distros(wsl: &std::path::Path) -> Vec<String> {
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
        return Vec::new();
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return Vec::new();
            }
        }
    }
    let mut out = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_end(&mut stdout, &mut out);
    }
    parse_wsl_list(&out)
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
fn system_shells(_list_wsl: bool) -> Vec<Shell> {
    let login = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = vec![shell("login", format!("Login shell ({})", name(&login)), login.clone(), &["-l"])];
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
        out.push(shell(format!("shell:{}", p.display()), name(&p), p, &["-l"]));
    }
    out
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

#[derive(Serialize)]
pub struct LocalShells {
    shells: Vec<Shell>,
    /// The id new Home terminals start: the saved choice while it's among
    /// `shells`, else the first.
    default_id: String,
    /// The program picked with "Choose program…", if any, and whether it's
    /// gone (moved or uninstalled), so Settings can say so.
    custom: Option<String>,
    custom_missing: bool,
}

fn local_shells_info(app: &tauri::AppHandle) -> LocalShells {
    let config = read_config(app);
    let shells = shells(&config, true);
    let default_id = config
        .default
        .clone()
        .filter(|d| shells.iter().any(|s| &s.id == d))
        .or_else(|| shells.first().map(|s| s.id.clone()))
        .unwrap_or_default();
    let custom_missing = config.custom.is_some() && custom_program(&config).is_none();
    LocalShells { shells, default_id, custom: config.custom, custom_missing }
}

/// The saved choice while that shell is still here, else the first.
fn default_shell(config: &Config) -> Option<Shell> {
    config
        .default
        .as_deref()
        .and_then(|d| find_shell(config, d))
        .or_else(|| shells(config, false).into_iter().next())
}

#[tauri::command]
pub async fn list_local_shells(app: tauri::AppHandle) -> LocalShells {
    local_shells_info(&app)
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
    Ok(local_shells_info(&app))
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
        let Some(path) = dialog.pick_file() else { return Ok(local_shells_info(&app)) };
        let mut config = read_config(&app);
        config.custom = Some(path.to_string_lossy().into_owned());
        config.default = Some("custom".into());
        write_config(&app, &config)?;
        Ok(local_shells_info(&app))
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
    Ok(local_shells_info(&app))
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

    /// Open a terminal for the page: output as `terminal-output-{id}`
    /// events, then a line saying the shell has ended and
    /// `terminal-closed-{id}`.
    pub fn open(app: &tauri::AppHandle, terms: &LocalTerms, id: &str, shell: &Shell, cols: u16, rows: u16) -> Result<(), String> {
        let (out_app, out_id) = (app.clone(), id.to_string());
        let (end_app, end_id) = (app.clone(), id.to_string());
        spawn(
            terms,
            id,
            shell,
            app.path().home_dir().ok(),
            (cols, rows),
            move |batch| crate::ssh_manager::emit_terminal_batch(&out_app, &out_id, batch),
            move || {
                crate::ssh_manager::emit_terminal_batch(
                    &end_app,
                    &end_id,
                    &mut b"\r\n\x1b[2m[The shell has ended. Close this tab, or open a new one with +.]\x1b[0m\r\n".to_vec(),
                );
                let _ = end_app.emit(&format!("terminal-closed-{}", end_id), serde_json::json!({}));
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
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &terms, terminal_id, shell, cols, rows);
        Err("Local terminals aren't available on Android.".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let terms = terms.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let config = read_config(&app);
            let chosen = match shell {
                Some(id) => find_shell(&config, &id),
                None => default_shell(&config),
            }
            .ok_or("That shell isn't on this computer.")?;
            pty::open(&app, &terms, &terminal_id, &chosen, cols, rows)?;
            Ok(chosen.label)
        })
        .await
        .map_err(|e| format!("[LOCAL_TERM] OPEN_FAILED: {}", e))?
    }
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
pub async fn local_terminal_close(terms: tauri::State<'_, LocalTerms>, terminal_id: String) -> Result<(), String> {
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
        assert!(!shells(&config, false).iter().any(|s| s.id == "custom"));
        let here = std::env::current_exe().unwrap();
        let config = Config { default: None, custom: Some(here.to_string_lossy().into_owned()) };
        assert!(shells(&config, false).iter().any(|s| s.id == "custom"));
        assert_eq!(find_shell(&config, "custom").map(|s| s.program), Some(here));
    }

    #[cfg(unix)]
    fn run_shell(terms: &pty::LocalTerms, id: &str) -> (std::sync::mpsc::Receiver<Vec<u8>>, std::sync::mpsc::Receiver<()>) {
        let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (end_tx, end_rx) = std::sync::mpsc::channel::<()>();
        let sh = shell("t", "sh", PathBuf::from("/bin/sh"), &[]);
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
        let sh = shell("t", "sh", PathBuf::from("/bin/sh"), &[]);
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
