//! Handing files that came from a server to this computer.
//!
//! On Windows the default action for an `.exe`, `.bat`, `.lnk`, `.hta`,
//! `.js`, … is to run it, and Git for Windows / Python make `.sh` and `.py`
//! run too. macOS runs a `.terminal` or `.command` file in Terminal, and
//! Linux desktops hand a `.jar` or a `.desktop` file to what runs it. So
//! "open this remote file in its default app" could run it here. This module
//! keeps that from happening:
//!
//! - Live Edit opens its copy in a text editor: the one picked under
//!   Settings, or else Notepad on Windows, the default text editor on macOS
//!   (`open -t`), and the text editor set for plain text on Linux. Never the
//!   file's own default app: a list of the types that run can't be complete
//!   (Wine runs Windows programs and scripts on Linux, for one).
//! - The Local pane asks before opening a file the system would run
//!   ([`runs_when_opened`]): by its type, and on macOS and Linux also when
//!   it's marked executable.
//! - Files downloaded from a server are marked as coming from the internet
//!   ([`mark_from_internet`]), as browsers mark their downloads, so
//!   SmartScreen and Office Protected View on Windows, and Gatekeeper on
//!   macOS, treat them as downloads.

use std::path::{Path, PathBuf};
use tauri::Manager;

const EDITOR_FILE: &str = "live_edit_editor.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct EditorChoice {
    path: String,
}

/// A small settings file of this computer's — a program picked in a native
/// dialog, which the page can't set — kept as JSON under app_config_dir (so
/// `submarine-data` in portable mode). Live Edit's editor here, the Home
/// terminal's shell in local_term.rs.
fn setting_file(app: &tauri::AppHandle, name: &str) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(name))
}

pub(crate) fn read_setting<T: serde::de::DeserializeOwned>(app: &tauri::AppHandle, name: &str) -> Option<T> {
    let raw = std::fs::read(setting_file(app, name)?).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// `what` names the setting in the error ("the editor").
pub(crate) fn write_setting<T: serde::Serialize>(app: &tauri::AppHandle, name: &str, value: &T, what: &str) -> Result<(), String> {
    let file = setting_file(app, name).ok_or("Couldn't find the settings folder.")?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save {}: {}", what, e))?;
    }
    let json = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| format!("Couldn't save {}: {}", what, e))
}

/// Forget a setting: a file that isn't there is already forgotten.
pub(crate) fn remove_setting(app: &tauri::AppHandle, name: &str, what: &str) -> Result<(), String> {
    let Some(file) = setting_file(app, name) else { return Ok(()) };
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't reset {}: {}", what, e)),
    }
}

/// `%SystemRoot%\System32`. Windows' own programs are started from here by
/// full path: a bare `notepad.exe` would be looked for next to submarine.exe
/// first, which in portable mode is a folder on a USB stick.
#[cfg(windows)]
pub(crate) fn system32() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
}

/// The code units of UTF-16 LE text, as regedit, mstsc and wsl.exe write it
/// (a stray last byte is dropped).
pub(crate) fn utf16le_units(bytes: &[u8]) -> Vec<u16> {
    bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// The editor picked under Settings, while it is still there. A program
/// file, or on macOS an app bundle (a folder, e.g. Zed.app).
fn chosen_editor(app: &tauri::AppHandle) -> Option<PathBuf> {
    let choice: EditorChoice = read_setting(app, EDITOR_FILE)?;
    let path = PathBuf::from(choice.path);
    (path.is_file() || is_app_bundle(&path)).then_some(path)
}

fn is_app_bundle(path: &Path) -> bool {
    cfg!(target_os = "macos") && path.is_dir() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

#[cfg(windows)]
fn notepad() -> PathBuf {
    system32().join("notepad.exe")
}

#[derive(serde::Serialize)]
pub struct LiveEditEditor {
    /// The editor picked under Settings; None means the default.
    chosen: Option<String>,
    /// What the default is on this system.
    default_label: &'static str,
}

fn editor_info(app: &tauri::AppHandle) -> LiveEditEditor {
    LiveEditEditor {
        chosen: chosen_editor(app).map(|p| p.to_string_lossy().into_owned()),
        default_label: if cfg!(windows) {
            "Notepad"
        } else if cfg!(target_os = "macos") {
            "the default text editor"
        } else {
            "the text editor for plain text"
        },
    }
}

#[tauri::command]
pub async fn live_edit_editor_get(app: tauri::AppHandle) -> LiveEditEditor {
    editor_info(&app)
}

/// Pick the editor in a native dialog. The page never passes a path in:
/// whatever is stored here is started with files from servers, so only a
/// person at the dialog can choose it.
#[tauri::command]
pub async fn live_edit_editor_pick(app: tauri::AppHandle) -> Result<LiveEditEditor, String> {
    #[cfg(target_os = "android")]
    {
        let _ = &app;
        Err("Live Edit isn't available on Android.".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut dialog = rfd::FileDialog::new().set_title("Choose the editor for remote files");
        if cfg!(windows) {
            dialog = dialog.add_filter("Programs", &["exe"]);
        }
        let Some(path) = dialog.pick_file() else { return Ok(editor_info(&app)) };
        let choice = EditorChoice { path: path.to_string_lossy().into_owned() };
        write_setting(&app, EDITOR_FILE, &choice, "the editor")?;
        Ok(editor_info(&app))
    }
}

/// Back to the default editor.
#[tauri::command]
pub async fn live_edit_editor_reset(app: tauri::AppHandle) -> Result<LiveEditEditor, String> {
    remove_setting(&app, EDITOR_FILE, "the editor")?;
    Ok(editor_info(&app))
}

/// Open a Live Edit copy for editing — in an editor, never by the file's own
/// default action.
#[cfg(not(target_os = "android"))]
pub fn open_for_editing(app: &tauri::AppHandle, file: &Path) -> Result<(), String> {
    if let Some(editor) = chosen_editor(app) {
        return start_editor(&editor, file);
    }
    #[cfg(windows)]
    {
        start_editor(&notepad(), file)
    }
    // `open -t` opens it in the default text editor (TextEdit, unless
    // another one is set), never as the file's own type would be opened:
    // a `.terminal` or `.command` file would run in Terminal.
    #[cfg(target_os = "macos")]
    {
        let mut child = std::process::Command::new("/usr/bin/open")
            .arg("-t")
            .arg(file)
            .spawn()
            .map_err(|e| format!("couldn't start the text editor: {e}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        open_in_text_editor(file)
    }
}

/// Linux: the copy opens in the program the desktop sets for plain text
/// (`xdg-mime query default text/plain`), started by `gtk-launch`, never by
/// its own type's association. Where either isn't there, it isn't opened:
/// the error says to pick an editor.
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
fn open_in_text_editor(file: &Path) -> Result<(), String> {
    let pick_one = "there's no text editor set for plain text here, and a file from a server isn't opened with its default app. Pick an editor for Live Edit in Settings.";
    let query = std::process::Command::new("xdg-mime")
        .args(["query", "default", "text/plain"])
        .output()
        .map_err(|_| pick_one.to_string())?;
    let id = String::from_utf8_lossy(&query.stdout).trim().to_string();
    if !query.status.success() || !is_desktop_id(&id) {
        return Err(pick_one.into());
    }
    let mut child = std::process::Command::new("gtk-launch")
        .arg(&id)
        .arg(file)
        .spawn()
        .map_err(|_| pick_one.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A desktop entry's id as xdg-mime gives it (`org.gnome.TextEditor.desktop`):
/// nothing a command line could read as more than one name.
#[cfg(any(all(unix, not(target_os = "macos"), not(target_os = "android")), test))]
fn is_desktop_id(id: &str) -> bool {
    id.ends_with(".desktop")
        && !id.starts_with('-')
        && id.len() <= 255
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(not(target_os = "android"))]
fn start_editor(editor: &Path, file: &Path) -> Result<(), String> {
    // An app bundle is started through `open -a`, as Finder would.
    let mut cmd = if is_app_bundle(editor) {
        let mut c = std::process::Command::new("/usr/bin/open");
        c.arg("-a").arg(editor);
        c
    } else {
        std::process::Command::new(editor)
    };
    let mut child = cmd
        .arg(file)
        .spawn()
        .map_err(|e| format!("couldn't start {}: {}", editor.display(), e))?;
    // Reap it when it exits; on Unix it would otherwise linger as a zombie
    // until the app quits.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// File types Windows runs, rather than shows, when they are opened:
/// programs, scripts, shortcuts, installers, disk images, settings files and
/// the macro-enabled Office formats. The older .doc / .xls / .ppt can carry
/// macros too but are everyday documents, so they open without asking;
/// Office blocks macros in files marked as downloaded (mark_from_internet).
/// Anything %PATHEXT% lists counts too. On macOS and Linux these are asked
/// about as well: Wine runs Windows programs and scripts there.
const RUNS_ON_OPEN: &[&str] = &[
    // programs and installers
    "exe", "com", "scr", "pif", "cpl", "msi", "msp", "mst", "msu", "msix", "msixbundle",
    "appx", "appxbundle", "appinstaller", "application", "appref-ms", "xbap", "gadget", "jar", "jnlp",
    // scripts
    "bat", "cmd", "ps1", "ps1xml", "psc1", "psd1", "psm1", "ps2", "ps2xml", "psc2",
    "vb", "vbe", "vbs", "js", "jse", "wsf", "wsh", "wsc", "ws", "sct", "hta", "htc",
    "py", "pyw", "pyc", "pyo", "pyz", "pyzw", "pl", "rb", "sh", "bash", "ksh", "csh",
    // shortcuts and shell files
    "lnk", "url", "website", "scf", "shb", "shs", "library-ms", "search-ms", "searchconnector-ms",
    "settingcontent-ms", "rdp", "ica",
    // system settings, help and containers
    "msc", "reg", "inf", "ins", "isp", "chm", "hlp", "cab", "iso", "img", "vhd", "vhdx", "wsb",
    "theme", "themepack", "desktopthemepackfile", "diagcab", "diagcfg", "diagpkg", "xll", "xnk", "vsto",
    // Access and macro-enabled Office files
    "mdb", "mde", "accde", "accdr", "ade", "adp",
    "docm", "dotm", "xlsm", "xltm", "xlam", "pptm", "potm", "ppam", "ppsm", "sldm",
];

/// File types macOS or a Linux desktop runs, or hands to something that runs
/// them, when they're opened: Terminal settings and commands, Automator and
/// AppleScript, locations that open other files or apps, installers and
/// configuration profiles, desktop entries, scripts an interpreter may be set
/// to run, Java archives and Linux programs. Downloads aren't marked
/// executable, but several of these don't need to be.
#[cfg(any(not(windows), test))]
const RUNS_ON_OPEN_UNIX: &[&str] = &[
    "terminal", "command", "tool", "workflow", "fileloc", "webloc", "inetloc",
    "pkg", "mpkg", "mobileconfig", "scpt", "scptd", "applescript", "desktop", "app",
    "sh", "bash", "zsh", "py", "pl", "rb", "jar", "appimage", "run", "bin",
];

/// The extension the system goes by. Windows drops trailing dots and spaces
/// from a name, so `x.exe.` and `x.exe ` run as `x.exe`; elsewhere they're
/// dropped too, to be on the safe side.
fn file_extension(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let name = name.trim_end_matches(['.', ' ']);
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

#[cfg(any(windows, test))]
fn runs_on_open_ext(ext: &str, pathext: Option<&str>) -> bool {
    RUNS_ON_OPEN.contains(&ext)
        || pathext.is_some_and(|pe| {
            pe.split(';').any(|p| {
                let p = p.trim().trim_start_matches('.');
                !p.is_empty() && p.eq_ignore_ascii_case(ext)
            })
        })
}

/// Whether opening `path` would run it rather than show it (on Windows,
/// RUNS_ON_OPEN and %PATHEXT%; on macOS and Linux, RUNS_ON_OPEN_UNIX, or a
/// file marked executable, whatever its name: a program or script without
/// an extension runs when a file manager opens it).
pub fn runs_when_opened(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let pathext = std::env::var("PATHEXT").ok();
        file_extension(path).is_some_and(|ext| runs_on_open_ext(&ext, pathext.as_deref()))
    }
    #[cfg(not(windows))]
    {
        file_extension(path).is_some_and(|ext| runs_on_open_unix(&ext)) || has_run_bits(path)
    }
}

/// A regular file (a link is followed, as opening it would) that anyone may
/// run: any of its `x` bits is set.
#[cfg(unix)]
fn has_run_bits(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(any(unix, windows)))]
fn has_run_bits(_path: &Path) -> bool {
    false
}

/// macOS and Linux: their own types that run, and Windows' (Wine).
#[cfg(any(not(windows), test))]
fn runs_on_open_unix(ext: &str) -> bool {
    RUNS_ON_OPEN_UNIX.contains(&ext) || RUNS_ON_OPEN.contains(&ext)
}

/// Mark a file from a server the way browsers mark downloads: the
/// `Zone.Identifier` stream with zone 3 (Internet). Windows then asks before
/// running it, and Office opens it in Protected View with macros blocked.
/// Best effort: a drive without alternate data streams (a FAT32 or exFAT
/// stick) leaves the file unmarked.
pub fn mark_from_internet(path: &Path) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(PathBuf::from(stream), b"[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    // Gatekeeper's mark (as Safari sets it): the system then asks before
    // anything in it runs, and says where it came from.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let value = quarantine_value(now);
        let Ok(file) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
        // SAFETY: both strings are NUL-terminated and the value's length is its own.
        unsafe {
            libc::setxattr(file.as_ptr(), c"com.apple.quarantine".as_ptr(), value.as_ptr().cast(), value.len(), 0, 0);
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
    }
}

/// Whether `path` is marked as from the internet (mark_from_internet, or a
/// browser's mark): on Windows a `Zone.Identifier` stream for the Internet or
/// Restricted zone (or one that doesn't say), on macOS a quarantine mark.
/// Never on other systems, which have no such mark.
pub fn is_marked_from_internet(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        std::fs::read(PathBuf::from(stream)).is_ok_and(|text| zone_is_internet(&String::from_utf8_lossy(&text)))
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(file) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
        // SAFETY: both strings are NUL-terminated; with no buffer (null, size
        // 0) it only answers the value's size, or -1 when there's no mark.
        unsafe { libc::getxattr(file.as_ptr(), c"com.apple.quarantine".as_ptr(), std::ptr::null_mut(), 0, 0, 0) >= 0 }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        false
    }
}

/// Whether a `Zone.Identifier` stream's text puts the file in the Internet
/// (3) or Restricted (4) zone. One without a zone that can be read counts
/// too: something marked it.
#[cfg(any(windows, test))]
fn zone_is_internet(text: &str) -> bool {
    match text.lines().find_map(|line| line.trim().strip_prefix("ZoneId=")) {
        Some(zone) => !matches!(zone.trim().parse::<u32>(), Ok(zone) if zone < 3),
        None => true,
    }
}

/// The `com.apple.quarantine` value for a file from the internet: the flags
/// (0081: downloaded, not yet opened), the time in hex, and the app.
#[cfg(any(target_os = "macos", test))]
fn quarantine_value(unix_secs: u64) -> String {
    format!("0081;{unix_secs:08x};Submarine;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(name: &str) -> bool {
        file_extension(Path::new(name)).is_some_and(|e| runs_on_open_ext(&e, Some(".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WSF;.MSC;.PY")))
    }

    #[test]
    fn programs_scripts_and_shortcuts_run() {
        for name in ["setup.exe", "x.BAT", "deploy.sh", "tool.py", "a.lnk", "b.hta", "c.js", "d.url", "e.msi", "f.ps1", "g.xlsm", "h.rdp"] {
            assert!(runs(name), "{name}");
        }
    }

    #[test]
    fn trailing_dots_and_spaces_dont_hide_the_extension() {
        assert!(runs("invoice.exe."));
        assert!(runs("invoice.exe . "));
    }

    #[test]
    fn documents_and_text_open() {
        for name in ["notes.txt", "photo.png", "report.pdf", "data.csv", "nginx.conf", "Makefile", ".bashrc", "letter.docx", "archive.zip"] {
            assert!(!runs(name), "{name}");
        }
    }

    fn runs_unix(name: &str) -> bool {
        file_extension(Path::new(name)).is_some_and(|e| runs_on_open_unix(&e))
    }

    #[test]
    fn windows_programs_and_scripts_are_asked_about_on_macos_and_linux_too() {
        // Wine runs them there when they're opened.
        for name in ["setup.exe", "x.MSI", "a.lnk", "run.bat", "x.cmd", "x.vbs", "b.hta", "c.js", "d.ps1", "e.reg", "invoice.pdf .exe"] {
            assert!(runs_unix(name), "{name}");
        }
    }

    #[test]
    fn macos_and_linux_types_that_run_are_asked_about() {
        for name in [
            "report.pdf      .terminal", "x.command", "x.TOOL", "a.workflow", "b.fileloc", "c.webloc", "d.inetloc", "setup.pkg",
            "e.mpkg", "f.mobileconfig", "g.scpt", "h.scptd", "i.applescript", "app.desktop", "Thing.app", "deploy.sh", "x.bash",
            "x.zsh", "tool.py", "x.pl", "x.rb", "app.jar", "Tool.AppImage", "installer.run", "firmware.bin", "x.terminal.",
        ] {
            assert!(runs_unix(name), "{name}");
        }
        for name in ["notes.txt", "photo.png", "report.pdf", "data.csv", "nginx.conf", "Makefile", ".bashrc", "letter.docx", "x.terminal.txt"] {
            assert!(!runs_unix(name), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_marked_executable_runs_whatever_its_name() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("submarine-runbits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let set = |p: &Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
        for (name, mode, runs) in [("install", 0o755, true), ("notes", 0o644, false), ("readme.txt", 0o700, true), ("group-only", 0o610, true)] {
            let file = dir.join(name);
            std::fs::write(&file, "#!/bin/sh\n").unwrap();
            set(&file, mode);
            assert_eq!(runs_when_opened(&file), runs, "{name} {mode:o}");
        }
        let folder = dir.join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert!(!runs_when_opened(&folder), "a folder isn't run");
        std::os::unix::fs::symlink(dir.join("install"), dir.join("link")).unwrap();
        assert!(runs_when_opened(&dir.join("link")), "a link to a program");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_zone_mark_says_internet() {
        assert!(zone_is_internet("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n"));
        assert!(zone_is_internet("[ZoneTransfer]\nZoneId=4\n"));
        assert!(!zone_is_internet("[ZoneTransfer]\r\nZoneId=1\r\n"));
        assert!(!zone_is_internet("[ZoneTransfer]\r\nZoneId=2\r\n"));
        assert!(zone_is_internet("garbled"), "marked, without a zone that can be read");
    }

    #[test]
    fn the_quarantine_mark_is_in_the_form_macos_reads() {
        assert_eq!(quarantine_value(0x6543_21ab), "0081;654321ab;Submarine;");
        assert_eq!(quarantine_value(5), "0081;00000005;Submarine;");
    }

    #[test]
    fn only_a_plain_desktop_id_is_started() {
        assert!(is_desktop_id("org.gnome.TextEditor.desktop"));
        assert!(is_desktop_id("gedit.desktop"));
        for bad in ["", "gedit", "-x.desktop", "a b.desktop", "a;b.desktop", "../x.desktop", "a/b.desktop"] {
            assert!(!is_desktop_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn pathext_extends_the_list() {
        assert!(!runs_on_open_ext("foo", Some(".COM;.EXE")));
        assert!(runs_on_open_ext("foo", Some(".COM;.EXE;.FOO")));
        assert!(!runs_on_open_ext("txt", Some(";;")));
    }
}
