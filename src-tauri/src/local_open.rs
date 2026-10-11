//! Handing files that came from a server to this computer.
//!
//! On Windows the default action for an `.exe`, `.bat`, `.lnk`, `.hta`,
//! `.js`, … is to run it, and Git for Windows / Python make `.sh` and `.py`
//! run too. So "open this remote file in its default app" could run it here.
//! This module keeps that from happening:
//!
//! - Live Edit opens its copy in a text editor: the one picked under
//!   Settings, or Notepad on Windows. Other systems keep their default app
//!   unless an editor is picked.
//! - The Local pane asks before opening a file Windows would run
//!   ([`runs_when_opened`]).
//! - Files downloaded from a server are marked as coming from the internet
//!   ([`mark_from_internet`]), as browsers mark their downloads, so
//!   SmartScreen and Office Protected View treat them as downloads.

use std::path::{Path, PathBuf};
use tauri::Manager;

/// Under app_config_dir (so `submarine-data` in portable mode).
const EDITOR_FILE: &str = "live_edit_editor.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct EditorChoice {
    path: String,
}

fn editor_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(EDITOR_FILE))
}

/// The editor picked under Settings, while it is still there. A program
/// file, or on macOS an app bundle (a folder, e.g. Zed.app).
fn chosen_editor(app: &tauri::AppHandle) -> Option<PathBuf> {
    let raw = std::fs::read(editor_file(app)?).ok()?;
    let choice: EditorChoice = serde_json::from_slice(&raw).ok()?;
    let path = PathBuf::from(choice.path);
    (path.is_file() || is_app_bundle(&path)).then_some(path)
}

fn is_app_bundle(path: &Path) -> bool {
    cfg!(target_os = "macos") && path.is_dir() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

/// By full path: a bare `notepad.exe` would be looked for next to
/// submarine.exe first, which in portable mode is a folder on a USB stick.
#[cfg(windows)]
fn notepad() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("notepad.exe")
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
        default_label: if cfg!(windows) { "Notepad" } else { "the system's default app" },
    }
}

#[tauri::command]
pub fn live_edit_editor_get(app: tauri::AppHandle) -> LiveEditEditor {
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
        let file = editor_file(&app).ok_or("No settings folder")?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save the editor: {}", e))?;
        }
        let choice = EditorChoice { path: path.to_string_lossy().into_owned() };
        let json = serde_json::to_vec(&choice).map_err(|e| e.to_string())?;
        std::fs::write(&file, json).map_err(|e| format!("Couldn't save the editor: {}", e))?;
        Ok(editor_info(&app))
    }
}

/// Back to the default editor.
#[tauri::command]
pub fn live_edit_editor_reset(app: tauri::AppHandle) -> Result<LiveEditEditor, String> {
    if let Some(file) = editor_file(&app) {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Couldn't reset the editor: {}", e)),
        }
    }
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
    #[cfg(not(windows))]
    {
        open::that(file).map_err(|e| e.to_string())
    }
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
/// Anything %PATHEXT% lists counts too.
#[cfg(any(windows, test))]
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
    "theme", "themepack", "desktopthemepackfile", "diagcab", "diagcfg", "diagpkg", "xll", "xnk",
    // Access and macro-enabled Office files
    "mdb", "mde", "accde", "accdr", "ade", "adp",
    "docm", "dotm", "xlsm", "xltm", "xlam", "pptm", "potm", "ppam", "ppsm", "sldm",
];

/// The extension Windows goes by: it drops trailing dots and spaces from a
/// name, so `x.exe.` and `x.exe ` run as `x.exe`.
#[cfg(any(windows, test))]
fn windows_extension(path: &Path) -> Option<String> {
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

/// Whether opening `path` would run it rather than show it. Windows only;
/// elsewhere opening a file doesn't run it unless it is marked executable,
/// which downloads aren't.
pub fn runs_when_opened(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let pathext = std::env::var("PATHEXT").ok();
        windows_extension(path).is_some_and(|ext| runs_on_open_ext(&ext, pathext.as_deref()))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
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
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(name: &str) -> bool {
        windows_extension(Path::new(name)).is_some_and(|e| runs_on_open_ext(&e, Some(".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WSF;.MSC;.PY")))
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

    #[test]
    fn pathext_extends_the_list() {
        assert!(!runs_on_open_ext("foo", Some(".COM;.EXE")));
        assert!(runs_on_open_ext("foo", Some(".COM;.EXE;.FOO")));
        assert!(!runs_on_open_ext("txt", Some(";;")));
    }
}
