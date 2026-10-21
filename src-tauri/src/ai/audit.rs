//! The AI audit log: every request an AI tab makes and what came of it, on
//! this computer only. `ai-audit.log` in the app's log folder (beside
//! submarine.log, inside `submarine-data` in portable mode), one JSON object
//! per line. Kept apart from the app log so it doesn't rotate out with it:
//! past 10 MB it becomes `ai-audit.old.log` and a new one starts.
//!
//! It holds what was asked and decided, never a command's output, and never a
//! token or a password.

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const FILE_NAME: &str = "ai-audit.log";
const OLD_NAME: &str = "ai-audit.old.log";
const MAX_BYTES: u64 = 10 * 1024 * 1024;

/// One line of the log. `event` says what happened: an AI tab started or
/// ended, a server shared or no longer, a request and its outcome.
#[derive(Serialize, Default, Clone, Debug)]
pub(crate) struct Entry {
    pub time: String,
    pub event: &'static str,
    pub ai_tab: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub root: bool,
    /// allowed, denied, cancelled, refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<&'static str>,
    /// Who decided: "you", or "Submarine" for one refused before it was shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_returned: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items_hidden: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Entry {
    pub(crate) fn new(event: &'static str, ai_tab: &str) -> Entry {
        Entry { time: chrono::Local::now().to_rfc3339(), event, ai_tab: ai_tab.to_string(), ..Entry::default() }
    }
}

pub(crate) struct AuditLog {
    dir: Option<PathBuf>,
    file: Mutex<Option<(File, u64)>>,
}

impl AuditLog {
    /// A log in `dir`; with none (no log folder), nothing is written.
    pub(crate) fn new(dir: Option<PathBuf>) -> AuditLog {
        AuditLog { dir, file: Mutex::new(None) }
    }

    pub(crate) fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(FILE_NAME))
    }

    pub(crate) fn write(&self, entry: &Entry) {
        let Some(dir) = self.dir.as_deref() else { return };
        let Ok(mut line) = serde_json::to_string(entry) else { return };
        line.push('\n');
        let Ok(mut slot) = self.file.lock() else { return };
        if slot.as_ref().is_some_and(|(_, size)| *size >= MAX_BYTES) {
            *slot = None;
            let _ = std::fs::rename(dir.join(FILE_NAME), dir.join(OLD_NAME));
        }
        if slot.is_none() {
            *slot = open(dir).map(|f| {
                let size = f.metadata().map(|m| m.len()).unwrap_or(0);
                (f, size)
            });
        }
        if let Some((f, size)) = slot.as_mut() {
            if f.write_all(line.as_bytes()).is_ok() {
                *size += line.len() as u64;
            }
        }
    }
}

/// The log file, readable by you only on macOS and Linux.
fn open(dir: &Path) -> Option<File> {
    let _ = std::fs::create_dir_all(dir);
    let mut o = OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(dir.join(FILE_NAME)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_json_line_per_entry() {
        let dir = std::env::temp_dir().join(format!("submarine-audit-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = AuditLog::new(Some(dir.clone()));
        let mut e = Entry::new("request", "Claude");
        e.server = Some("xyz".into());
        e.command = Some("uptime".into());
        e.decision = Some("allowed");
        e.by = Some("you");
        e.exit_code = Some(0);
        log.write(&e);
        log.write(&Entry::new("ai_tab_ended", "Claude"));
        let text = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["command"], "uptime");
        assert_eq!(lines[0]["decision"], "allowed");
        assert!(lines[0].get("root").is_none(), "only said when true");
        assert_eq!(lines[1]["event"], "ai_tab_ended");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(FILE_NAME)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "yours only");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
