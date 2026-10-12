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
use tauri::Manager;

/// Under app_config_dir (so `submarine-data` in portable mode).
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

fn config_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(CONFIG_FILE))
}

fn read_config(app: &tauri::AppHandle) -> Config {
    config_file(app)
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn write_config(app: &tauri::AppHandle, config: &Config) -> Result<(), String> {
    let file = config_file(app).ok_or("Couldn't find the settings folder.")?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save the Home terminal: {}", e))?;
    }
    let json = serde_json::to_vec(config).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| format!("Couldn't save the Home terminal: {}", e))
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

/// The shells on this computer, the usual one first. Programs go by full
/// path: a bare name would be looked for next to submarine.exe first.
/// `list_wsl`: also name each WSL distribution, which asks wsl.exe.
fn shells(config: &Config, list_wsl: bool) -> Vec<Shell> {
    let mut out = system_shells(list_wsl);
    if let Some(custom) = config.custom.as_deref().map(PathBuf::from).filter(|p| p.is_file()) {
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
        let wsl = system32().join("wsl.exe");
        return (wsl.is_file() && valid_distro(name)).then(|| wsl_shell(&wsl, Some(name)));
    }
    shells(config, false).into_iter().find(|s| s.id == id)
}

#[cfg(windows)]
fn system32() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
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
    let sys = system32();
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

#[cfg(any(windows, test))]
fn valid_distro(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// The WSL distributions installed, from `wsl.exe -l -q` (UTF-16), leaving
/// out Docker Desktop's, which aren't shells.
#[cfg(windows)]
fn wsl_distros(wsl: &std::path::Path) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(out) = std::process::Command::new(wsl).args(["-l", "-q"]).creation_flags(CREATE_NO_WINDOW).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_wsl_list(&out.stdout)
}

/// `wsl -l -q` writes UTF-16 LE, one name per line.
#[cfg(any(windows, test))]
fn parse_wsl_list(raw: &[u8]) -> Vec<String> {
    let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
        .lines()
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
    /// The id new Home terminals start: the saved choice while that shell is
    /// still here, else the first.
    default_id: String,
    /// The program picked with "Choose program…", if any.
    custom: Option<String>,
}

fn local_shells_info(app: &tauri::AppHandle) -> LocalShells {
    let config = read_config(app);
    let shells = shells(&config, true);
    let default_id = default_shell(&config).map(|s| s.id).unwrap_or_default();
    LocalShells { shells, default_id, custom: config.custom }
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
pub async fn local_shells(app: tauri::AppHandle) -> LocalShells {
    local_shells_info(&app)
}

/// The shell new Home terminals start: one of `local_shells`.
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

#[cfg(not(target_os = "android"))]
mod pty {
    use super::*;
    use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;
    use tauri::Emitter;

    struct Term {
        master: Box<dyn MasterPty + Send>,
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
        killer: Box<dyn ChildKiller + Send + Sync>,
    }

    /// The open local terminals, by terminal id.
    #[derive(Default, Clone)]
    pub struct LocalTerms(Arc<Mutex<HashMap<String, Term>>>);

    impl LocalTerms {
        fn take(&self, id: &str) -> Option<Term> {
            self.0.lock().ok()?.remove(id)
        }
    }

    /// The most a batch carries, and how long output may wait to join one —
    /// as for an SSH terminal (ssh_manager::emit_terminal_batch).
    const BATCH_MAX: usize = 64 * 1024;
    const BATCH_WAIT: Duration = Duration::from_millis(8);

    pub fn open(app: &tauri::AppHandle, terms: &LocalTerms, id: &str, shell: &Shell, cols: u16, rows: u16) -> Result<(), String> {
        if !valid_id(id) {
            return Err("[LOCAL_TERM] BAD_ID".into());
        }
        if terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?.contains_key(id) {
            return Err("[LOCAL_TERM] ALREADY_OPEN".into());
        }
        let size = PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 };
        let pair = native_pty_system().openpty(size).map_err(|e| format!("Couldn't open a terminal: {}", e))?;
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        if let Ok(home) = app.path().home_dir() {
            cmd.cwd(home);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Couldn't start {}: {}", shell.label, e))?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let killer = child.clone_killer();
        terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?.insert(
            id.to_string(),
            Term { master: pair.master, writer: Arc::new(Mutex::new(writer)), killer },
        );

        // Output: a reader thread hands chunks to a sender that coalesces
        // them, so a burst of small reads becomes a few events.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || pump_reader(reader, tx));
        let (app2, id2) = (app.clone(), id.to_string());
        std::thread::spawn(move || {
            send_batches(&app2, &id2, rx);
            crate::ssh_manager::emit_terminal_batch(
                &app2,
                &id2,
                &mut b"\r\n\x1b[2m[The shell has ended. Close this tab, or open a new one with +.]\x1b[0m\r\n".to_vec(),
            );
            let _ = app2.emit(&format!("terminal-closed-{}", id2), serde_json::json!({}));
        });

        // When the shell exits, let go of the terminal: on Windows the
        // output pipe only ends once the pseudo-console is closed.
        let (terms2, id3) = (terms.clone(), id.to_string());
        std::thread::spawn(move || {
            let _ = child.wait();
            std::thread::sleep(Duration::from_millis(200));
            drop(terms2.take(&id3));
        });
        Ok(())
    }

    fn pump_reader(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    }

    fn send_batches(app: &tauri::AppHandle, id: &str, rx: mpsc::Receiver<Vec<u8>>) {
        let mut batch = Vec::new();
        while let Ok(first) = rx.recv() {
            batch.extend_from_slice(&first);
            while batch.len() < BATCH_MAX {
                match rx.recv_timeout(BATCH_WAIT) {
                    Ok(more) => batch.extend_from_slice(&more),
                    Err(_) => break,
                }
            }
            crate::ssh_manager::emit_terminal_batch(app, id, &mut batch);
        }
    }

    pub fn write(terms: &LocalTerms, id: &str, data: &[u8]) -> Result<(), String> {
        // Held only to find the terminal: a shell slow to read blocks its own
        // writes, not every terminal's.
        let writer = {
            let map = terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
            map.get(id).map(|t| t.writer.clone()).ok_or("[LOCAL_TERM] NOT_OPEN")?
        };
        let mut w = writer.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        w.write_all(data).and_then(|_| w.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(terms: &LocalTerms, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let map = terms.0.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let t = map.get(id).ok_or("[LOCAL_TERM] NOT_OPEN")?;
        t.master
            .resize(PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())
    }

    pub fn close(terms: &LocalTerms, id: &str) {
        // Out of the map first, then ended and dropped without the lock held:
        // closing a pseudo-console waits for its output to drain.
        if let Some(mut t) = terms.take(id) {
            let _ = t.killer.kill();
            drop(t);
        }
    }

    #[cfg(test)]
    pub(super) fn open_for_test(terms: &LocalTerms, id: &str, shell: &Shell) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let killer = child.clone_killer();
        terms.0.lock().unwrap().insert(id.to_string(), Term { master: pair.master, writer: Arc::new(Mutex::new(writer)), killer });
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || pump_reader(reader, tx));
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(rx)
    }
}

#[cfg(not(target_os = "android"))]
pub use pty::LocalTerms;

#[cfg(target_os = "android")]
#[derive(Default)]
pub struct LocalTerms;

/// Start a shell (one of `local_shells`, or the default) in a new local
/// terminal. Returns the shell's label for the tab.
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
        let config = read_config(&app);
        let chosen = match shell {
            Some(id) => find_shell(&config, &id),
            None => default_shell(&config),
        }
        .ok_or("That shell isn't on this computer.")?;
        pty::open(&app, &terms, &terminal_id, &chosen, cols, rows)?;
        Ok(chosen.label)
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
    }
    #[cfg(not(target_os = "android"))]
    {
        pty::close(&terms, &terminal_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_list_is_utf16_and_skips_docker() {
        let mut raw = vec![0xFF, 0xFE];
        for u in "Ubuntu\r\nDebian\r\ndocker-desktop\r\ndocker-desktop-data\r\n\r\nbad name;rm\r\n".encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(parse_wsl_list(&raw), vec!["Ubuntu", "Debian"]);
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
    #[test]
    fn a_shell_runs_what_it_is_sent_and_ends() {
        let terms = LocalTerms::default();
        let sh = shell("t", "sh", PathBuf::from("/bin/sh"), &[]);
        let rx = pty::open_for_test(&terms, "local-test", &sh).unwrap();
        pty::write(&terms, "local-test", b"echo sub$((40+2))marine; exit\n").unwrap();
        let mut seen = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(chunk) => seen.extend_from_slice(&chunk),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if String::from_utf8_lossy(&seen).contains("sub42marine") { break; }
                }
                Err(_) => break,
            }
        }
        assert!(String::from_utf8_lossy(&seen).contains("sub42marine"), "{}", String::from_utf8_lossy(&seen));
        pty::resize(&terms, "local-test", 100, 30).unwrap();
        pty::close(&terms, "local-test");
        assert!(pty::write(&terms, "local-test", b"x").is_err(), "closed");
    }
}
