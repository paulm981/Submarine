//! Sync through storage the user owns.
//!
//! The cloud sync (cloud.rs) keeps a profile's encrypted records on the
//! original project's server. This keeps the same records somewhere the user
//! controls instead:
//!
//! - a folder that something else copies between computers — OneDrive,
//!   Dropbox, Nextcloud, Syncthing, a network share; or
//! - a git repository (GitHub, GitLab, any git server, a bare repository on
//!   a share), on a branch of its own that is kept to one commit: each sync
//!   replaces that commit, so no history builds up.
//!
//! The layout is the same in both:
//!
//! ```text
//! <folder or repository>/submarine-sync/<profile id>/profile.key
//! <folder or repository>/submarine-sync/<profile id>/<computer id>.subdev
//! ```
//!
//! `profile.key` is the profile's data key sealed under the profile's
//! password — the escrow the cloud keeps too (`build_pw_escrow_record`) — so
//! another computer needs only the folder or repository and the password.
//! Each computer writes only its own `.subdev` file: everything it knows
//! about the profile, encrypted with that key. Names, hosts, logins, even
//! which kinds of items there are and when they changed are inside it;
//! without the password nothing in the store can be read.
//!
//! As no two computers write the same file, a sync tool never has two
//! versions of one file to choose between. A conflict copy it makes anyway
//! (`x.sync-conflict-….subdev`, `x (conflicted copy).subdev`, `x-PC.subdev`)
//! is read like the rest and removed once merged.
//!
//! A sync reads every computer's file, merges them into the vault
//! (`apply_remote_records`: the newest version of each item wins), then
//! writes this computer's file again if what it holds changed.

use crate::{DbState, SyncRecord, SyncReport};
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;
use zeroize::Zeroizing;

const STORE_DIR: &str = "submarine-sync";
const KEY_FILE: &str = "profile.key";
const DEVICE_EXT: &str = ".subdev";
const README: &str = "README.txt";
// A computer's file: magic ‖ format ‖ nonce ‖ AES-256-GCM(key, zstd(JSON)).
const FILE_MAGIC: &[u8; 4] = b"SUBS";
const FILE_FORMAT: u8 = 1;
const DEFAULT_BRANCH: &str = "submarine-sync";

// Where a profile's store is recorded: its vault's sync_meta, so a git
// address that carries a token is as protected as the vault, and none of it
// travels to the other computers.
const META_KIND: &str = "store_kind";
const META_LOCATION: &str = "store_location";
const META_BRANCH: &str = "store_branch";
const META_PROFILE: &str = "store_profile_id";
/// The vault salt `profile.key` was last sealed with from here: a new salt
/// (a new password) means it's sealed again.
const META_KEY_SALT: &str = "store_key_salt";

/// One store operation at a time: a sync and the Profile tab's look at the
/// store share the git working copy.
static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// How long a store operation waits for another Submarine window's.
const STORE_TURN_WAIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// A store operation's turn: this window's (`STORE_LOCK`), and, since each
/// open profile has a window of its own now, the other windows' too. They
/// share the git working copies under `sync-git` (a pull there removes
/// git's lock files and resets the copy) and this computer's own file in a
/// synced folder, so a held lock on `sync-store.lock` in the app's data folder
/// keeps them taking turns. The system lets go of it when a window ends,
/// however it ends.
struct StoreTurn {
    _here: tokio::sync::MutexGuard<'static, ()>,
    _file: Option<std::fs::File>,
}

async fn store_turn(app: &tauri::AppHandle) -> Result<StoreTurn, String> {
    let here = STORE_LOCK.lock().await;
    let Ok(dir) = app.path().app_data_dir() else { return Ok(StoreTurn { _here: here, _file: None }) };
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("sync-store.lock")) else {
        // A folder that can't hold a lock: nothing guards it, nothing stops it.
        return Ok(StoreTurn { _here: here, _file: None });
    };
    let deadline = tokio::time::Instant::now() + STORE_TURN_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(StoreTurn { _here: here, _file: Some(file) }),
            Err(std::fs::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err("[STORE] Another Submarine window has been syncing for a long time. Try again once it's done.".into());
            }
            Err(std::fs::TryLockError::Error(_)) => return Ok(StoreTurn { _here: here, _file: None }),
        }
    }
}

// ── Where ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Folder,
    Git,
}

#[derive(Clone, Debug)]
struct Store {
    kind: Kind,
    location: String,
    branch: String,
}

impl Store {
    fn new(kind: &str, location: &str, branch: Option<&str>) -> Result<Self, String> {
        let location = location.trim().to_string();
        match kind {
            "folder" => {
                if location.is_empty() {
                    return Err("[STORE] Choose a folder.".into());
                }
                if !Path::new(&location).is_absolute() {
                    return Err("[STORE] Give the folder's full path.".into());
                }
                Ok(Store { kind: Kind::Folder, location, branch: String::new() })
            }
            "git" => {
                valid_git_url(&location)?;
                let branch = branch.map(str::trim).filter(|b| !b.is_empty()).unwrap_or(DEFAULT_BRANCH);
                valid_branch(branch)?;
                Ok(Store { kind: Kind::Git, location, branch: branch.to_string() })
            }
            _ => Err("[STORE] Unknown kind of storage.".into()),
        }
    }

    fn kind_str(&self) -> &'static str {
        match self.kind {
            Kind::Folder => "folder",
            Kind::Git => "git",
        }
    }
}

/// An address git will take as a repository, and only as one: never as an
/// option (a leading `-`), never through the `ext::` transport (a command
/// line of its own), nothing that breaks across lines.
fn valid_git_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("[STORE] Enter the repository's address.".into());
    }
    if url.len() > 2048 || url.chars().any(|c| c.is_control()) {
        return Err("[STORE] That isn't a repository address.".into());
    }
    if url.starts_with('-') {
        return Err("[STORE] A repository address can't start with '-'.".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("ext::") || lower.starts_with("fd::") {
        return Err("[STORE] That kind of git address isn't allowed.".into());
    }
    Ok(())
}

fn valid_branch(b: &str) -> Result<(), String> {
    let ok = b.len() <= 100
        && !b.starts_with(['-', '/', '.'])
        && !b.ends_with(['/', '.'])
        && !b.ends_with(".lock")
        && !b.contains("..")
        && !b.contains("//")
        && b.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    if ok { Ok(()) } else { Err("[STORE] That isn't a branch name git takes (letters, digits, - _ . /).".into()) }
}

/// The address to show: whatever is written before the host
/// (`https://user:token@…`, or a token as the user name) stays off the
/// screen.
fn shown_location(store: &Store) -> String {
    if store.kind != Kind::Git {
        return store.location.clone();
    }
    let url = &store.location;
    let Some(scheme_end) = url.find("://") else { return url.clone() };
    let rest = &url[scheme_end + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    match rest[..host_end].rfind('@') {
        Some(at) => format!("{}•••@{}", &url[..scheme_end + 3], &rest[at + 1..]),
        None => url.clone(),
    }
}

/// An http(s) address with a user name and password in it, split into the
/// address without them and the two, decoded: git is given the password
/// through its credential protocol rather than on its command line, where
/// other users of the computer could read it, and it never reaches the
/// user's own credential helper, which would store it. A user name alone
/// (`https://TOKEN@host/…`, as a token is often given) is taken out too, with
/// no password: it may be the secret.
fn split_credentials(url: &str) -> (String, Option<(String, Option<String>)>) {
    let lower = url.to_ascii_lowercase();
    let Some(scheme_end) = url.find("://").filter(|_| lower.starts_with("https://") || lower.starts_with("http://")) else {
        return (url.to_string(), None);
    };
    let rest = &url[scheme_end + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let Some(at) = rest[..host_end].rfind('@') else { return (url.to_string(), None) };
    let bare = format!("{}{}", &url[..scheme_end + 3], &rest[at + 1..]);
    match rest[..at].split_once(':') {
        Some((user, pass)) => (bare, Some((percent_decode(user), Some(percent_decode(pass))))),
        None if at > 0 => (bare, Some((percent_decode(&rest[..at]), None))),
        None => (url.to_string(), None),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A profile's place in a store.
#[derive(Clone, Debug)]
struct Linked {
    store: Store,
    profile_id: String,
}

fn meta_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM sync_meta WHERE key=?1", [key], |r| r.get(0)).ok()
}

fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sync_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [key, value],
    )
    .map(|_| ())
    .map_err(|e| format!("[STORE] META_SET: {e}"))
}

fn linked(conn: &Connection) -> Result<Option<Linked>, String> {
    let (Some(kind), Some(location), Some(profile_id)) =
        (meta_get(conn, META_KIND), meta_get(conn, META_LOCATION), meta_get(conn, META_PROFILE))
    else {
        return Ok(None);
    };
    let branch = meta_get(conn, META_BRANCH);
    let store = Store::new(&kind, &location, branch.as_deref())?;
    if !valid_id(&profile_id) {
        return Err("[STORE] This profile's sync settings are damaged — set it up again.".into());
    }
    Ok(Some(Linked { store, profile_id }))
}

fn link(conn: &Connection, l: &Linked) -> Result<(), String> {
    meta_set(conn, META_KIND, l.store.kind_str())?;
    meta_set(conn, META_LOCATION, &l.store.location)?;
    meta_set(conn, META_BRANCH, &l.store.branch)?;
    meta_set(conn, META_PROFILE, &l.profile_id)?;
    conn.execute("DELETE FROM sync_meta WHERE key=?1", [META_KEY_SALT])
        .map_err(|e| format!("[STORE] META_CLEAR: {e}"))?;
    Ok(())
}

fn unlink(conn: &Connection) -> Result<(), String> {
    for k in [META_KIND, META_LOCATION, META_BRANCH, META_PROFILE, META_KEY_SALT] {
        conn.execute("DELETE FROM sync_meta WHERE key=?1", [k]).map_err(|e| format!("[STORE] META_CLEAR: {e}"))?;
    }
    Ok(())
}

fn valid_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A computer's id as it names its file (`sync_device_node_id`: 16 hex).
fn valid_node(s: &str) -> bool {
    (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The folder the profiles are kept in. Whoever picks `submarine-sync`
/// itself, rather than the folder it's in, gets the same place.
fn data_dir(root: &Path) -> PathBuf {
    if root.file_name().is_some_and(|n| n == STORE_DIR) && !root.join(STORE_DIR).is_dir() {
        root.to_path_buf()
    } else {
        root.join(STORE_DIR)
    }
}

/// A synced folder's root, which has to be there already: a sync tool that
/// isn't running or a network drive that isn't connected must read as that,
/// not as an empty store to start afresh in.
fn folder_root(location: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(location);
    if !p.is_dir() {
        return Err(format!(
            "[STORE] The folder {} isn't there. If it's a network drive or a sync app's folder, check that it's connected.",
            p.display()
        ));
    }
    Ok(p)
}

// ── A computer's file ───────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct DeviceFile {
    /// The profile's name on that computer, for a restore to offer.
    profile: String,
    /// The computer's name, for the Profile tab's list.
    device: String,
    /// When it was written, in milliseconds since 1970.
    written: u64,
    records: Vec<SyncRecord>,
}

fn seal(key: &[u8; 32], f: &DeviceFile) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(f).map_err(|e| format!("[STORE] ENCODE: {e}"))?;
    let packed = crate::vault_compress(&json)?;
    let (ct, nonce) = crate::encrypt_with_key(&packed, key)?;
    let mut out = Vec::with_capacity(FILE_MAGIC.len() + 1 + nonce.len() + ct.len());
    out.extend_from_slice(FILE_MAGIC);
    out.push(FILE_FORMAT);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn unseal(key: &[u8; 32], bytes: &[u8]) -> Result<DeviceFile, String> {
    let head = FILE_MAGIC.len() + 1;
    if bytes.len() < head + crate::NONCE_LEN + 16 || &bytes[..FILE_MAGIC.len()] != FILE_MAGIC {
        return Err("[STORE] NOT_A_SYNC_FILE".into());
    }
    if bytes[FILE_MAGIC.len()] != FILE_FORMAT {
        return Err(format!("[STORE] UNKNOWN_FORMAT {}", bytes[FILE_MAGIC.len()]));
    }
    let nonce = &bytes[head..head + crate::NONCE_LEN];
    let packed = crate::decrypt_with_key(&bytes[head + crate::NONCE_LEN..], nonce, key)?;
    let json = crate::vault_decompress(&packed)?;
    serde_json::from_slice(&json).map_err(|e| format!("[STORE] DECODE: {e}"))
}

type Stamps = BTreeMap<(String, String, bool), String>;

/// What a set of records says about each item, without its contents: the
/// stamp of each (kind, id, deleted).
fn stamps(records: &[SyncRecord]) -> Stamps {
    records
        .iter()
        .map(|r| ((r.entity_type.clone(), r.uuid.clone(), r.deleted), r.updated_at.clone()))
        .collect()
}

/// Write `bytes` to `dir/name` whole or not at all: a sync tool or a crash
/// sees the old file or the new one, never half of one. The temporary file
/// is made new, so a link someone left in its place is replaced, never
/// written through; and the rename replaces a link at `name` rather than
/// following it.
fn write_whole(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("[STORE] Couldn't create {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let dest = dir.join(name);
    let res = (|| {
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        // OneDrive, Dropbox or a virus scanner holding the old file open
        // makes Windows refuse the rename for a moment.
        let mut tries = 0;
        loop {
            match std::fs::rename(&tmp, &dest) {
                Err(e) if tries < 5 && e.kind() == std::io::ErrorKind::PermissionDenied => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(200));
                }
                r => return r,
            }
        }
    })();
    res.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("[STORE] Couldn't write {}: {e}", dest.display())
    })
}

#[derive(serde::Serialize, Clone, Debug)]
pub(crate) struct Device {
    name: String,
    /// When its file last changed, in milliseconds since 1970.
    written: u64,
    this_one: bool,
}

/// What one look at a profile's folder found.
#[derive(Default)]
struct Seen {
    /// The newest record of each item across every file.
    records: Vec<SyncRecord>,
    /// What this computer's own file holds — its profile name, computer name
    /// and stamps — so a sync that changes nothing leaves it alone.
    own: Option<(String, String, Stamps)>,
    /// Conflict copies a sync tool made, read in with the rest; removed once
    /// this computer's file is written.
    copies: Vec<PathBuf>,
    /// Files this profile's key doesn't open.
    unreadable: Vec<String>,
    devices: Vec<Device>,
    /// The profile's name in the most recently written file.
    name: Option<String>,
    has_key: bool,
}

/// Submarine's own folders in a store (`submarine-sync`, a profile's) must
/// be folders, not links: written through, a link would put files wherever
/// whoever planted it chose.
fn refuse_links(profile_dir: &Path) -> Result<(), String> {
    for p in [Some(profile_dir), profile_dir.parent()].into_iter().flatten() {
        if std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("[STORE] {} is a link, not a folder — Submarine won't write through it.", p.display()));
        }
    }
    Ok(())
}

fn is_plain_file(e: &std::fs::DirEntry) -> bool {
    e.file_type().is_ok_and(|t| t.is_file())
}

/// The most one computer's file in the store may be: as much as the cloud
/// sync takes in one answer (cloud::MAX_SYNC_ANSWER).
const MAX_DEVICE_FILE: u64 = crate::cloud::MAX_SYNC_ANSWER as u64;

/// The most a profile's `profile.key` (a sealed key, in hex) may be.
const MAX_KEY_FILE: u64 = 64 * 1024;

/// A computer's file, read no further than `MAX_DEVICE_FILE`.
fn read_device_file(path: &Path) -> Result<Vec<u8>, String> {
    read_capped(path, MAX_DEVICE_FILE)
}

/// A file in the store, read no further than `max` bytes: its size is looked
/// at first, and the read stops one byte past the limit anyway, as the file
/// can grow while it's read.
fn read_capped(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let too_big = || {
        if max >= 1024 * 1024 {
            format!("it's more than the {} MB a sync file can be, so it wasn't read", max / (1024 * 1024))
        } else {
            format!("it's more than the {} KB this file can be, so it wasn't read", max / 1024)
        }
    };
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > max {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err(too_big());
    }
    Ok(bytes)
}

fn read_profile(dir: &Path, key: &[u8; 32], node: &str) -> Result<Seen, String> {
    let mut seen = Seen::default();
    refuse_links(dir)?;
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(seen),
        Err(e) => return Err(format!("[STORE] Couldn't read {}: {e}", dir.display())),
    };
    let mut newest: BTreeMap<(String, String, bool), SyncRecord> = BTreeMap::new();
    let mut newest_written = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == KEY_FILE {
            seen.has_key = true;
            continue;
        }
        if !name.ends_with(DEVICE_EXT) || name.starts_with('.') || !is_plain_file(&entry) {
            continue;
        }
        // Looked at before it's read: a file far bigger than any profile (a
        // sync tool's mistake, or someone who can write to the store) would
        // otherwise be read whole into memory on every sync.
        let file = match read_device_file(&entry.path()).and_then(|b| unseal(key, &b)) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[STORE] skipped {name}: {e}");
                crate::applog::write_line("sync", &format!("skipped {name}: {e}"));
                seen.unreadable.push(name);
                continue;
            }
        };
        let stem = &name[..name.len() - DEVICE_EXT.len()];
        if stem == node || valid_node(stem) {
            seen.devices.push(Device { name: file.device.clone(), written: file.written, this_one: stem == node });
            if stem == node {
                seen.own = Some((file.profile.clone(), file.device.clone(), stamps(&file.records)));
            }
        } else {
            seen.copies.push(entry.path());
        }
        if file.written >= newest_written {
            newest_written = file.written;
            seen.name = Some(file.profile.clone());
        }
        for r in file.records {
            if r.entity_type == crate::ESCROW_ETYPE {
                continue;
            }
            let k = (r.entity_type.clone(), r.uuid.clone(), r.deleted);
            if newest.get(&k).is_none_or(|cur| r.updated_at > cur.updated_at) {
                newest.insert(k, r);
            }
        }
    }
    seen.devices.sort_by_key(|d| std::cmp::Reverse(d.written));
    seen.records = newest.into_values().collect();
    Ok(seen)
}

/// Every item's stamp in the vault, by id, without decrypting or
/// encrypting anything: to tell how many items a merge changed. A deleted
/// item's is its tombstone's.
fn local_stamps(conn: &Connection) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for spec in crate::ENTITIES {
        let mut stmt = conn
            .prepare(&format!("SELECT uuid, updated_at FROM {} WHERE uuid IS NOT NULL", spec.table))
            .map_err(|e| format!("[STORE] STAMPS {}: {e}", spec.table))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
            .map_err(|e| format!("[STORE] STAMPS {}: {e}", spec.table))?;
        for row in rows.flatten() {
            out.insert(row.0, row.1.unwrap_or_default());
        }
    }
    let mut stmt = conn
        .prepare("SELECT uuid, updated_at FROM sync_tombstones")
        .map_err(|e| format!("[STORE] STAMPS tombstones: {e}"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| format!("[STORE] STAMPS tombstones: {e}"))?;
    for row in rows.flatten() {
        out.entry(row.0).or_insert_with(|| format!("deleted {}", row.1));
    }
    Ok(out)
}

/// Before a merge deletes rows here (items deleted on another computer, or
/// by whoever can write to the folder or repository), the profile file is
/// kept as it stands, as the cloud sync does (vault_backup.rs): the file on
/// disk is still the state from before this merge. How many rows it will
/// delete, and the copy's file name when one was made.
fn keep_before_deleting(app: &tauri::AppHandle, profile: &str, conn: &Connection, seen: &Seen) -> (usize, Option<String>) {
    let removed = crate::rows_to_delete(conn, &seen.records);
    if removed == 0 {
        return (0, None);
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    match crate::profile_path(app, profile).and_then(|p| crate::vault_backup::keep(&p, now)) {
        Ok(name) => (removed, name),
        Err(e) => {
            crate::applog::write_line("sync", &format!("no copy of the profile before a merge that deletes: {e}"));
            (removed, None)
        }
    }
}

/// Merge what the store holds into the vault; returns this computer's
/// records after it, and how many items the merge changed here.
fn merge(conn: &Connection, key: &[u8; 32], hlc: &crate::hlc::Hlc, seen: &Seen) -> Result<(Vec<SyncRecord>, usize), String> {
    let before = local_stamps(conn)?;
    if !seen.records.is_empty() {
        crate::apply_remote_records(conn, key, &seen.records, hlc)?;
    }
    let after = local_stamps(conn)?;
    let changed = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).count()
        + before.keys().filter(|k| !after.contains_key(*k)).count();
    Ok((crate::collect_local_records(conn, key, "")?, changed))
}

/// What this computer writes into its profile folder.
struct Mine<'a> {
    node: &'a str,
    device: &'a str,
    profile: &'a str,
    records: &'a [SyncRecord],
    /// `profile.key`'s contents, when it is to be (re)written.
    key_file: Option<String>,
}

/// Write this computer's file if it changed, `profile.key` if asked, then
/// drop the conflict copies that are merged in now. Returns how many items
/// the store didn't have yet.
fn write_profile(dir: &Path, key: &[u8; 32], seen: &Seen, mine: &Mine) -> Result<usize, String> {
    refuse_links(dir)?;
    let ours = stamps(mine.records);
    let unchanged = seen
        .own
        .as_ref()
        .is_some_and(|(profile, device, st)| profile == mine.profile && device == mine.device && *st == ours);
    let mut added = 0;
    if !unchanged {
        let theirs = stamps(&seen.records);
        added = ours.iter().filter(|(k, v)| theirs.get(*k) != Some(*v)).count();
        let file = DeviceFile {
            profile: mine.profile.to_string(),
            device: mine.device.to_string(),
            written: now_ms(),
            records: mine.records.to_vec(),
        };
        write_whole(dir, &format!("{}{DEVICE_EXT}", mine.node), &seal(key, &file)?)?;
    }
    if let Some(k) = &mine.key_file {
        write_whole(dir, KEY_FILE, k.as_bytes())?;
    }
    if let Some(data) = dir.parent() {
        if !data.join(README).exists() {
            let _ = write_whole(data, README, README_TEXT.as_bytes());
        }
    }
    for copy in &seen.copies {
        let _ = std::fs::remove_file(copy);
    }
    Ok(added)
}

const README_TEXT: &str = "Submarine keeps profiles here to sync them between your computers.\r\n\
\r\n\
Everything in these folders is encrypted with the profile's password: without it, none of it can be read.\r\n\
Each computer writes only its own .subdev file. Please don't edit or rename them.\r\n\
\r\n\
To get a profile on another computer: in Submarine's profile list, choose \"From storage\".\r\n";

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// This computer's name, for the other computers' lists. Travels only
/// inside the encrypted file.
fn device_label() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    #[cfg(not(windows))]
    let name = ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .find(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default()
        });
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    if name.is_empty() { "A computer".into() } else { name }
}

// ── Git ─────────────────────────────────────────────────────────────────────

/// A git store's working copy, in the app's data folder: one per repository
/// and branch. It never names the repository as a remote — the address is
/// passed to each command — and a password in the address is taken out of
/// it (`split_credentials`), so neither is written to disk outside the vault.
struct Repo {
    url: String,
    /// User name and password from the address, given to git through an
    /// environment variable its credential helper reads. A user name with no
    /// password (often a token) goes to git as `credential.username`, set
    /// through the environment too.
    login: Option<(String, Option<String>)>,
    branch: String,
    dir: PathBuf,
}

enum Pushed {
    Unchanged,
    Sent,
    /// Another computer pushed since this one fetched.
    Stale,
}

impl Repo {
    fn for_store(app: &tauri::AppHandle, store: &Store) -> Result<Repo, String> {
        use sha2::{Digest, Sha256};
        let base = app.path().app_data_dir().map_err(|e| format!("[STORE] APP_DATA_DIR: {e}"))?;
        let mut h = Sha256::new();
        h.update(store.location.as_bytes());
        h.update(b"\0");
        h.update(store.branch.as_bytes());
        let id = hex::encode(&h.finalize()[..12]);
        let (url, login) = split_credentials(&store.location);
        Ok(Repo { url, login, branch: store.branch.clone(), dir: base.join("sync-git").join(id) })
    }

    async fn git(&self, args: &[&str], secs: u64) -> Result<String, String> {
        self.git_env(args, &[], secs).await
    }

    async fn git_env(&self, args: &[&str], envs: &[(&str, &str)], secs: u64) -> Result<String, String> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.current_dir(&self.dir);
        // Settings of the user's own that would change what's stored or
        // stop a command to ask something: line-ending conversion, hooks,
        // commit signing, background maintenance. And no reflog, whose
        // entries would repeat the command line, address and all.
        let hooks = format!("core.hooksPath={}", self.dir.join(".git").join("no-hooks").display());
        for c in [
            "core.autocrlf=false",
            "core.safecrlf=false",
            "core.fsmonitor=false",
            hooks.as_str(),
            "protocol.ext.allow=never",
            "commit.gpgSign=false",
            "push.gpgSign=false",
            "gc.auto=0",
            "maintenance.auto=false",
            "core.logAllRefUpdates=false",
            // A link in the repository is checked out as a plain file, so
            // nothing written here can be sent somewhere else on disk.
            "core.symlinks=false",
            // The working copy is Submarine's own, wherever %APPDATA% is
            // (a roaming share, a portable stick that records no owner).
            "safe.directory=*",
        ] {
            cmd.arg("-c").arg(c);
        }
        match &self.login {
            Some((user, Some(pass))) => {
                // Only this helper: the user's own would be asked to store the
                // password from the address.
                cmd.arg("-c").arg("credential.helper=");
                cmd.arg("-c").arg(
                    "credential.helper=!f() { printf '%s\\n' \"username=$SUBMARINE_GIT_USER\" \"password=$SUBMARINE_GIT_PASS\"; }; f",
                );
                cmd.env("SUBMARINE_GIT_USER", user).env("SUBMARINE_GIT_PASS", pass);
            }
            Some((user, None)) => {
                // The user name alone, as git would have taken it from the
                // address: the user's own helpers are asked for its password
                // as before, and when none has one, an empty one is sent (a
                // token as user name needs none). Through the environment,
                // after any settings already given there, so it isn't on the
                // command line.
                let n: usize = std::env::var("GIT_CONFIG_COUNT").ok().and_then(|c| c.parse().ok()).unwrap_or(0);
                cmd.env("GIT_CONFIG_COUNT", (n + 1).to_string())
                    .env(format!("GIT_CONFIG_KEY_{n}"), "credential.username")
                    .env(format!("GIT_CONFIG_VALUE_{n}"), user);
                cmd.arg("-c").arg("credential.helper=!f() { printf '%s\\n' password=; }; f");
            }
            None => {}
        }
        cmd.args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_REFLOG_ACTION", "submarine-sync")
            .env("GIT_AUTHOR_NAME", "Submarine")
            .env("GIT_AUTHOR_EMAIL", "submarine@localhost")
            .env("GIT_COMMITTER_NAME", "Submarine")
            .env("GIT_COMMITTER_EMAIL", "submarine@localhost")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        // No terminal to ask in. On Windows git's own credential manager
        // shows its sign-in window instead, which is wanted.
        #[cfg(not(windows))]
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let out = tokio::time::timeout(Duration::from_secs(secs), cmd.output())
            .await
            .map_err(|_| format!("[STORE] git didn't finish within {secs} seconds (git {}).", args[0]))?
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "[STORE] Git isn't installed (or isn't on the PATH). Install it from git-scm.com, or sync through a folder instead.".to_string()
                } else {
                    format!("[STORE] Couldn't run git: {e}")
                }
            })?;
        if !out.status.success() {
            return Err(git_error(&String::from_utf8_lossy(&out.stderr)));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    async fn remote_head(&self) -> Result<Option<String>, String> {
        let want = format!("refs/heads/{}", self.branch);
        let out = self.git(&["ls-remote", "--heads", &self.url, &want], 120).await?;
        Ok(out.lines().find_map(|l| {
            let (sha, name) = l.split_once('\t')?;
            (name.trim() == want).then(|| sha.trim().to_string())
        }))
    }

    /// Bring the working copy to the branch's one commit, or to nothing when
    /// the branch isn't there yet. Returns that commit.
    async fn pull(&self) -> Result<Option<String>, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("[STORE] Couldn't create {}: {e}", self.dir.display()))?;
        if !self.dir.join(".git").is_dir() {
            self.git(&["init", "-q"], 30).await?;
        }
        // A git that was stopped (a timeout, the app quitting) leaves its
        // lock files, and every later command would refuse to run. Only
        // Submarine works in this copy, and not now: they're stale.
        remove_locks(&self.dir.join(".git"));
        // Above any .gitattributes of the user's: store every file as it is.
        let info = self.dir.join(".git").join("info");
        let _ = std::fs::create_dir_all(&info);
        let _ = std::fs::write(info.join("attributes"), "* -text -filter -diff -merge\n");
        let Some(_) = self.remote_head().await? else {
            clear_worktree(&self.dir)?;
            self.git(&["read-tree", "--empty"], 30).await?;
            return Ok(None);
        };
        let spec = format!("+refs/heads/{}:refs/submarine/base", self.branch);
        self.git(&["fetch", "-q", "--no-tags", "--depth=1", &self.url, &spec], 180).await?;
        // It names where the fetch came from; nothing here reads it.
        let _ = std::fs::remove_file(self.dir.join(".git").join("FETCH_HEAD"));
        let base = self.git(&["rev-parse", "--verify", "refs/submarine/base^{commit}"], 30).await?;
        // A branch with history, or with files of its own, isn't one this
        // made: syncing would replace all of it with one commit.
        let commit = self.git(&["cat-file", "-p", &base], 30).await?;
        let has_parent = commit.lines().take_while(|l| !l.is_empty()).any(|l| l.starts_with("parent "));
        let top = self.git(&["ls-tree", "--name-only", &base], 30).await?;
        if has_parent || top.lines().any(|n| n != STORE_DIR) {
            return Err(format!(
                "[STORE] The branch \"{}\" already holds other work, which syncing would replace. Use a branch of its own — the default, {DEFAULT_BRANCH}, is made for this.",
                self.branch
            ));
        }
        self.git(&["reset", "-q", "--hard", &base], 60).await?;
        self.git(&["clean", "-q", "-f", "-d", "-x"], 60).await?;
        Ok(Some(base))
    }

    /// Replace the branch's commit with one holding the working copy as it
    /// is now — no parent, so the repository keeps no history of it.
    async fn push(&self, base: Option<&str>) -> Result<Pushed, String> {
        self.git(&["add", "-A", "--force", "."], 60).await?;
        let tree = self.git(&["write-tree"], 30).await?;
        if let Some(b) = base {
            let had = self.git(&["rev-parse", &format!("{b}^{{tree}}")], 30).await?;
            if had == tree {
                return Ok(Pushed::Unchanged);
            }
        }
        if base.is_none() {
            self.give_empty_repository_a_main().await;
        }
        let commit = self.git(&["commit-tree", "--no-gpg-sign", "-m", "Submarine sync", &tree], 30).await?;
        let lease = format!("--force-with-lease=refs/heads/{}:{}", self.branch, base.unwrap_or(""));
        let target = format!("{commit}:refs/heads/{}", self.branch);
        if let Err(e) = self.git(&["push", "-q", "--no-verify", &lease, &self.url, &target], 180).await {
            // Refused because the branch moved: another computer synced in
            // between. Anything else is the error to show.
            return match self.remote_head().await {
                Ok(now) if now.as_deref() != base => Ok(Pushed::Stale),
                _ => Err(e),
            };
        }
        let _ = self.git(&["update-ref", "refs/submarine/base", &commit], 30).await;
        let _ = self.git(&["reset", "-q", "--soft", &commit], 30).await;
        // The commits this one replaced are of no use here either.
        let _ = self.git(&["reflog", "expire", "--expire=now", "--all"], 30).await;
        let _ = self.git(&["gc", "-q", "--prune=now"], 60).await;
        Ok(Pushed::Sent)
    }

    /// The first branch pushed to an empty repository becomes its default,
    /// and GitLab protects a default branch from the force pushes sync
    /// makes. So an empty repository first gets a `main` holding a short
    /// README. Best effort: without it, sync still works where nothing is
    /// protected.
    async fn give_empty_repository_a_main(&self) {
        if self.branch == "main" {
            return;
        }
        match self.git(&["ls-remote", &self.url], 120).await {
            Ok(refs) if refs.trim().is_empty() => {}
            _ => return,
        }
        let readme = self.dir.join(".git").join("submarine-main-readme");
        if std::fs::write(&readme, MAIN_README.replace("{branch}", &self.branch)).is_err() {
            return;
        }
        let index = self.dir.join(".git").join("submarine-main-index");
        let index_s = index.to_string_lossy().into_owned();
        let readme_s = readme.to_string_lossy().into_owned();
        let made = async {
            let blob = self.git(&["hash-object", "-w", &readme_s], 30).await?;
            let entry = format!("100644,{blob},README.md");
            self.git_env(&["update-index", "--add", "--cacheinfo", &entry], &[("GIT_INDEX_FILE", &index_s)], 30).await?;
            let tree = self.git_env(&["write-tree"], &[("GIT_INDEX_FILE", &index_s)], 30).await?;
            let commit = self.git(&["commit-tree", "--no-gpg-sign", "-m", "About this repository", &tree], 30).await?;
            self.git(&["push", "-q", "--no-verify", &self.url, &format!("{commit}:refs/heads/main")], 180).await
        }
        .await;
        if let Err(e) = made {
            eprintln!("[STORE] couldn't give the empty repository a main branch: {e}");
        }
        let _ = std::fs::remove_file(&readme);
        let _ = std::fs::remove_file(&index);
    }
}

const MAIN_README: &str = "# Submarine sync\n\n\
Submarine keeps encrypted profiles on the `{branch}` branch of this repository, to sync them between your computers.\n\
Everything there is encrypted with each profile's password. That branch is kept to a single commit, replaced at each sync.\n";

/// Remove git's lock files under `dir`.
fn remove_locks(dir: &Path) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() && e.file_name() != "objects" {
                stack.push(e.path());
            } else if t.is_file() && e.file_name().to_string_lossy().ends_with(".lock") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

fn clear_worktree(dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("[STORE] Couldn't read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        if entry.file_name() == ".git" {
            continue;
        }
        let p = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let r = if is_dir { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        r.map_err(|e| format!("[STORE] Couldn't clear {}: {e}", p.display()))?;
    }
    Ok(())
}

/// git's complaint, shorter, with what to do about the usual ones.
fn git_error(stderr: &str) -> String {
    let text = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
        .map(|l| l.trim_start_matches("fatal: ").trim_start_matches("error: "))
        .collect::<Vec<_>>()
        .join(" ");
    let low = text.to_lowercase();
    let hint = if low.contains("could not read username")
        || low.contains("could not read password")
        || low.contains("authentication failed")
        || low.contains("terminal prompts disabled")
    {
        " — git couldn't sign in. Use an SSH address (git@…) with your key loaded, or sign in once from a terminal (git ls-remote <address>) so your credential manager remembers you."
    } else if low.contains("permission denied (publickey") {
        " — the server didn't take your SSH key. Check that it's loaded (ssh-add) and added to your account."
    } else if low.contains("host key verification failed") {
        " — connect once from a terminal (ssh -T git@<server>) to accept the server's host key."
    } else if low.contains("protected branch") || low.contains("force push") || low.contains("non-fast-forward") {
        " — the server won't let this branch's commit be replaced, which each sync does. Allow force pushes on it (GitLab: Settings › Repository › Protected branches), or use a branch of its own."
    } else if low.contains("not found") || low.contains("does not appear to be a git repository") {
        " — check the address, and that your account can reach it."
    } else {
        ""
    };
    let text: String = if text.chars().count() > 400 { text.chars().take(400).chain("…".chars()).collect() } else { text };
    format!("[STORE] git: {}{hint}", if text.is_empty() { "failed" } else { &text })
}

// ── Syncing ─────────────────────────────────────────────────────────────────

/// The open profile, as a sync needs it.
struct Open {
    linked: Linked,
    key: Zeroizing<[u8; 32]>,
    hlc: std::sync::Arc<crate::hlc::Hlc>,
    profile: String,
    /// The DEK sealed under the vault's password, and the salt it's under.
    escrow: Option<(String, String)>,
    key_salt: Option<String>,
    /// Which opening of the profile this is (`DbState::generation`).
    generation: u64,
}

/// The open profile, when it syncs to a store. `at`: only if it's still the
/// one open at that generation.
fn open_profile(db: &DbState, at: Option<u64>) -> Result<Option<Open>, String> {
    let profile = db.active_profile.lock().map_err(|_| "[STATE] LOCK_PROFILE")?.clone().ok_or("[SYNC] NO_PROFILE_OPEN")?;
    let conn_g = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
    let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
    if let Some(generation) = at {
        db.still_open(generation)?;
    }
    let generation = db.generation_now();
    let Some(linked) = linked(conn)? else { return Ok(None) };
    let hlc = db.hlc.lock().map_err(|_| "[STATE] LOCK_HLC")?.as_ref().ok_or("[SYNC] NO_HLC")?.clone();
    let (dek, _) = crate::get_or_create_dek(conn)?;
    let master = db.master_key.lock().map_err(|_| "[STATE] LOCK_KEY")?.as_ref().map(|k| **k);
    let salt = *db.salt.lock().map_err(|_| "[STATE] LOCK_SALT")?;
    let escrow = match (master, salt) {
        (Some(mk), Some(salt)) => {
            let rec = crate::build_pw_escrow_record(&mk, &salt, &dek)?;
            rec.blob.map(|b| (b, hex::encode(salt)))
        }
        _ => None,
    };
    Ok(Some(Open { linked, key: Zeroizing::new(dek), hlc, profile, escrow, key_salt: meta_get(conn, META_KEY_SALT), generation }))
}

/// The profile a sync started on is the one open now. Profiles can be
/// switched while a sync waits on its folder or on git, and one profile's
/// items must never be merged into another's vault.
fn still_open(db: &DbState, conn: &Connection, open: &Open) -> Result<(), String> {
    let name = db.active_profile.lock().map_err(|_| "[STATE] LOCK_PROFILE")?.clone();
    if db.generation_now() != open.generation
        || name.as_deref() != Some(open.profile.as_str())
        || meta_get(conn, META_PROFILE).as_deref() != Some(open.linked.profile_id.as_str())
    {
        return Err("[STORE] The profile was closed during the sync.".into());
    }
    Ok(())
}

struct Exchanged {
    pushed: usize,
    pulled: usize,
    /// Rows the merge deleted here, and the copy of the profile kept first.
    removed: usize,
    backup: Option<String>,
    /// The salt `profile.key` was just sealed with, to record once the key
    /// is really in the store (for git, once it's pushed).
    key_salt: Option<String>,
}

/// Note that the store's `profile.key` is sealed under this vault's salt.
async fn record_key_salt(db: &DbState, open: &Open, salt: &str) -> Result<(), String> {
    {
        let conn_g = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
        let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
        still_open(db, conn, open)?;
        meta_set(conn, META_KEY_SALT, salt)?;
    }
    crate::save_vault_async(db).await
}

/// One read–merge–write against a profile's folder (in a synced folder, or
/// in the git working copy).
async fn exchange(app: &tauri::AppHandle, db: &DbState, dir: PathBuf, open: &Open) -> Result<Exchanged, String> {
    let node = crate::sync_device_node_id(app);
    let key = *open.key;
    let (d, n) = (dir.clone(), node.clone());
    let seen = tokio::task::spawn_blocking(move || read_profile(&d, &key, &n))
        .await
        .map_err(|e| format!("[STORE] JOIN: {e}"))??;

    let (records, pulled, removed, backup) = {
        let conn_g = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
        let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
        still_open(db, conn, open)?;
        let (removed, backup) = keep_before_deleting(app, &open.profile, conn, &seen);
        let (records, pulled) = merge(conn, &key, &open.hlc, &seen)?;
        (records, pulled, removed, backup)
    };

    // Sealed again when it's missing, or when this vault's password (its
    // salt) changed since it last sealed it. Otherwise it's left as another
    // computer last wrote it, so two never take turns rewriting it.
    let key_file = open.escrow.as_ref().and_then(|(blob, salt)| {
        (!seen.has_key || open.key_salt.as_deref() != Some(salt.as_str())).then(|| blob.clone())
    });
    let wrote_key = key_file.is_some();
    let profile = open.profile.clone();
    let pushed = tokio::task::spawn_blocking(move || {
        let device = device_label();
        let mine = Mine { node: &node, device: &device, profile: &profile, records: &records, key_file };
        write_profile(&dir, &key, &seen, &mine)
    })
    .await
    .map_err(|e| format!("[STORE] JOIN: {e}"))??;

    if pulled > 0 {
        crate::save_vault_async(db).await?;
    }
    let key_salt = if wrote_key { open.escrow.as_ref().map(|(_, salt)| salt.clone()) } else { None };
    Ok(Exchanged { pushed, pulled, removed, backup, key_salt })
}

/// Sync the open profile with its store. Runs under `STORE_LOCK`.
async fn sync_open(app: &tauri::AppHandle, db: &DbState, open: &Open) -> Result<SyncReport, String> {
    match open.linked.store.kind {
        Kind::Folder => {
            let data = data_dir(&folder_root(&open.linked.store.location)?);
            let moved = match folder_to_join_now(app, &data, open).await? {
                Some(id) => {
                    leave_folder_now(app, &data, open).await?;
                    Some(relink(db, open, id).await?)
                }
                None => None,
            };
            let open = moved.as_ref().unwrap_or(open);
            let ex = exchange(app, db, data.join(&open.linked.profile_id), open).await?;
            if let Some(salt) = &ex.key_salt {
                record_key_salt(db, open, salt).await?;
            }
            Ok(SyncReport { pushed: ex.pushed, pulled: ex.pulled, key_change: false, removed: ex.removed, backup: ex.backup })
        }
        Kind::Git => {
            let repo = Repo::for_store(app, &open.linked.store)?;
            let data = data_dir(&repo.dir);
            let mut moved: Option<Open> = None;
            let mut pulled = 0;
            let (mut removed, mut backup) = (0, None);
            // A push refused because another computer pushed first starts
            // over from its commit, which this merge then includes.
            for _ in 0..4 {
                let base = repo.pull().await?;
                let open = moved.as_ref().unwrap_or(open);
                if let Some(id) = folder_to_join_now(app, &data, open).await? {
                    // Out of its own folder first, in a commit of its own:
                    // linked to the other one only once that's pushed.
                    leave_folder_now(app, &data, open).await?;
                    if let Pushed::Unchanged | Pushed::Sent = repo.push(base.as_deref()).await? {
                        moved = Some(relink(db, open, id).await?);
                    }
                    continue;
                }
                let id = &open.linked.profile_id;
                let ex = exchange(app, db, data.join(id), open).await?;
                pulled += ex.pulled;
                removed += ex.removed;
                backup = ex.backup.or(backup);
                match repo.push(base.as_deref()).await? {
                    Pushed::Stale => continue,
                    Pushed::Unchanged | Pushed::Sent => {
                        if let Some(salt) = &ex.key_salt {
                            record_key_salt(db, open, salt).await?;
                        }
                        return Ok(SyncReport { pushed: ex.pushed, pulled, key_change: false, removed, backup });
                    }
                }
            }
            Err("[STORE] Other computers kept syncing at the same moment — try again.".into())
        }
    }
}

/// `sync_now` for a profile that syncs through the user's own storage; None
/// for one that doesn't.
pub(crate) async fn sync_if_linked(app: &tauri::AppHandle, db: &DbState) -> Result<Option<SyncReport>, String> {
    let _turn = store_turn(app).await?;
    let Some(open) = open_profile(db, None)? else { return Ok(None) };
    sync_open(app, db, &open).await.map(Some)
}

/// The store's root on disk, fetched first for git.
async fn reach(app: &tauri::AppHandle, store: &Store) -> Result<PathBuf, String> {
    match store.kind {
        Kind::Folder => folder_root(&store.location),
        Kind::Git => {
            let repo = Repo::for_store(app, store)?;
            repo.pull().await?;
            Ok(repo.dir)
        }
    }
}

/// What the store holds for the open profile, for the Profile tab: read
/// only, nothing written. None when the profile doesn't sync to one.
pub(crate) struct Peek {
    pub records: Vec<SyncRecord>,
    pub devices: Vec<Device>,
    pub unreadable: usize,
}

pub(crate) async fn peek(app: &tauri::AppHandle, db: &DbState) -> Result<Option<Peek>, String> {
    let _turn = store_turn(app).await?;
    let Some(open) = open_profile(db, None)? else { return Ok(None) };
    let root = reach(app, &open.linked.store).await?;
    let dir = data_dir(&root).join(&open.linked.profile_id);
    let node = crate::sync_device_node_id(app);
    let key = *open.key;
    let seen = tokio::task::spawn_blocking(move || read_profile(&dir, &key, &node))
        .await
        .map_err(|e| format!("[STORE] JOIN: {e}"))??;
    Ok(Some(Peek { records: seen.records, devices: seen.devices, unreadable: seen.unreadable.len() }))
}

pub(crate) fn is_linked(conn: &Connection) -> bool {
    meta_get(conn, META_PROFILE).is_some()
}

/// The first profile folder in a store (by name) that holds this key's files:
/// a copy of the open profile set up from another computer (the same vault,
/// exported or restored), to join rather than start beside. `before`: only
/// a folder named before that one.
fn find_same_profile(data: &Path, key: &[u8; 32], before: Option<&str>) -> Option<String> {
    for (id, dir) in profile_dirs(data) {
        if before.is_some_and(|b| id.as_str() >= b) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(DEVICE_EXT) && is_plain_file(&entry) && read_device_file(&entry.path()).is_ok_and(|b| unseal(key, &b).is_ok()) {
                return Some(id);
            }
        }
    }
    None
}

/// Two computers holding the same profile that set it up before the folder
/// had brought either one's file to the other each found nothing to join,
/// and started a folder of their own: each would sync only with itself, for
/// good. So while this computer is the only one in its folder, it looks
/// again, and joins the first folder (by name) holding this profile's files.
/// They all pick the same one, and the one already in it stays. None when
/// it stays where it is.
fn folder_to_join(data: &Path, key: &[u8; 32], id: &str, node: &str) -> Result<Option<String>, String> {
    let seen = read_profile(&data.join(id), key, node)?;
    if seen.devices.iter().any(|d| !d.this_one) || !seen.copies.is_empty() {
        return Ok(None);
    }
    Ok(find_same_profile(data, key, Some(id)))
}

async fn folder_to_join_now(app: &tauri::AppHandle, data: &Path, open: &Open) -> Result<Option<String>, String> {
    let (data, key, id, node) = (data.to_path_buf(), *open.key, open.linked.profile_id.clone(), crate::sync_device_node_id(app));
    tokio::task::spawn_blocking(move || folder_to_join(&data, &key, &id, &node)).await.map_err(|e| format!("[STORE] JOIN: {e}"))?
}

/// Take this computer's file out of the profile folder it's leaving, and the
/// folder itself, with its `profile.key`, when no other computer's file is in
/// it: a stale file there would hold the next computer to set up apart.
fn leave_folder(dir: &Path, node: &str) -> Result<(), String> {
    refuse_links(dir)?;
    let own = dir.join(format!("{node}{DEVICE_EXT}"));
    match std::fs::remove_file(&own) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("[STORE] Couldn't remove {}: {e}", own.display()));
        }
        _ => {}
    }
    let others = match std::fs::read_dir(dir) {
        Ok(rd) => rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(DEVICE_EXT)),
        Err(_) => true,
    };
    if !others {
        let _ = std::fs::remove_file(dir.join(KEY_FILE));
        let _ = std::fs::remove_dir(dir);
    }
    Ok(())
}

async fn leave_folder_now(app: &tauri::AppHandle, data: &Path, open: &Open) -> Result<(), String> {
    let (dir, node) = (data.join(&open.linked.profile_id), crate::sync_device_node_id(app));
    tokio::task::spawn_blocking(move || leave_folder(&dir, &node)).await.map_err(|e| format!("[STORE] JOIN: {e}"))?
}

/// Link the open profile to another folder in its store (`folder_to_join`),
/// and the `Open` that syncs with it.
async fn relink(db: &DbState, open: &Open, profile_id: String) -> Result<Open, String> {
    let linked = Linked { store: open.linked.store.clone(), profile_id };
    {
        let conn_g = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
        let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
        still_open(db, conn, open)?;
        link(conn, &linked)?;
    }
    crate::save_vault_async(db).await?;
    crate::applog::write_line("sync", "joined the folder another computer started for this profile");
    Ok(Open {
        linked,
        key: open.key.clone(),
        hlc: open.hlc.clone(),
        profile: open.profile.clone(),
        escrow: open.escrow.clone(),
        // `link` forgot it: the new folder's `profile.key` is another's.
        key_salt: None,
        generation: open.generation,
    })
}

fn profile_dirs(data: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(data) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            valid_id(&name).then(|| (name, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// A profile folder's `profile.key`, and any conflict copies of it.
fn key_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("profile") && n.ends_with(".key") && is_plain_file(e)
        })
        .filter_map(|e| read_capped(&e.path(), MAX_KEY_FILE).ok().and_then(|b| String::from_utf8(b).ok()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The profile's key, from its folder's `profile.key` and the password.
async fn unlock_key(dir: &Path, password: &str) -> Option<[u8; 32]> {
    for sealed in key_files(dir) {
        let pw = Zeroizing::new(password.to_string());
        let opened = tokio::task::spawn_blocking(move || crate::open_pw_escrow(&sealed, &pw)).await.ok()?;
        if let Ok(k) = opened {
            return Some(k);
        }
    }
    None
}

// ── Commands ────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub(crate) struct StoreInfo {
    kind: &'static str,
    /// The folder, or the repository's address (without a token in it).
    location: String,
    branch: String,
}

/// Where the open profile syncs to, if it syncs to the user's own storage.
#[tauri::command]
pub async fn store_status(db_state: tauri::State<'_, DbState>) -> Result<Option<StoreInfo>, String> {
    let conn_g = db_state.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
    let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
    Ok(linked(conn)?.map(|l| StoreInfo { kind: l.store.kind_str(), location: shown_location(&l.store), branch: l.store.branch }))
}

/// Link the profile open at `generation` to `l`, unless another one (or this
/// one again) was opened since; what it was linked to before, for
/// `undo_setup`.
fn link_setup(db: &DbState, generation: u64, l: &Linked) -> Result<Option<Linked>, String> {
    let conn_g = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
    let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
    db.still_open(generation)?;
    let previous = linked(conn).ok().flatten();
    link(conn, l)?;
    Ok(previous)
}

/// Back to how it was after a setup that failed: a store that can't be
/// synced with isn't set up. Only in the profile the setup was for; another
/// one open now keeps its own store.
fn undo_setup(db: &DbState, generation: u64, previous: Option<&Linked>) {
    let Ok(g) = db.conn.lock() else { return };
    let Some(conn) = g.as_ref() else { return };
    if db.still_open(generation).is_err() {
        return;
    }
    let _ = match previous {
        Some(p) => link(conn, p),
        None => unlink(conn),
    };
}

/// Start syncing the open profile through a folder or a git repository, and
/// sync it once. A copy of this profile already there (the same vault from
/// another computer) is joined; otherwise it gets a folder of its own.
#[tauri::command]
pub async fn store_setup(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbState>,
    kind: String,
    location: String,
    branch: Option<String>,
) -> Result<SyncReport, String> {
    let _turn = store_turn(&app).await?;
    let store = Store::new(&kind, &location, branch.as_deref())?;
    // `generation` pins the profile this is for: reaching the store (git may
    // wait on a sign-in) and the first sync take a while, and a profile
    // opened meanwhile must be neither linked nor unlinked.
    let (key, generation) = {
        let conn_g = db_state.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
        let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
        if meta_get(conn, "share_id").is_some() {
            return Err("[STORE] This profile is shared through the cloud, so it can't sync to your own storage too.".into());
        }
        (Zeroizing::new(crate::get_or_create_dek(conn)?.0), db_state.generation_now())
    };
    let root = reach(&app, &store).await?;
    let data = data_dir(&root);
    let k = *key;
    let found = tokio::task::spawn_blocking(move || find_same_profile(&data, &k, None)).await.map_err(|e| format!("[STORE] JOIN: {e}"))?;
    let l = Linked { store, profile_id: found.unwrap_or_else(crate::new_entity_uuid) };
    let previous = link_setup(&db_state, generation, &l)?;
    let res = async {
        let open = open_profile(&db_state, Some(generation))?.ok_or("[STORE] LINK_LOST")?;
        sync_open(&app, &db_state, &open).await
    }
    .await;
    if res.is_err() {
        undo_setup(&db_state, generation, previous.as_ref());
    }
    crate::save_vault_async(&db_state).await?;
    res
}

/// Stop syncing the open profile to its store. What's in the store stays.
#[tauri::command]
pub async fn store_disconnect(db_state: tauri::State<'_, DbState>) -> Result<(), String> {
    let _turn = STORE_LOCK.lock().await;
    {
        let conn_g = db_state.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
        let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
        unlink(conn)?;
    }
    crate::save_vault_async(&db_state).await
}

#[derive(serde::Serialize)]
pub(crate) struct FoundProfile {
    id: String,
    name: String,
    items: usize,
    devices: Vec<Device>,
}

/// The profiles in a store that open with `password`, for a new computer to
/// restore from.
#[tauri::command]
pub async fn store_find_profiles(
    app: tauri::AppHandle,
    kind: String,
    location: String,
    branch: Option<String>,
    password: String,
) -> Result<Vec<FoundProfile>, String> {
    let password = Zeroizing::new(password);
    let _turn = store_turn(&app).await?;
    let store = Store::new(&kind, &location, branch.as_deref())?;
    let data = data_dir(&reach(&app, &store).await?);
    let dirs = profile_dirs(&data);
    if dirs.is_empty() {
        return Err("[STORE] No Submarine profiles there yet. Set up sync on the computer that has the profile first (its Profile tab).".into());
    }
    let mut found = Vec::new();
    for (id, dir) in dirs {
        let Some(key) = unlock_key(&dir, &password).await else { continue };
        let seen = tokio::task::spawn_blocking(move || read_profile(&dir, &key, ""))
            .await
            .map_err(|e| format!("[STORE] JOIN: {e}"))??;
        found.push(FoundProfile {
            name: seen.name.clone().unwrap_or_else(|| "profile".into()),
            items: seen.records.iter().filter(|r| !r.deleted).count(),
            devices: seen.devices,
            id,
        });
    }
    if found.is_empty() {
        return Err("[STORE] None of the profiles there opens with that password.".into());
    }
    Ok(found)
}

/// Make a store's profile a profile on this computer: a new vault under
/// `password`, holding the profile's key, filled by a first sync. Leaves no
/// trace when any step fails.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn store_restore_profile(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbState>,
    kind: String,
    location: String,
    branch: Option<String>,
    profile_id: String,
    local_name: String,
    password: String,
) -> Result<SyncReport, String> {
    let _turn = store_turn(&app).await?;
    let store = Store::new(&kind, &location, branch.as_deref())?;
    crate::validate_profile_name(&local_name)?;
    if password.is_empty() {
        return Err("Password cannot be empty".into());
    }
    if !valid_id(&profile_id) {
        return Err("[STORE] BAD_PROFILE_ID".into());
    }
    if crate::profile_path(&app, &local_name)?.exists() {
        return Err(format!("Profile '{}' already exists", local_name));
    }
    let dir = data_dir(&reach(&app, &store).await?).join(&profile_id);
    let key = Zeroizing::new(unlock_key(&dir, &password).await.ok_or("[STORE] That password doesn't open this profile.")?);

    std::fs::create_dir_all(crate::profiles_dir(&app)?).map_err(|e| format!("[FILE] MKDIR_FAILED: {}", e))?;
    *db_state.active_profile.lock().map_err(|_| "[STATE] LOCK_FAILED")? = Some(local_name.clone());
    // No strength floor: the key file just proved this is the profile's
    // real password.
    let opened = match crate::setup_master_db_with(app.clone(), crate::VaultSecret::Password(password), app.state::<DbState>(), false, true).await {
        Ok(generation) => generation,
        Err(e) => {
            crate::discard_half_built_profile(&app, &local_name, None);
            return Err(e);
        }
    };
    let res = async {
        {
            let conn_g = db_state.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
            let conn = conn_g.as_ref().ok_or("[STATE] DB_NOT_OPEN")?;
            db_state.still_open(opened)?;
            meta_set(conn, "dek", &hex::encode(*key))?;
            link(conn, &Linked { store, profile_id })?;
        }
        let open = open_profile(&db_state, Some(opened))?.ok_or("[STORE] LINK_LOST")?;
        let report = sync_open(&app, &db_state, &open).await?;
        crate::save_vault_async(&db_state).await?;
        Ok::<_, String>(report)
    }
    .await;
    if res.is_err() {
        crate::discard_half_built_profile(&app, &local_name, Some(opened));
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_engine_tests::device;

    const KEY: [u8; 32] = [7u8; 32];

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("submarine-store-{}-{}-{}", name, std::process::id(), crate::new_entity_uuid()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One sync of `conn` (computer `node`) against `dir`, as `exchange`
    /// does it, minus the vault and the app.
    fn sync(conn: &Connection, hlc: &crate::hlc::Hlc, dir: &Path, node: &str) -> (usize, usize) {
        let seen = read_profile(dir, &KEY, node).unwrap();
        let (records, pulled) = merge(conn, &KEY, hlc, &seen).unwrap();
        let mine = Mine { node, device: node, profile: "work", records: &records, key_file: None };
        (write_profile(dir, &KEY, &seen, &mine).unwrap(), pulled)
    }

    fn names(conn: &Connection, table: &str) -> Vec<String> {
        let mut st = conn.prepare(&format!("SELECT name FROM {table} ORDER BY name")).unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
    }

    /// The app's state with `conn` open as the profile `name`.
    fn state(conn: Connection, hlc: std::sync::Arc<crate::hlc::Hlc>, name: &str) -> DbState {
        use std::sync::{atomic::AtomicU64, Arc, Mutex};
        DbState {
            conn: Arc::new(Mutex::new(Some(conn))),
            master_key: Mutex::new(None),
            salt: Mutex::new(None),
            db_path: Mutex::new(None),
            active_profile: Mutex::new(Some(name.into())),
            hlc: Mutex::new(Some(hlc)),
            generation: Arc::new(AtomicU64::new(1)),
            sync_cursor: Mutex::new(None),
            profile_lock: Mutex::new(None),
        }
    }

    /// Open the profile `name` (`conn`) in place of the one open, as opening
    /// one does.
    fn open_another(db: &DbState, conn: Connection, hlc: std::sync::Arc<crate::hlc::Hlc>, name: &str) {
        let mut g = db.conn.lock().unwrap();
        *g = Some(conn);
        *db.active_profile.lock().unwrap() = Some(name.into());
        *db.hlc.lock().unwrap() = Some(hlc);
        db.bump_generation();
    }

    fn linked_now(db: &DbState) -> Option<Linked> {
        linked(db.conn.lock().unwrap().as_ref().unwrap()).unwrap()
    }

    // Setup reaches the store and syncs once before it's done; a profile
    // opened meanwhile is neither linked to the store nor, when the setup
    // gives up, unlinked from its own.
    #[test]
    fn setup_links_only_the_profile_it_was_started_in() {
        let folder = |n: &str| Store::new("folder", &tmp(n).to_string_lossy(), None).unwrap();
        let (a, ha) = device("A");
        let db = state(a, ha, "work");
        let generation = db.generation_now();
        let work = Linked { store: folder("work-store"), profile_id: "a".repeat(32) };

        // Nothing changed: linked, and put back when the setup fails.
        assert!(link_setup(&db, generation, &work).unwrap().is_none());
        assert_eq!(linked_now(&db).unwrap().profile_id, work.profile_id);
        assert!(open_profile(&db, Some(generation)).unwrap().is_some());
        undo_setup(&db, generation, None);
        assert!(linked_now(&db).is_none());

        // "home", which syncs to a store of its own, is opened while the
        // setup of "work" waits on its store.
        let (b, hb) = device("B");
        let home = Linked { store: folder("home-store"), profile_id: "b".repeat(32) };
        link(&b, &home).unwrap();
        open_another(&db, b, hb, "home");
        assert!(link_setup(&db, generation, &work).unwrap_err().contains("PROFILE_CHANGED"));
        assert!(matches!(open_profile(&db, Some(generation)), Err(e) if e.contains("PROFILE_CHANGED")));
        undo_setup(&db, generation, None);
        let now = linked_now(&db).expect("home keeps its store");
        assert_eq!((now.profile_id, now.store.location), (home.profile_id.clone(), home.store.location.clone()));

        // A sync that started before a profile was opened again doesn't
        // write into it, even into the same one.
        let open = open_profile(&db, None).unwrap().unwrap();
        let (c, hc) = device("B");
        link(&c, &home).unwrap();
        open_another(&db, c, hc, "home");
        let g = db.conn.lock().unwrap();
        assert!(still_open(&db, g.as_ref().unwrap(), &open).is_err());
    }

    // Two computers with the same profile set up sync before the folder had
    // brought either one's file to the other: each started a folder of its
    // own. The one alone in the later folder joins the first; the one in the
    // first stays; once together, neither moves.
    #[test]
    fn two_computers_set_up_apart_end_up_in_one_folder() {
        let data = tmp("split").join(STORE_DIR);
        let (first, later) = ("1".repeat(32), "2".repeat(32));
        let (na, nb, nc) = ("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb", "cccccccccccccccc");
        let (a, ha) = device("A");
        let (b, hb) = device("B");
        a.execute("INSERT INTO servers(name,host,port) VALUES('web','h',22)", []).unwrap();
        b.execute("INSERT INTO notes(title,body) VALUES('todo','x')", []).unwrap();
        sync(&a, &ha, &data.join(&first), na);
        sync(&b, &hb, &data.join(&later), nb);

        assert_eq!(folder_to_join(&data, &KEY, &first, na).unwrap(), None, "the first folder's computer stays");
        // A computer that isn't alone in its folder stays with the others.
        let (c, hc) = device("C");
        sync(&c, &hc, &data.join(&later), nc);
        assert_eq!(folder_to_join(&data, &KEY, &later, nb).unwrap(), None);
        std::fs::remove_file(data.join(&later).join(format!("{nc}{DEVICE_EXT}"))).unwrap();

        // Alone, B joins the first folder, and leaves nothing behind.
        assert_eq!(folder_to_join(&data, &KEY, &later, nb).unwrap(), Some(first.clone()));
        leave_folder(&data.join(&later), nb).unwrap();
        assert!(!data.join(&later).exists());
        sync(&b, &hb, &data.join(&first), nb);
        sync(&a, &ha, &data.join(&first), na);
        assert_eq!(names(&a, "servers"), ["web"]);
        let mut st = a.prepare("SELECT title FROM notes").unwrap();
        let notes: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(notes, ["todo"]);
        assert_eq!(names(&b, "servers"), ["web"]);
        assert_eq!(folder_to_join(&data, &KEY, &first, na).unwrap(), None);
        assert_eq!(folder_to_join(&data, &KEY, &first, nb).unwrap(), None);
        // The same computer's file for another profile (another key) in a
        // folder before it is no reason to move.
        let other = data.join("0".repeat(32));
        let f = DeviceFile { profile: "x".into(), device: "X".into(), written: 1, records: vec![] };
        write_whole(&other, &format!("{nb}{DEVICE_EXT}"), &seal(&[9u8; 32], &f).unwrap()).unwrap();
        assert_eq!(folder_to_join(&data, &KEY, &first, nb).unwrap(), None);
    }

    // Leaving a folder another computer is still in takes only this one's file.
    #[test]
    fn leaving_a_folder_keeps_the_others_files() {
        let dir = tmp("leave").join("p");
        write_whole(&dir, "aaaaaaaaaaaaaaaa.subdev", b"a").unwrap();
        write_whole(&dir, "bbbbbbbbbbbbbbbb.subdev", b"b").unwrap();
        write_whole(&dir, KEY_FILE, b"k").unwrap();
        leave_folder(&dir, "aaaaaaaaaaaaaaaa").unwrap();
        assert!(!dir.join("aaaaaaaaaaaaaaaa.subdev").exists());
        assert!(dir.join("bbbbbbbbbbbbbbbb.subdev").exists() && dir.join(KEY_FILE).exists());
    }

    #[test]
    fn files_open_only_with_their_key() {
        let f = DeviceFile { profile: "p".into(), device: "d".into(), written: 5, records: vec![] };
        let sealed = seal(&KEY, &f).unwrap();
        assert_eq!(unseal(&KEY, &sealed).unwrap().device, "d");
        assert!(unseal(&[8u8; 32], &sealed).is_err());
        // Nothing of it readable in the file.
        assert!(!String::from_utf8_lossy(&sealed).contains("device"));
    }

    #[test]
    fn two_computers_meet_through_a_folder() {
        let dir = tmp("meet").join("submarine-sync").join("a".repeat(32));
        let (a, ha) = device("A");
        let (b, hb) = device("B");
        a.execute("INSERT INTO servers(name,host,port) VALUES('web','h',22)", []).unwrap();
        a.execute("INSERT INTO rdp_links(name,host) VALUES('desk','d.example')", []).unwrap();
        assert_eq!(sync(&a, &ha, &dir, "aaaaaaaaaaaaaaaa"), (2, 0));
        // B gets both, and writes its own file with what it now knows.
        let (_, pulled) = sync(&b, &hb, &dir, "bbbbbbbbbbbbbbbb");
        assert_eq!(pulled, 2);
        assert_eq!(names(&b, "servers"), ["web"]);
        assert_eq!(names(&b, "rdp_links"), ["desk"]);
        // B renames the link and deletes the server; A picks both up.
        b.execute("UPDATE rdp_links SET name='desk-2'", []).unwrap();
        b.execute("DELETE FROM servers", []).unwrap();
        sync(&b, &hb, &dir, "bbbbbbbbbbbbbbbb");
        sync(&a, &ha, &dir, "aaaaaaaaaaaaaaaa");
        assert_eq!(names(&a, "rdp_links"), ["desk-2"]);
        assert!(names(&a, "servers").is_empty());
        // Nothing new: neither file is written again.
        let before = std::fs::metadata(dir.join("aaaaaaaaaaaaaaaa.subdev")).unwrap().modified().unwrap();
        assert_eq!(sync(&a, &ha, &dir, "aaaaaaaaaaaaaaaa"), (0, 0));
        assert_eq!(std::fs::metadata(dir.join("aaaaaaaaaaaaaaaa.subdev")).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn conflict_copies_are_merged_then_removed() {
        let dir = tmp("conflict").join("p");
        let (a, ha) = device("A");
        a.execute("INSERT INTO commands(title,content) VALUES('one','x')", []).unwrap();
        sync(&a, &ha, &dir, "aaaaaaaaaaaaaaaa");
        // The sync tool kept an older copy of A's file under another name,
        // and it holds something A's current file doesn't.
        let (c, hc) = device("C");
        c.execute("INSERT INTO commands(title,content) VALUES('two','y')", []).unwrap();
        let recs = crate::collect_local_records(&c, &KEY, "").unwrap();
        let f = DeviceFile { profile: "work".into(), device: "A".into(), written: 1, records: recs };
        let copy = dir.join("aaaaaaaaaaaaaaaa.sync-conflict-20260101-000000-XYZ.subdev");
        std::fs::write(&copy, seal(&KEY, &f).unwrap()).unwrap();
        let _ = hc;
        sync(&a, &ha, &dir, "aaaaaaaaaaaaaaaa");
        let mut st = a.prepare("SELECT title FROM commands ORDER BY title").unwrap();
        let titles: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(titles, ["one", "two"]);
        assert!(!copy.exists(), "a merged conflict copy is removed");
    }

    #[test]
    fn a_file_far_bigger_than_any_profile_is_not_read() {
        let dir = tmp("huge").join("p");
        std::fs::create_dir_all(&dir).unwrap();
        // A sparse file: the size is what's looked at, nothing is written.
        let f = std::fs::File::create(dir.join("dddddddddddddddd.subdev")).unwrap();
        f.set_len(MAX_DEVICE_FILE + 1).unwrap();
        drop(f);
        let seen = read_profile(&dir, &KEY, "aaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(seen.unreadable, ["dddddddddddddddd.subdev"]);
        assert!(seen.records.is_empty());
    }

    #[test]
    fn a_device_file_is_read_up_to_the_limit_and_no_further() {
        let dir = tmp("limit");
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small");
        std::fs::write(&small, b"abc").unwrap();
        assert_eq!(read_device_file(&small).unwrap(), b"abc");
        let at = dir.join("at");
        std::fs::File::create(&at).unwrap().set_len(MAX_DEVICE_FILE).unwrap();
        assert_eq!(read_device_file(&at).unwrap().len() as u64, MAX_DEVICE_FILE);
        let over = dir.join("over");
        std::fs::File::create(&over).unwrap().set_len(MAX_DEVICE_FILE + 1).unwrap();
        assert!(read_device_file(&over).unwrap_err().contains("more than"));
        assert!(read_device_file(&dir.join("missing")).is_err());
    }

    #[test]
    fn key_files_and_the_same_profile_are_looked_for_within_the_limits() {
        let data = tmp("capped");
        let dir = data.join("aaaaaaaaaaaaaaaa");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("profile.key"), "  abcd\n").unwrap();
        std::fs::File::create(dir.join("profile (conflict).key")).unwrap().set_len(MAX_KEY_FILE + 1).unwrap();
        assert_eq!(key_files(&dir), ["abcd"], "the oversized one is skipped, not read");
        std::fs::File::create(dir.join("bbbbbbbbbbbbbbbb.subdev")).unwrap().set_len(MAX_DEVICE_FILE + 1).unwrap();
        assert_eq!(find_same_profile(&data, &KEY, None), None);
    }

    #[test]
    fn a_file_under_another_key_is_left_alone() {
        let dir = tmp("other").join("p");
        std::fs::create_dir_all(&dir).unwrap();
        let f = DeviceFile { profile: "x".into(), device: "X".into(), written: 1, records: vec![] };
        std::fs::write(dir.join("cccccccccccccccc.subdev"), seal(&[9u8; 32], &f).unwrap()).unwrap();
        let (a, ha) = device("A");
        sync(&a, &ha, &dir, "aaaaaaaaaaaaaaaa");
        let seen = read_profile(&dir, &KEY, "aaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(seen.unreadable, ["cccccccccccccccc.subdev"]);
        assert!(dir.join("cccccccccccccccc.subdev").exists());
    }

    #[test]
    fn the_key_file_opens_with_the_password() {
        let dir = tmp("key");
        let salt = [3u8; crate::SALT_LEN];
        let mk = crate::derive_key("pass word", &salt).unwrap();
        let rec = crate::build_pw_escrow_record(&mk, &salt, &KEY).unwrap();
        write_whole(&dir, KEY_FILE, rec.blob.unwrap().as_bytes()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert_eq!(rt.block_on(unlock_key(&dir, "pass word")), Some(KEY));
        assert_eq!(rt.block_on(unlock_key(&dir, "wrong")), None);
    }

    #[test]
    fn picking_the_sync_folder_itself_finds_the_same_place() {
        let root = tmp("root");
        assert_eq!(data_dir(&root), root.join(STORE_DIR));
        let inner = root.join(STORE_DIR);
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(data_dir(&inner), inner);
    }

    #[test]
    fn addresses_and_branches_are_checked() {
        assert!(Store::new("git", "git@github.com:me/sync.git", None).is_ok());
        assert_eq!(Store::new("git", "https://x/y.git", None).unwrap().branch, DEFAULT_BRANCH);
        assert!(Store::new("git", "--upload-pack=evil", None).is_err());
        assert!(Store::new("git", "ext::sh -c touch% /tmp/x", None).is_err());
        assert!(Store::new("git", "https://x/y.git", Some("-x")).is_err());
        assert!(Store::new("git", "https://x/y.git", Some("a..b")).is_err());
        assert!(Store::new("folder", "relative/path", None).is_err());
        let s = Store::new("git", "https://me:tok3n@example.com/r.git", None).unwrap();
        assert_eq!(shown_location(&s), "https://•••@example.com/r.git");
        assert_eq!(
            split_credentials("https://me:p%40ss@example.com/r.git"),
            ("https://example.com/r.git".to_string(), Some(("me".to_string(), Some("p@ss".to_string()))))
        );
        assert_eq!(split_credentials("git@example.com:r.git").1, None);
        // A token given as the user name is taken out of the address too.
        assert_eq!(
            split_credentials("https://ghp_t%2Bk@example.com/r.git"),
            ("https://example.com/r.git".to_string(), Some(("ghp_t+k".to_string(), None)))
        );
        assert_eq!(split_credentials("https://example.com/r.git").1, None);
    }

    fn git_available() -> bool {
        std::process::Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
    }

    // The whole git round: an empty repository gets the branch; a second
    // computer sees it; a push from a stale fetch is refused as stale; the
    // branch stays one commit with no parent.
    #[test]
    fn git_keeps_one_commit_and_refuses_stale_pushes() {
        if !git_available() {
            eprintln!("git not installed; skipped");
            return;
        }
        let base = tmp("git");
        let bare = base.join("remote.git");
        assert!(std::process::Command::new("git").args(["init", "-q", "--bare"]).arg(&bare).status().unwrap().success());
        let url = bare.to_string_lossy().into_owned();
        let repo = |n: &str| Repo { url: url.clone(), login: None, branch: DEFAULT_BRANCH.into(), dir: base.join(n) };
        let (a, mut b) = (repo("a"), repo("b"));
        // A password from the address goes through the inline helper; a
        // local repository never asks for it, but git must take the config.
        b.login = Some(("me".into(), Some("p w'd".into())));
        // And a user name alone through the environment's settings.
        let mut a = a;
        a.login = Some(("tok3n".into(), None));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            assert_eq!(a.pull().await.unwrap(), None);
            write_whole(&data_dir(&a.dir).join("p"), "aaaaaaaaaaaaaaaa.subdev", b"one").unwrap();
            assert!(matches!(a.push(None).await.unwrap(), Pushed::Sent));

            let b_base = b.pull().await.unwrap();
            assert!(b_base.is_some());
            assert_eq!(std::fs::read(data_dir(&b.dir).join("p").join("aaaaaaaaaaaaaaaa.subdev")).unwrap(), b"one");
            assert!(matches!(b.push(b_base.as_deref()).await.unwrap(), Pushed::Unchanged));

            // A pushes again; B's push from what it fetched before is stale.
            let a_base = a.pull().await.unwrap();
            write_whole(&data_dir(&a.dir).join("p"), "aaaaaaaaaaaaaaaa.subdev", b"two").unwrap();
            assert!(matches!(a.push(a_base.as_deref()).await.unwrap(), Pushed::Sent));
            write_whole(&data_dir(&b.dir).join("p"), "bbbbbbbbbbbbbbbb.subdev", b"b").unwrap();
            assert!(matches!(b.push(b_base.as_deref()).await.unwrap(), Pushed::Stale));
            // Starting over from the new commit, B's push goes through and
            // keeps A's file.
            let b_base = b.pull().await.unwrap();
            write_whole(&data_dir(&b.dir).join("p"), "bbbbbbbbbbbbbbbb.subdev", b"b").unwrap();
            assert!(matches!(b.push(b_base.as_deref()).await.unwrap(), Pushed::Sent));
            let a_base = a.pull().await.unwrap();
            assert!(a_base.is_some());
            assert_eq!(std::fs::read(data_dir(&a.dir).join("p").join("aaaaaaaaaaaaaaaa.subdev")).unwrap(), b"two");
            assert_eq!(std::fs::read(data_dir(&a.dir).join("p").join("bbbbbbbbbbbbbbbb.subdev")).unwrap(), b"b");
        });
        // The working copies don't keep the address anywhere.
        for r in [&a, &b] {
            let mut stack = vec![r.dir.join(".git")];
            while let Some(p) = stack.pop() {
                for e in std::fs::read_dir(&p).unwrap().flatten() {
                    if e.path().is_dir() {
                        stack.push(e.path());
                    } else if let Ok(t) = std::fs::read(e.path()) {
                        assert!(!String::from_utf8_lossy(&t).contains("remote.git"), "address in {:?}", e.path());
                    }
                }
            }
        }
        let count = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&bare)
            .args(["rev-list", "--count", DEFAULT_BRANCH])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "1", "the branch is one commit, no history");
        // The empty repository was given a main first, so the sync branch
        // isn't its default.
        let main = std::process::Command::new("git").arg("--git-dir").arg(&bare).args(["ls-tree", "--name-only", "main"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&main.stdout).trim(), "README.md");

        // Pointed at a branch that holds other work (here that main), sync
        // refuses rather than replacing it.
        let c = Repo { url: url.clone(), login: None, branch: "main".into(), dir: base.join("c") };
        let err = rt.block_on(c.pull()).unwrap_err();
        assert!(err.contains("already holds other work"), "{err}");
        let after = std::process::Command::new("git").arg("--git-dir").arg(&bare).args(["ls-tree", "--name-only", "main"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&after.stdout).trim(), "README.md");
    }

    // Links someone planted in a store are never written through.
    #[cfg(unix)]
    #[test]
    fn links_in_the_store_are_not_followed() {
        let base = tmp("links");
        let victim = base.join("victim");
        std::fs::write(&victim, b"precious").unwrap();
        let dir = base.join("submarine-sync").join("p");
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&victim, dir.join(".aaaaaaaaaaaaaaaa.subdev.tmp")).unwrap();
        std::os::unix::fs::symlink(&victim, dir.join(KEY_FILE)).unwrap();
        write_whole(&dir, "aaaaaaaaaaaaaaaa.subdev", b"new").unwrap();
        write_whole(&dir, KEY_FILE, b"key").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
        assert_eq!(std::fs::read(dir.join(KEY_FILE)).unwrap(), b"key");
        // A profile folder that is itself a link is refused.
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let linked = base.join("submarine-sync").join("q");
        std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
        assert!(read_profile(&linked, &KEY, "aaaaaaaaaaaaaaaa").is_err());
        let (a, ha) = device("A");
        let seen = Seen::default();
        let recs = merge(&a, &KEY, &ha, &seen).unwrap().0;
        let mine = Mine { node: "aaaaaaaaaaaaaaaa", device: "A", profile: "p", records: &recs, key_file: None };
        assert!(write_profile(&linked, &KEY, &seen, &mine).is_err());
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    }
}
