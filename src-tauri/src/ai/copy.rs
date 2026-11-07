//! The AI's `copy_files` tool (#159, step 2): push a file or a folder from
//! this computer to several shared servers, or pull one from each of them,
//! as one request on one card, as `run_on_servers` runs one command (#158).
//!
//! - **One card,** listing the servers, the direction, the source and the
//!   destination, how many files and how big (a push counts them here first;
//!   a pull finds out when it runs), and whether files already there are
//!   replaced. It runs without asking only where every server is at "Run
//!   anything" (or "Allow everything here" is on): a copy has no command for
//!   an allow rule to match (`decision`). Its answers are the group's: allow
//!   once, allow everything on these servers until the AI tab closes, deny.
//! - **The copying is the app's own,** in lib.rs: `sftp_upload_file`,
//!   `sftp_upload_dir` and `sftp_download_file`, the commands the user's own
//!   transfers and the bulk dialog use; a folder pulled goes through the
//!   pieces `sftp_download_dir` is made of (`walk_download_tree`,
//!   `download_dir_file`), so the limits below hold on what is fetched, and a
//!   file already there is left and counted. Four servers at a time.
//! - **On this computer,** a pull goes only into a folder the user added in
//!   Settings › AI tools (`pull_target`), each server's copy in a folder of
//!   its own named after the server (`folder_names`), and never replaces a
//!   file. A push reads from anywhere (the card shows the path first), and
//!   never follows a link (`walk_local`). Every check is made here, beside
//!   the copying, and made again just before each server's copy starts.
//! - **Limits** (`MAX_FILES`, `MAX_BYTES`): what one request copies to or
//!   from each server.
//! - **Logged** in the audit log as commands are: the request, the decision,
//!   and how it ended.

use super::boundary::ServerText;
use super::*;
use crate::ssh_manager::SshState;
use serde_json::Value;
use std::path::{Component, Path};
use tokio::sync::Semaphore;

/// The most files one request copies to, or from, each server.
pub(crate) static MAX_FILES: u64 = 1000;
/// The most bytes one request copies to, or from, each server: 2 GiB.
pub(crate) static MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The most folders a folder pushed may hold.
static MAX_FOLDERS: u64 = 1000;
/// How deep a folder pushed may go.
static MAX_DEPTH: usize = 64;
/// The longest path taken, in bytes.
static MAX_PATH: usize = 4096;
/// How many servers copy at once (as the bulk dialog does, plan.ts AT_ONCE).
static SERVERS_AT_ONCE: usize = 4;
/// The most folders the AI may be allowed to pull into.
pub(crate) static MAX_PULL_FOLDERS: usize = 32;
/// The longest folder name made from a server's name, in characters.
static NAME_MAX: usize = 80;

/// Where the setting is, for the AI and for the user.
static SETTING: &str = "Settings › AI tools › Folders the AI may copy into";

// -- What the AI asks ---------------------------------------------------------

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    /// From this computer to the servers.
    Push,
    /// From the servers to this computer.
    Pull,
}

/// A `copy_files` request's arguments, as given: checked for shape only.
#[derive(Debug, PartialEq)]
pub(crate) struct Args {
    pub direction: Direction,
    pub servers: Vec<String>,
    pub all: bool,
    pub source: String,
    pub destination: String,
    pub replace: bool,
}

/// A text argument: there, a string, not empty, at most MAX_PATH long, and
/// with no control characters (a NUL ends a path early; a newline hides
/// what follows on the card).
fn text_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    let v = match args.get(name) {
        None | Some(Value::Null) => return Err(format!("{name} is missing.")),
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(format!("{name} must be a path, as text.")),
    };
    if v.trim().is_empty() {
        return Err(format!("{name} is empty."));
    }
    if v.len() > MAX_PATH {
        return Err(format!("{name} is longer than {MAX_PATH} bytes."));
    }
    if v.chars().any(char::is_control) {
        return Err(format!("{name} has a control character in it."));
    }
    Ok(v)
}

fn flag_arg(args: &Value, name: &str) -> Result<bool, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{name} must be true or false.")),
    }
}

pub(crate) fn parse_args(args: &Value) -> Result<Args, String> {
    let direction = match args.get("direction").and_then(Value::as_str) {
        Some("push") => Direction::Push,
        Some("pull") => Direction::Pull,
        _ => return Err("direction must be push (from this computer to the servers) or pull (from the servers to this computer).".into()),
    };
    let servers = match args.get("servers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        Some(_) => return Err("servers must be a list of server names.".into()),
    };
    let all = flag_arg(args, "all")?;
    let source = text_arg(args, "source")?.to_string();
    let destination = text_arg(args, "destination")?.to_string();
    let replace = flag_arg(args, "replace")?;
    if replace && direction == Direction::Pull {
        return Err("replace works only with push: Submarine never replaces a file on this computer. Files already there are left, and reported.".into());
    }
    Ok(Args { direction, servers, all, source, destination, replace })
}

// -- Paths on a server --------------------------------------------------------

/// A path on a server as the AI gives it: a full path (`/srv/app`), or `~`
/// or `~/…` for the login's home folder. Given back tidy (no doubled or
/// trailing `/`). A `.` or `..` step is refused: the card shows the path the
/// copy uses, nothing to work out.
pub(crate) fn remote_path(typed: &str) -> Result<String, String> {
    let t = typed.trim();
    let (head, rest) = if t == "~" {
        return Ok("~".into());
    } else if let Some(rest) = t.strip_prefix("~/") {
        ("~", rest)
    } else if let Some(rest) = t.strip_prefix('/') {
        ("", rest)
    } else {
        return Err(format!("{t} isn't a full path on the server: start it with / (or ~/ for the login's home folder)."));
    };
    let steps: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if steps.iter().any(|s| *s == "." || *s == "..") {
        return Err(format!("{t} has a . or .. step: give the path itself."));
    }
    if steps.is_empty() {
        return Ok(if head.is_empty() { "/".into() } else { "~".into() });
    }
    Ok(format!("{head}/{}", steps.join("/")))
}

/// The last name in a tidy remote path: None for `/` and `~`.
fn remote_name(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    (!name.is_empty() && name != "~").then_some(name)
}

fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// A tidy remote path with `~` made the login's home folder, `home`.
fn resolve_remote(path: &str, home: &str) -> String {
    match path.strip_prefix('~') {
        Some("") => home.to_string(),
        Some(rest) => format!("{}{rest}", home.trim_end_matches('/')),
        None => path.to_string(),
    }
}

// -- Paths on this computer ---------------------------------------------------

/// A path on this computer as the AI gives it: a full path, with no `.` or
/// `..` step (it's taken as written, or not at all).
///
/// The steps are looked for between `/` and `\` on every system, not by how
/// this one splits a path: Windows doesn't split a `\\?\` path at `/`, so
/// `\\?\C:\allowed/../x` would have no `..` step as parsed. And on Windows
/// only a drive (`C:\…`) or a share (`\\server\share\…`) is taken: not the
/// `\\?\` and `\\.\` forms, which Windows passes on without resolving.
pub(crate) fn plain_local(typed: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(typed);
    if !p.is_absolute() || !plain_prefix(&p) {
        return Err(format!("{typed} isn't a full path on this computer."));
    }
    if typed.split(['/', '\\']).any(|step| step == "." || step == "..")
        || p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(format!("{typed} has a . or .. step: give the path itself."));
    }
    Ok(p)
}

/// A drive or a share on Windows (not `\\?\…` or `\\.\…`); on other systems,
/// any full path.
fn plain_prefix(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(..)),
        _ => !cfg!(windows),
    }
}

/// What a push sends: one file, or a folder's files (`is_dir`). Links and
/// what's neither a file nor a folder (a socket, a device) are left out and
/// counted.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LocalTree {
    pub is_dir: bool,
    pub files: u64,
    pub bytes: u64,
    pub folders: u64,
    pub links_skipped: u64,
    pub others_skipped: u64,
}

fn over_limits(files: u64, bytes: u64) -> Option<String> {
    if files > MAX_FILES {
        Some(format!("That's more than {MAX_FILES} files, more than one request may copy. Copy a smaller folder, or several."))
    } else if bytes > MAX_BYTES {
        Some(format!("That's more than {}, more than one request may copy. Copy less at once.", size_text(MAX_BYTES)))
    } else {
        None
    }
}

/// Count what a push of `root` sends, without following a link anywhere
/// (`symlink_metadata`, and a folder's entries by their own type): `root`
/// itself a link is refused, and links inside are left out, as
/// sftp_upload_dir leaves them. Refused past MAX_FILES, MAX_BYTES,
/// MAX_FOLDERS or MAX_DEPTH. Blocking: on a blocking thread.
pub(crate) fn walk_local(root: &Path) -> Result<LocalTree, String> {
    let shown = root.display();
    let meta = std::fs::symlink_metadata(root).map_err(|e| format!("Can't read {shown}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{shown} is a link: Submarine doesn't follow links on this computer. Give the path it points to."));
    }
    if meta.is_file() {
        let t = LocalTree { files: 1, bytes: meta.len(), ..LocalTree::default() };
        return over_limits(t.files, t.bytes).map_or(Ok(t), Err);
    }
    if !meta.is_dir() {
        return Err(format!("{shown} isn't a file or a folder."));
    }
    let mut t = LocalTree { is_dir: true, ..LocalTree::default() };
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let read = std::fs::read_dir(&dir).map_err(|e| format!("Can't read the folder {}: {e}", dir.display()))?;
        for entry in read {
            let entry = entry.map_err(|e| format!("Can't read the folder {}: {e}", dir.display()))?;
            // The entry's own type: a link is a link, not what it points to.
            let kind = entry.file_type().map_err(|e| format!("Can't read {}: {e}", entry.path().display()))?;
            if kind.is_symlink() {
                t.links_skipped += 1;
            } else if kind.is_dir() {
                t.folders += 1;
                if t.folders > MAX_FOLDERS {
                    return Err(format!("{shown} holds more than {MAX_FOLDERS} folders, more than one request may copy."));
                }
                if depth + 1 > MAX_DEPTH {
                    return Err(format!("{shown} has folders more than {MAX_DEPTH} deep."));
                }
                stack.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                let len = entry.metadata().map_err(|e| format!("Can't read {}: {e}", entry.path().display()))?.len();
                t.files += 1;
                t.bytes = t.bytes.saturating_add(len);
                if let Some(why) = over_limits(t.files, t.bytes) {
                    return Err(why);
                }
            } else {
                t.others_skipped += 1;
            }
        }
    }
    Ok(t)
}

/// A push's source checked: a full path, not a link, not in a system folder
/// (guard_local_path, as sftp_upload_file checks it), and counted.
fn push_source(typed: &str) -> Result<(PathBuf, LocalTree), String> {
    let path = plain_local(typed)?;
    let tree = walk_local(&path)?;
    crate::guard_local_path(typed, false)?;
    Ok((path, tree))
}

/// Whether `path` is inside `folder` (or is it), step by step: `/a/allowed2`
/// isn't inside `/a/allowed`.
fn inside(path: &Path, folder: &Path) -> bool {
    path.starts_with(folder)
}

/// Where a pull into `typed` goes: a folder inside one of `allowed` (the
/// folders the user added in Settings), resolved, as it is now.
///
/// - A full path with no `.` or `..` step (plain_local).
/// - Its parent is there; resolved (canonicalize) it must be inside an
///   allowed folder, itself resolved, step by step. So a link anywhere
///   above that leads outside is refused.
/// - No link on the way down from the allowed folder: the folder itself and
///   every folder above it that's inside an allowed folder, as written, is
///   looked at without following it.
/// - The folder itself, if it's there, is a folder.
///
/// Blocking: on a blocking thread.
pub(crate) fn pull_target(typed: &str, allowed: &[PathBuf]) -> Result<PathBuf, String> {
    if allowed.is_empty() {
        return Err(no_pull_folder());
    }
    pull_target_at(&plain_local(typed)?, typed, allowed)
}

fn no_pull_folder() -> String {
    format!("The user hasn't allowed any folder on this computer for copies from servers. Ask them to add one in {SETTING}, then copy into it.")
}

/// pull_target's checks on a path already through plain_local, or resolved
/// by it (the re-check before each server's copy, on Windows a `\\?\` path
/// plain_local wouldn't take as typed). `typed` is how it's shown.
fn pull_target_at(path: &Path, typed: &str, allowed: &[PathBuf]) -> Result<PathBuf, String> {
    if allowed.is_empty() {
        return Err(no_pull_folder());
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(format!("{typed} can't be a folder to copy into."));
    };
    let parent = parent.canonicalize().map_err(|_| format!("{} isn't there, so {typed} can't be made in it.", parent.display()))?;
    let target = parent.join(name);
    let resolved: Vec<PathBuf> = allowed.iter().filter_map(|a| a.canonicalize().ok()).collect();
    let refused = || {
        let list: Vec<String> = allowed.iter().map(|a| a.display().to_string()).collect();
        format!("{typed} isn't inside a folder the user allowed for copies from servers ({}). They're set in {SETTING}.", list.join(", "))
    };
    if !resolved.iter().any(|a| inside(&target, a)) {
        return Err(refused());
    }
    // No link on the way down, as written and as resolved.
    for step in path.ancestors().chain(target.ancestors()) {
        let below_allowed = allowed.iter().chain(resolved.iter()).any(|a| inside(step, a));
        if !below_allowed {
            continue;
        }
        match std::fs::symlink_metadata(step) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("{} is a link: Submarine doesn't copy into links on this computer.", step.display()));
            }
            Ok(m) if step == target && !m.is_dir() => return Err(format!("{typed} is there, and isn't a folder.")),
            _ => {}
        }
    }
    Ok(target)
}

/// The folder a server's pulled copy goes in, inside `target`: made if it
/// isn't there (with `target` itself), and checked again as made: a folder,
/// not a link, inside `target`. Blocking: on a blocking thread.
fn server_folder(target: &Path, folder: &str) -> Result<PathBuf, String> {
    if !crate::is_safe_dir_entry_name(folder) {
        return Err(format!("{folder} can't be a folder's name on this computer."));
    }
    if !target.exists() {
        std::fs::create_dir(target).map_err(|e| format!("Couldn't make {}: {e}", target.display()))?;
    }
    let dir = target.join(folder);
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("Couldn't make {}: {e}", dir.display())),
    }
    let meta = std::fs::symlink_metadata(&dir).map_err(|e| format!("Can't read {}: {e}", dir.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("{} isn't a folder (it's a link, or a file): Submarine doesn't copy into it.", dir.display()));
    }
    let resolved = target.canonicalize().map_err(|e| format!("Can't read {}: {e}", target.display()))?;
    if dir.canonicalize().ok().as_deref() != Some(resolved.join(folder).as_path()) {
        return Err(format!("{} moved while it was checked: Submarine doesn't copy into it.", dir.display()));
    }
    Ok(resolved.join(folder))
}

/// A folder the user picked to allow pulls into: a full path in text this
/// computer can read back (UTF-8), there, a folder, and not the system's
/// root or one of its own folders (check_local_path_policy). Resolved.
pub(crate) fn allowable_folder(path: &Path) -> Result<PathBuf, String> {
    if path.to_str().is_none() {
        return Err("Submarine can't keep that folder's name. Pick another.".into());
    }
    let resolved = path.canonicalize().map_err(|e| format!("Can't use {}: {e}", path.display()))?;
    if !resolved.is_dir() {
        return Err(format!("{} isn't a folder.", path.display()));
    }
    let resolved = crate::check_local_path_policy(resolved)?;
    if resolved.to_str().is_none() {
        return Err("Submarine can't keep that folder's name. Pick another.".into());
    }
    Ok(resolved)
}

/// The folders kept in the settings file, as they're used: full paths only,
/// each once, at most MAX_PULL_FOLDERS.
pub(crate) fn kept_folders(saved: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in saved.iter().map(PathBuf::from).filter(|p| p.is_absolute()) {
        if !out.contains(&p) && out.len() < MAX_PULL_FOLDERS {
            out.push(p);
        }
    }
    out
}

// -- Server names as folder names ---------------------------------------------

/// Names Windows keeps for devices, whatever follows the first dot.
fn reserved(stem: &str) -> bool {
    let s = stem.to_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&s.as_str()) {
        return true;
    }
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    (head == "COM" || head == "LPT") && rest.len() == 1 && "0123456789¹²³".contains(rest[0])
}

/// A server's name as one safe folder name on any system (as the bulk
/// dialog makes it, plan.ts safeFolderName): no separators, drive letters,
/// `..`, control or invisible characters, no Windows device name, no dot or
/// space at its end, at most NAME_MAX characters. What can't be kept becomes
/// `_`; nothing left at all is "server".
pub(crate) fn safe_folder_name(name: &str) -> String {
    let invisible = |c: char| c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}' | '\u{2028}' | '\u{2029}');
    let kept: String = name
        .chars()
        .filter(|c| !invisible(*c))
        .map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let spaced = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = spaced.chars().take(NAME_MAX).collect();
    let trimmed = cut.trim_matches(|c| c == ' ' || c == '.');
    if trimmed.is_empty() {
        return "server".into();
    }
    let stem = trimmed.split('.').next().unwrap_or("").trim_end();
    if reserved(stem) {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Each server's folder name: safe, and different from the others' even
/// where upper and lower case are the same name: the second `web` is
/// `web (2)`. In the order given.
pub(crate) fn folder_names(names: &[String]) -> Vec<String> {
    let wanted: Vec<String> = names.iter().map(|n| safe_folder_name(n)).collect();
    let key = |s: &str| s.to_lowercase();
    let as_given: std::collections::HashSet<String> = wanted.iter().map(|w| key(w)).collect();
    let mut taken = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(wanted.len());
    for (i, base) in wanted.iter().enumerate() {
        let first_of_it = wanted.iter().position(|w| key(w) == key(base)) == Some(i);
        let mut name = base.clone();
        let mut n = 2;
        while taken.contains(&key(&name)) || (!first_of_it && as_given.contains(&key(&name))) {
            name = format!("{base} ({n})");
            n += 1;
        }
        taken.insert(key(&name));
        out.push(name);
    }
    out
}

// -- The decision -------------------------------------------------------------

/// Whether a copy on a server whose settings are `policy` runs without
/// asking: only at "Run anything", or with "Allow everything here" on
/// (`allow_all`); never while the settings can't be read or a change waits.
/// There's no command for an allow or always-ask rule to match.
pub(crate) fn decision(policy: &trust::Policy, allow_all: bool) -> trust::Decision {
    if policy.force_ask.is_some() {
        trust::Decision::Ask { rule: None }
    } else if allow_all {
        trust::Decision::Session
    } else if policy.level == trust::RUN_ANYTHING {
        trust::Decision::Anything
    } else {
        trust::Decision::Ask { rule: None }
    }
}

// -- The card -----------------------------------------------------------------

/// What a copy's card shows (PendingView::copy).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct CopyView {
    direction: Direction,
    /// Push: the path on this computer; pull: the path on each server.
    source: String,
    /// Push: the folder on each server; pull: the folder on this computer.
    destination: String,
    /// Push: what's sent, counted on this computer. Pull: None, found when
    /// it runs.
    local: Option<LocalTree>,
    /// Push: files already on the server are replaced.
    replace: bool,
    /// Pull: the folder of its own this server's copy goes in, inside
    /// `destination`.
    into: Option<String>,
}

/// A request checked and ready: what every server's part copies.
pub(crate) struct Plan {
    pub direction: Direction,
    /// Push: the path on this computer; pull: the tidy path on the servers.
    pub source: String,
    /// Push: the tidy folder on the servers; pull: the resolved folder on
    /// this computer.
    pub destination: String,
    pub replace: bool,
    /// Push: what's sent.
    pub local: Option<LocalTree>,
}

impl Plan {
    /// One line for the panel's lists and the audit log.
    fn summary(&self) -> String {
        match self.direction {
            Direction::Push => {
                let what = self.local.map(|t| format!(" ({}, {})", files_text(t.files), size_text(t.bytes))).unwrap_or_default();
                let there = if self.replace { "replaced" } else { "left as they are" };
                format!("copy_files push {}{what} → {} on each server; files already there {there}", self.source, self.destination)
            }
            Direction::Pull => {
                format!("copy_files pull {} from each server → {}, a folder for each server; files already there left as they are", self.source, self.destination)
            }
        }
    }

    fn view(&self, into: Option<String>) -> CopyView {
        CopyView {
            direction: self.direction,
            source: self.source.clone(),
            destination: self.destination.clone(),
            local: self.local,
            replace: self.replace,
            into,
        }
    }
}

pub(crate) fn files_text(n: u64) -> String {
    if n == 1 {
        "1 file".into()
    } else {
        format!("{n} files")
    }
}

pub(crate) fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return if bytes == 1 { "1 byte".into() } else { format!("{bytes} bytes") };
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

// -- One server's copy ----------------------------------------------------------

/// One server's part of a request, for the copier.
pub(crate) struct Job {
    pub session_id: String,
    /// The server's name, as the AI calls it.
    pub server: String,
    /// Pull: the folder of its own its copy goes in (folder_names).
    pub folder: String,
    pub plan: Arc<Plan>,
    /// Pull: the folders the user allows, as they are when it starts.
    pub allowed: Vec<PathBuf>,
}

/// How one server's part ended.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Outcome {
    /// Where it went: the folder on the server, or on this computer.
    pub to: String,
    pub copied: u64,
    pub bytes: u64,
    /// Files left as they were, because they were there already.
    pub skipped: u64,
    /// Nothing was copied because what it would make was there already: what.
    pub left: Option<String>,
    pub notes: Vec<String>,
    pub end: End,
    /// What the server said about a failure (its error, which may name its
    /// files): kept apart from Submarine's words in `end`, so the AI is
    /// given it marked as the server's (boundary.rs).
    pub said: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) enum End {
    #[default]
    Done,
    /// Stopped part way: the AI withdrew it, or access ended.
    Stopped,
    Failed(String),
}

impl Outcome {
    pub(crate) fn failed(why: impl Into<String>) -> Outcome {
        Outcome { end: End::Failed(why.into()), ..Outcome::default() }
    }

    /// Failed: why, in Submarine's words, and what the server said.
    fn failed_saying(why: impl Into<String>, said: impl Into<String>) -> Outcome {
        Outcome { end: End::Failed(why.into()), said: Some(said.into()), ..Outcome::default() }
    }

    /// Why it failed, for you: Submarine's words and the server's.
    fn why_for_you(&self) -> Option<String> {
        match (&self.end, &self.said) {
            (End::Failed(why), Some(said)) => Some(format!("{why} {said}")),
            (End::Failed(why), None) => Some(why.clone()),
            _ => None,
        }
    }

    fn stopped(to: String) -> Outcome {
        Outcome { to, end: End::Stopped, ..Outcome::default() }
    }

    /// The server's part of the AI's answer, `server`'s: what the server
    /// said goes with it, to be marked as the server's when it's given.
    pub(crate) fn answer(&self, server: &str) -> ToolResult {
        let so_far = if self.copied > 0 { format!(" {} ({}) copied before that, to {}.", files_text(self.copied), size_text(self.bytes), self.to) } else { String::new() };
        let mut text = match &self.end {
            End::Failed(why) => {
                let mut r = ToolResult::err(format!("Failed: {why}{so_far}"));
                r.server_text = self.said.as_ref().map(|said| ServerText { server: server.to_string(), text: said.clone() });
                return r;
            }
            End::Stopped => {
                return ToolResult::err(format!("Stopped before it finished: the request was withdrawn, or the server stopped being shared.{so_far}"));
            }
            End::Done if self.copied > 0 => format!("Copied {} ({}) to {}.", files_text(self.copied), size_text(self.bytes), self.to),
            End::Done => match &self.left {
                Some(what) => format!("Nothing copied: {what}"),
                None if self.skipped > 0 => format!("Nothing copied: {} already there, left as they were.", files_text(self.skipped)),
                None => "Nothing copied: there were no files to copy.".to_string(),
            },
        };
        if self.copied > 0 && self.skipped > 0 {
            text.push_str(&format!(" {} already there, left as they were.", files_text(self.skipped)));
        }
        for note in &self.notes {
            text.push_str(&format!(" {note}."));
        }
        ToolResult::ok(text)
    }
}

pub(crate) type CopyFuture = Pin<Box<dyn Future<Output = Outcome> + Send>>;
/// Copies one server's part (`Job`); stops what it can as soon as `stop` is
/// set. The app's (app_copier), or a test's.
pub(crate) type Copier = Arc<dyn Fn(Job, Arc<AtomicBool>) -> CopyFuture + Send + Sync>;

/// The app's copier: its own SFTP transfers, on the server tab's connection.
pub(crate) fn app_copier(app: tauri::AppHandle) -> Copier {
    Arc::new(move |job, stop| {
        let app = app.clone();
        Box::pin(async move { run_job(app, job, stop).await })
    })
}

/// What a transfer command's error means here: Ok with what was left, or
/// how it ended, with what the server said (the error, which may name its
/// files) when it failed.
fn ended_by(err: String, left: impl FnOnce() -> String) -> Result<Option<String>, (End, Option<String>)> {
    if err.starts_with("EXISTS:") {
        Ok(Some(left()))
    } else if err == "cancelled" {
        Err((End::Stopped, None))
    } else {
        Err((End::Failed("The transfer gave an error.".into()), Some(err)))
    }
}

async fn run_job(app: tauri::AppHandle, job: Job, stop: Arc<AtomicBool>) -> Outcome {
    use tauri::Manager;
    let ssh = app.state::<SshState>();
    // The AI's copies run as the login user, as its commands do: never
    // through the root the tab's file panel may have on (sftp_set_elevated).
    if ssh.sftp_elevation.lock().await.contains_key(&job.session_id) {
        return Outcome::failed(format!(
            "{}'s file panel is in root mode in Submarine, and the AI's copies don't run as root. Ask the user to turn root mode off there, or copy as the login user another way.",
            job.server
        ));
    }
    let sftp = match crate::get_sftp_session(&ssh, &job.session_id).await {
        Ok(s) => s,
        Err(e) => return Outcome::failed_saying("Couldn't open the server's files.", e),
    };
    let home = if job.plan.source.starts_with('~') || job.plan.destination.starts_with('~') {
        match sftp.canonicalize(".").await {
            Ok(h) => h,
            Err(e) => return Outcome::failed_saying("Couldn't find the login's home folder.", e.to_string()),
        }
    } else {
        String::new()
    };
    match job.plan.direction {
        Direction::Push => push(&app, &sftp, &job, &home, &stop).await,
        Direction::Pull => pull(&app, &ssh, &sftp, &job, &home, &stop).await,
    }
}

async fn push(app: &tauri::AppHandle, sftp: &russh_sftp::client::SftpSession, job: &Job, home: &str, stop: &AtomicBool) -> Outcome {
    use tauri::Manager;
    let plan = &job.plan;
    let dest = resolve_remote(&plan.destination, home);
    // What the answer names: the path as the AI gave it, never the home
    // folder the server gave for `~`, which is the server's words.
    let shown = &plan.destination;
    let to = format!("{shown} on {}", job.server);
    // What's sent is what the card showed: counted again, as it is now.
    let source = PathBuf::from(&plan.source);
    let now = {
        let source = source.clone();
        tokio::task::spawn_blocking(move || walk_local(&source)).await.unwrap_or_else(|e| Err(e.to_string()))
    };
    let tree = match now {
        Ok(t) if Some(t) == plan.local => t,
        Ok(t) => {
            return Outcome::failed(format!(
                "{} changed since the user allowed it (now {}, {}): ask again.",
                plan.source,
                files_text(t.files),
                size_text(t.bytes)
            ))
        }
        Err(e) => return Outcome::failed(e),
    };
    match sftp.metadata(&dest).await {
        Ok(m) if m.file_type().is_dir() => {}
        Ok(_) => return Outcome::failed(format!("{shown} on {} isn't a folder.", job.server)),
        Err(e) => return Outcome::failed_saying(format!("{shown} isn't there on {}.", job.server), e.to_string()),
    }
    if stop.load(Ordering::Relaxed) {
        return Outcome::stopped(to);
    }
    let Some(name) = source.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
        return Outcome::failed(format!("{} has no name to copy it by.", plan.source));
    };
    let ended = if tree.is_dir {
        crate::sftp_upload_dir(app.clone(), app.state(), job.session_id.clone(), plan.source.clone(), dest.clone(), Some(plan.replace)).await
    } else {
        crate::sftp_upload_file(app.clone(), app.state(), job.session_id.clone(), plan.source.clone(), join_remote(&dest, &name), Some(plan.replace)).await
    };
    let there = join_remote(shown, &name);
    let mut out = Outcome { to, ..Outcome::default() };
    match ended {
        Ok(()) => {
            out.copied = tree.files;
            out.bytes = tree.bytes;
        }
        Err(e) => match ended_by(e, || {
            if tree.is_dir {
                format!("the folder {there} is already on {}, so it was left as it is. Set replace to copy over it.", job.server)
            } else {
                format!("{there} is already on {}, so it was left as it is. Set replace to copy over it.", job.server)
            }
        }) {
            Ok(left) => out.left = left,
            Err((end, said)) => {
                out.end = end;
                out.said = said;
            }
        },
    }
    if out.end == End::Done && tree.links_skipped > 0 {
        out.notes.push(format!("{} left out: links aren't followed on this computer", if tree.links_skipped == 1 { "1 link".to_string() } else { format!("{} links", tree.links_skipped) }));
    }
    out
}

async fn pull(
    app: &tauri::AppHandle,
    ssh: &SshState,
    sftp: &Arc<russh_sftp::client::SftpSession>,
    job: &Job,
    home: &str,
    stop: &Arc<AtomicBool>,
) -> Outcome {
    use tauri::Manager;
    let plan = &job.plan;
    let remote = resolve_remote(&plan.source, home);
    let Some(name) = remote_name(&remote).map(str::to_string) else {
        return Outcome::failed(format!("{} names no file or folder to copy.", plan.source));
    };
    if !crate::is_safe_dir_entry_name(&name) {
        return Outcome::failed(format!("{name} can't be a file's name on this computer."));
    }
    // Where it goes, checked again now, and this server's own folder made.
    let place = {
        let (dest, allowed, folder) = (plan.destination.clone(), job.allowed.clone(), job.folder.clone());
        tokio::task::spawn_blocking(move || pull_target_at(Path::new(&dest), &dest, &allowed).and_then(|t| server_folder(&t, &folder)))
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
    };
    let dir = match place {
        Ok(d) => d,
        Err(e) => return Outcome::failed(e),
    };
    let to = dir.display().to_string();
    let meta = match sftp.metadata(&remote).await {
        Ok(m) => m,
        Err(e) => return Outcome::failed_saying(format!("{} isn't there on {}.", plan.source, job.server), e.to_string()),
    };
    if stop.load(Ordering::Relaxed) {
        return Outcome::stopped(to);
    }
    if meta.file_type().is_dir() {
        return pull_folder(ssh, sftp, job, &remote, &name, &dir, stop).await;
    }
    if !meta.file_type().is_file() {
        return Outcome::failed(format!("{} on {} isn't a file or a folder.", plan.source, job.server));
    }
    let size = meta.size.unwrap_or(0);
    if let Some(why) = over_limits(1, size) {
        return Outcome::failed(why);
    }
    let local = dir.join(&name);
    if !crate::downloads::LoginFolders::new().may_hold_server_file(&local) {
        return Outcome::failed(format!("{} is in a folder this computer runs programs from at login.", local.display()));
    }
    let mut out = Outcome { to, ..Outcome::default() };
    let ended = crate::sftp_download_file(app.clone(), app.state(), job.session_id.clone(), remote, local.to_string_lossy().into_owned(), Some(false)).await;
    match ended {
        Ok(()) => {
            out.copied = 1;
            out.bytes = size;
        }
        Err(e) => match ended_by(e, || format!("{} is already on this computer, and Submarine never replaces a file here.", local.display())) {
            Ok(left) => out.left = left,
            Err((end, said)) => {
                out.end = end;
                out.said = said;
            }
        },
    }
    out
}

/// A folder pulled into `dir`, as sftp_download_dir does it, from its own
/// pieces: so the limits hold on what's fetched (the walk stops past
/// MAX_FILES), and a file already here is left and counted, not the whole
/// folder skipped.
async fn pull_folder(
    ssh: &SshState,
    sftp: &Arc<russh_sftp::client::SftpSession>,
    job: &Job,
    remote: &str,
    name: &str,
    dir: &Path,
    stop: &Arc<AtomicBool>,
) -> Outcome {
    let to = dir.display().to_string();
    if let Err(e) = crate::check_download_dir_target(&to, name) {
        return Outcome::failed(e);
    }
    let local_root = dir.join(name);
    let over_shell = ssh.sftp_shell.lock().await.contains(&job.session_id);
    let mut tree = crate::DownloadTree::default();
    let too_many = AtomicBool::new(false);
    let walked = crate::walk_download_tree(sftp, remote, &local_root, if over_shell { 1 } else { crate::DIR_LISTINGS_AT_ONCE }, stop, &mut tree, |found| {
        if found as u64 > MAX_FILES {
            too_many.store(true, Ordering::Relaxed);
            stop.store(true, Ordering::Relaxed);
        }
    })
    .await;
    match walked {
        Ok(()) => {}
        Err(None) if too_many.load(Ordering::Relaxed) => return Outcome::failed(over_limits(MAX_FILES + 1, 0).unwrap_or_default()),
        Err(None) => return Outcome::stopped(to),
        Err(Some(why)) => return Outcome::failed_saying("Couldn't go through the folder on the server.", why),
    }
    let crate::DownloadTree { mut files, total_bytes, skipped_names } = tree;
    if let Some(why) = over_limits(files.len() as u64, total_bytes) {
        return Outcome::failed(why);
    }
    let login = crate::downloads::LoginFolders::new();
    let before = files.len();
    files.retain(|(_, rel, _)| login.may_hold_server_file(&local_root.join(rel)));
    let autorun = (before - files.len()) as u64;
    let twins = crate::downloads::drop_case_twins(&mut files, cfg!(any(windows, target_os = "macos")));
    if let Err(e) = tokio::fs::create_dir_all(&local_root).await {
        return Outcome::failed(format!("Couldn't make {}: {e}", local_root.display()));
    }
    let transferred = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let skipped = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let at_once = if over_shell { 1 } else { crate::DIR_FILES_AT_ONCE };
    let jobs = files.iter().map(|(remote_file, rel, size)| crate::DirJob {
        alone: *size > crate::DIR_FILE_ALONE_DOWN,
        key: rel.to_lowercase(),
        shown: rel.clone(),
        run: crate::download_dir_file(
            Arc::clone(sftp),
            remote_file.clone(),
            rel.clone(),
            local_root.clone(),
            Arc::clone(&transferred),
            Arc::clone(stop),
            // Never over a file that's here.
            false,
            Arc::clone(&skipped),
        ),
    });
    let ended = crate::run_dir_files(jobs, at_once, stop, || {}).await;
    let skipped = skipped.load(Ordering::Relaxed);
    let mut out = Outcome { to: local_root.display().to_string(), ..Outcome::default() };
    out.bytes = transferred.load(Ordering::Relaxed);
    match ended {
        crate::DirFilesEnd::Done => out.copied = (files.len() as u64).saturating_sub(skipped),
        crate::DirFilesEnd::Stopped(_) => out.end = End::Stopped,
        crate::DirFilesEnd::Failed(why, _) => {
            out.end = End::Failed("A file didn't copy.".into());
            out.said = Some(why);
        }
    }
    out.skipped = skipped;
    out.notes = [crate::skipped_names_note(skipped_names), crate::downloads::autorun_skipped_note(autorun), crate::downloads::case_twins_note(twins)]
        .into_iter()
        .flatten()
        .collect();
    out
}

// -- The request ----------------------------------------------------------------

/// What a server's settings say of a copy, read before its card: the share
/// they were read for (its saved id and "allow everything"), the decision,
/// and why it must ask, if it must.
struct Said {
    looked_up: Option<(Option<i64>, bool)>,
    decision: trust::Decision,
    force_ask: Option<String>,
}

impl AiState {
    pub(crate) fn pull_folders(&self) -> Vec<PathBuf> {
        self.0.pull_folders.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_pull_folders(&self, folders: Vec<PathBuf>) {
        *self.0.pull_folders.lock().unwrap_or_else(|e| e.into_inner()) = folders;
    }

    /// Add one; false if it was there already.
    pub(crate) fn add_pull_folder(&self, folder: PathBuf) -> Result<bool, String> {
        let mut list = self.0.pull_folders.lock().unwrap_or_else(|e| e.into_inner());
        if list.contains(&folder) {
            return Ok(false);
        }
        if list.len() >= MAX_PULL_FOLDERS {
            return Err(format!("At most {MAX_PULL_FOLDERS} folders. Remove one first."));
        }
        list.push(folder);
        Ok(true)
    }

    /// Remove one; false if it wasn't there.
    pub(crate) fn remove_pull_folder(&self, folder: &Path) -> bool {
        let mut list = self.0.pull_folders.lock().unwrap_or_else(|e| e.into_inner());
        let before = list.len();
        list.retain(|p| p != folder);
        before != list.len()
    }

    pub(crate) fn set_copier(&self, copier: Copier) {
        *self.0.copier.lock().unwrap_or_else(|e| e.into_inner()) = Some(copier);
    }

    /// Refused before it reaches you: logged, and the AI told why.
    fn copy_refused(&self, label: &str, summary: &str, why: String) -> ToolResult {
        let mut e = audit::Entry::new("request", label);
        e.request = Some("copy_files");
        e.command = Some(for_log(summary, MAX_LOGGED_COMMAND));
        e.decision = Some("refused");
        e.by = Some("Submarine");
        e.note = Some(why.clone());
        self.audit(e);
        ToolResult::err(why)
    }

    /// `copy_files` (#159): one request, one card, a part per server in the
    /// answer.
    pub(crate) async fn copy_files(&self, tab_id: &str, args: &Value, cancel: watch::Receiver<bool>) -> ToolResult {
        let args = match parse_args(args) {
            Ok(a) => a,
            Err(why) => return ToolResult::err(why),
        };
        let names = match self.named_servers(tab_id, args.servers.clone(), args.all) {
            Ok(n) => n,
            Err(why) => return ToolResult::err(why),
        };
        let asked = format!(
            "copy_files {} {} → {}",
            if args.direction == Direction::Push { "push" } else { "pull" },
            args.source,
            args.destination
        );
        let label = match self.admit_group(tab_id) {
            Ok(label) => label,
            Err((label, why)) => return self.copy_refused(&label, &asked, why),
        };
        // The paths, checked here, on a blocking thread (they touch the disk).
        let checked = {
            let (args_dir, source, destination, allowed) = (args.direction, args.source.clone(), args.destination.clone(), self.pull_folders());
            tokio::task::spawn_blocking(move || -> Result<(String, String, Option<LocalTree>), String> {
                match args_dir {
                    Direction::Push => {
                        let (path, tree) = push_source(&source)?;
                        Ok((path.to_string_lossy().into_owned(), remote_path(&destination)?, Some(tree)))
                    }
                    Direction::Pull => {
                        let source = remote_path(&source)?;
                        if remote_name(&source).is_none() {
                            return Err(format!("{source} names no file or folder to copy."));
                        }
                        Ok((source, pull_target(&destination, &allowed)?.to_string_lossy().into_owned(), None))
                    }
                }
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        };
        let (source, destination, local) = match checked {
            Ok(c) => c,
            Err(why) => return self.copy_refused(&label, &asked, why),
        };
        let plan = Arc::new(Plan { direction: args.direction, source, destination, replace: args.replace, local });
        // Each server's settings, read off the async workers (the vault has
        // a lock of its own): it asks unless every one would run it.
        let looked: Vec<Option<(Option<i64>, bool)>> = {
            let s = self.lock();
            let tab = s.tabs.iter().find(|t| t.id == tab_id);
            names.iter().map(|n| tab.and_then(|t| t.servers.iter().find(|x| &x.name == n)).map(|x| (x.server_id, x.allow_all))).collect()
        };
        let early: Vec<Said> = {
            let lookup = self.0.policy.clone();
            let each = looked.clone();
            tokio::task::spawn_blocking(move || {
                each
                    .into_iter()
                    .map(|l| match l {
                        Some((id, allow_all)) => {
                            let p = lookup(id);
                            Said { looked_up: l, decision: decision(&p, allow_all), force_ask: p.force_ask }
                        }
                        None => Said { looked_up: l, decision: trust::Decision::Ask { rule: None }, force_ask: None },
                    })
                    .collect()
            })
            .await
            .unwrap_or_else(|_| looked.iter().map(|l| Said { looked_up: *l, decision: trust::Decision::Ask { rule: None }, force_ask: Some(trust::UNREADABLE.to_string()) }).collect())
        };
        let ask = early.iter().any(|e| e.decision.approved_by().is_none());
        let group = {
            let mut s = self.lock();
            s.next_group += 1;
            GroupRef { id: s.next_group, size: names.len(), ask }
        };
        let folders = folder_names(&names);
        let slots = Arc::new(Semaphore::new(SERVERS_AT_ONCE));
        let mut parts = tokio::task::JoinSet::new();
        for (i, ((name, folder), early)) in names.iter().zip(folders).zip(early).enumerate() {
            let (me, tab, name, plan, cancel, slots) = (self.clone(), tab_id.to_string(), name.clone(), plan.clone(), cancel.clone(), slots.clone());
            parts.spawn(async move { (i, me.copy_on(&tab, &name, folder, plan, early, group, cancel, slots).await) });
        }
        let mut results: Vec<Option<ToolResult>> = (0..names.len()).map(|_| None).collect();
        while let Some(joined) = parts.join_next().await {
            if let Ok((i, r)) = joined {
                results[i] = Some(r);
            }
        }
        let results: Vec<(String, ToolResult)> =
            names.into_iter().zip(results).map(|(n, r)| (n, r.unwrap_or_else(|| ToolResult::err("It stopped unexpectedly.")))).collect();
        group_answer(&results)
    }

    /// One server's part of a `copy_files` request: its card (with the
    /// group's), the wait for your answer, a turn among SERVERS_AT_ONCE, the
    /// copy, and its lines in the log and the panel.
    #[allow(clippy::too_many_arguments)]
    async fn copy_on(
        &self,
        tab_id: &str,
        name: &str,
        folder: String,
        plan: Arc<Plan>,
        early: Said,
        group: GroupRef,
        mut cancel: watch::Receiver<bool>,
        slots: Arc<Semaphore>,
    ) -> ToolResult {
        let Said { looked_up, decision: early, force_ask } = early;
        let summary = plan.summary();
        let into = (plan.direction == Direction::Pull).then(|| Path::new(&plan.destination).join(&folder).to_string_lossy().into_owned());
        let (label, session_id, user, id, epoch, rx, stop_tx, stop_rx, auto) = {
            let mut s = self.lock();
            let next_id = s.next_id + 1;
            let epoch = s.epoch;
            let Some(tab) = s.tabs.iter().find(|t| t.id == tab_id) else {
                return ToolResult::err("This AI tab's access has ended.");
            };
            let label = tab.label.clone();
            let Some(found) = tab.servers.iter().find(|x| x.name == name).cloned() else {
                drop(s);
                return self.copy_refused(&label, &summary, format!("{name} isn't shared with you any more."));
            };
            // Decided for this same share as it is now, else it asks; and it
            // asks with the others when any of them asks.
            let decision = if looked_up == Some((found.server_id, found.allow_all)) { early } else { trust::Decision::Ask { rule: None } };
            let held_for_group = group.ask && decision.approved_by().is_some();
            let auto = if group.ask { None } else { decision.approved_by() };
            let running_there = s.running.iter().filter(|r| r.tab_id == tab_id && r.session_id == found.session_id).count();
            if running_there >= MAX_RUNNING {
                drop(s);
                return self.copy_refused(&label, &summary, format!("{MAX_RUNNING} of your requests are already running on {name}. Wait for one to finish."));
            }
            s.next_id = next_id;
            let (stop_tx, stop_rx) = watch::channel(false);
            let card = PendingView {
                id: next_id,
                tab_id: tab_id.to_string(),
                tab_label: label.clone(),
                session_id: found.session_id.clone(),
                server: found.name.clone(),
                user: found.user.clone(),
                command: summary.clone(),
                root: false,
                as_root: false,
                root_via: None,
                password: PasswordAsk::None,
                timeout_secs: DEFAULT_TIMEOUT,
                asked_at: now(),
                server_id: found.server_id,
                ask_rule: None,
                can_allow_always: false,
                note: force_ask.or_else(|| held_for_group.then(|| "This server's settings would copy it, but others in this request ask, so it asks with them.".to_string())),
                group: Some(GroupView { id: group.id, size: group.size }),
                copy: Some(plan.view(into.clone())),
            };
            let rx = match &auto {
                Some(by) => {
                    s.running.push(Running {
                        view: RunningView {
                            id: next_id,
                            tab_label: label.clone(),
                            session_id: found.session_id.clone(),
                            server: found.name.clone(),
                            command: summary.clone(),
                            started_at: now(),
                            ran_without_asking: Some(by.clone()),
                            as_root: false,
                            root_via: None,
                        },
                        tab_id: tab_id.to_string(),
                        session_id: found.session_id.clone(),
                        stop: stop_tx.clone(),
                    });
                    None
                }
                None => {
                    let (tx, rx) = oneshot::channel();
                    s.pending.push(Pending { view: card, decide: tx, stop: stop_tx.clone() });
                    Some(rx)
                }
            };
            (label, found.session_id, found.user, next_id, epoch, rx, stop_tx, stop_rx, auto)
        };
        self.changed();
        if rx.is_some() && self.0.flash.load(Ordering::Relaxed) {
            (self.0.attention)();
        }
        let entry_for = |event: &'static str| {
            let mut e = audit::Entry::new(event, &label);
            e.id = Some(id);
            e.request = Some("copy_files");
            e.server = Some(name.to_string());
            e.user = user.clone();
            e.command = Some(for_log(&summary, MAX_LOGGED_COMMAND));
            e
        };
        let done = |outcome: &'static str, text: &str| Done {
            view: DoneView {
                id,
                tab_label: label.clone(),
                server: name.to_string(),
                command: summary.clone(),
                root: false,
                as_root: false,
                root_via: None,
                outcome,
                exit_code: None,
                output_bytes: text.len(),
                hidden: 0,
                cut: false,
                timed_out: false,
                error: None,
                at: now(),
                ran_without_asking: None,
            },
            output: Zeroizing::new(text.to_string()),
            ai_output: Zeroizing::new(text.to_string()),
        };
        let approved_by = match (auto, rx) {
            (Some(by), _) => by,
            (None, None) => return ToolResult::err("It stopped unexpectedly."),
            (None, Some(mut rx)) => {
                let decision: Result<Option<Answer>, ()> = tokio::select! {
                    biased;
                    d = &mut rx => Ok(d.ok()),
                    _ = withdrawal(&mut cancel) => Err(()),
                };
                let decision = match decision {
                    Ok(d) => d,
                    Err(()) => {
                        let was_waiting = {
                            let mut s = self.lock();
                            let before = s.pending.len();
                            s.pending.retain(|p| p.view.id != id);
                            before != s.pending.len()
                        };
                        if was_waiting {
                            let mut e = entry_for("request");
                            e.decision = Some("cancelled");
                            e.note = Some("cancelled by the AI tool".into());
                            self.audit(e);
                            let why = "Cancelled before the user decided: the request was withdrawn.";
                            self.remember(epoch, done("cancelled", why));
                            self.changed();
                            return ToolResult::err(why);
                        }
                        rx.try_recv().ok()
                    }
                };
                match decision {
                    None => {
                        let mut e = entry_for("request");
                        e.decision = Some("cancelled");
                        e.note = Some("access ended".into());
                        self.audit(e);
                        let why = "Cancelled before the user decided: the server isn't shared any more.";
                        self.remember(epoch, done("cancelled", why));
                        self.changed();
                        return ToolResult::err(why);
                    }
                    Some(Answer::Deny(note)) => {
                        let mut e = entry_for("request");
                        e.decision = Some("denied");
                        e.by = Some(BY_YOU);
                        e.note = note.as_ref().map(|n| format!("note: {n}"));
                        self.audit(e);
                        let why = match note {
                            Some(n) => format!("Denied by the user: {n}"),
                            None => "Denied by the user.".to_string(),
                        };
                        self.remember(epoch, done("denied", &why));
                        self.changed();
                        return ToolResult::err(why);
                    }
                    Some(Answer::Allow { by, .. }) => by,
                }
            }
        };
        let ran_without_asking = Some(approved_by.clone()).filter(|b| b != BY_YOU);
        let mut e = entry_for("request");
        e.decision = Some("allowed");
        e.by = Some(if ran_without_asking.is_some() { "your settings" } else { BY_YOU });
        e.approved_by = Some(approved_by);
        self.audit(e);

        // Its turn among the servers copying at once; nothing starts once
        // the AI withdraws the request or access ends.
        let mut stop_wait = stop_rx.clone();
        let flag = Arc::new(AtomicBool::new(false));
        let turn = if *cancel.borrow() || *stop_wait.borrow() {
            None
        } else {
            tokio::select! {
                t = slots.acquire_owned() => t.ok(),
                _ = stop_wait.wait_for(|s| *s) => None,
                _ = withdrawal(&mut cancel) => None,
            }
        };
        let to_ai = |o: &Outcome| o.answer(name);
        let outcome = match turn {
            None => Outcome::stopped(String::new()),
            Some(turn) => {
                let copier = self.0.copier.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let allowed = self.pull_folders();
                match copier {
                    None => Outcome::failed("Copying files isn't available here."),
                    Some(copier) => {
                        let job = Job { session_id: session_id.clone(), server: name.to_string(), folder, plan: plan.clone(), allowed };
                        let mut task = tokio::spawn(copier(job, flag.clone()));
                        let mut told = false;
                        let joined = loop {
                            tokio::select! {
                                biased;
                                r = &mut task => break r,
                                _ = withdrawal(&mut cancel), if !told => {
                                    told = true;
                                    flag.store(true, Ordering::Relaxed);
                                }
                                _ = stop_wait.wait_for(|s| *s), if !told => {
                                    told = true;
                                    flag.store(true, Ordering::Relaxed);
                                }
                            }
                        };
                        drop(turn);
                        joined.unwrap_or_else(|e| Outcome::failed(format!("It stopped unexpectedly: {e}")))
                    }
                }
            }
        };
        drop(stop_tx);
        drop(stop_rx);
        {
            let mut s = self.lock();
            s.running.retain(|r| r.view.id != id);
        }
        let answer = to_ai(&outcome);
        // You see it plain; the AI gets the server's words marked.
        let plain = answer.plain();
        let mut e = entry_for("finished");
        e.note = Some(plain.clone());
        self.audit(e);
        let mut d = done(
            match outcome.end {
                End::Done => "done",
                End::Stopped => "stopped",
                End::Failed(_) => "failed",
            },
            &plain,
        );
        d.view.ran_without_asking = ran_without_asking;
        d.view.error = outcome.why_for_you();
        self.remember(epoch, d);
        self.changed();
        answer
    }
}

#[cfg(test)]
mod tests;

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks("lib.rs", include_str!("../lib.rs"), &[("run", "ai::ai_add_pull_folder", 1), ("run", "ai::ai_remove_pull_folder", 1)]);
    }
}
