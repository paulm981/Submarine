//! Remote Desktop links: saved RDP targets that open the system's own Remote
//! Desktop client. Submarine stores the target, never draws the desktop.
//!
//! - Windows: `mstsc.exe` from System32 (by full path). With no user name it
//!   gets `/v:host:port`, so the user's own Default.rdp settings apply. With
//!   one, it gets a short-lived `.rdp` file: Default.rdp's settings plus the
//!   address and user name.
//! - Elsewhere the `.rdp` file goes to the default app for `.rdp` (Microsoft's
//!   Windows App on macOS, Remmina on many Linux desktops).
//!
//! Passwords are never stored here. Remote Desktop's own "Remember me" keeps
//! them in Windows Credential Manager.
//!
//! Links live in the vault's `rdp_links` table, which is not part of cloud
//! sync (they stay on this device and travel with profile export/import).
//! Its folder_id has no foreign key, unlike servers': sync can delete a
//! folder that only this device has links in, and that must not fail.
//! delete_folder removes the links itself.

use crate::DbState;
use rusqlite::{params, Connection};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rdp_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 3389,
    username TEXT NOT NULL DEFAULT '',
    folder_id INTEGER,
    notes TEXT NOT NULL DEFAULT ''
)";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct RdpLink {
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub notes: String,
}

fn default_port() -> u16 {
    3389
}

/// A host name, IPv4 or IPv6 address: letters, digits and `.-_:`, optionally
/// in brackets. Nothing that could end a `.rdp` line or start an mstsc option.
fn valid_host(host: &str) -> bool {
    let h = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    !h.is_empty()
        && h.len() <= 253
        && !h.starts_with(['-', '/'])
        && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:".contains(c))
}

/// `DOMAIN\user`, `user@domain` or a plain name; no control characters, so
/// nothing can add a line to the `.rdp` file.
fn valid_username(user: &str) -> bool {
    user.len() <= 256 && !user.chars().any(|c| c.is_control())
}

pub(crate) fn validate(link: &RdpLink) -> Result<(), String> {
    if link.name.trim().is_empty() {
        return Err("Give the link a name.".into());
    }
    if !valid_host(link.host.trim()) {
        return Err("The host should be a computer name or an IP address.".into());
    }
    if link.port == 0 {
        return Err("The port should be between 1 and 65535.".into());
    }
    if !valid_username(&link.username) {
        return Err("The user name can't contain line breaks or control characters.".into());
    }
    Ok(())
}

/// `host:port`, bracketing a bare IPv6 address.
fn address(host: &str, port: u16) -> String {
    let host = host.trim();
    let bracketed = if host.contains(':') && !host.starts_with('[') { format!("[{}]", host) } else { host.to_string() };
    if port == 3389 { bracketed } else { format!("{}:{}", bracketed, port) }
}

pub(crate) fn list(conn: &Connection) -> rusqlite::Result<Vec<RdpLink>> {
    let mut stmt = conn.prepare("SELECT id, name, host, port, username, folder_id, notes FROM rdp_links ORDER BY lower(name), id")?;
    let rows = stmt.query_map([], |r| {
        Ok(RdpLink {
            id: Some(r.get(0)?),
            name: r.get(1)?,
            host: r.get(2)?,
            port: r.get::<_, i64>(3)?.clamp(1, 65535) as u16,
            username: r.get(4)?,
            folder_id: r.get(5)?,
            notes: r.get(6)?,
        })
    })?;
    rows.collect()
}

pub(crate) fn save(conn: &Connection, link: &RdpLink) -> Result<i64, String> {
    validate(link)?;
    let db = |e: rusqlite::Error| format!("[DATABASE] RDP_SAVE_FAILED: {}", e);
    let (name, host, user) = (link.name.trim(), link.host.trim(), link.username.trim());
    match link.id {
        Some(id) => {
            conn.execute(
                "UPDATE rdp_links SET name=?1, host=?2, port=?3, username=?4, folder_id=?5, notes=?6 WHERE id=?7",
                params![name, host, link.port, user, link.folder_id, link.notes, id],
            )
            .map_err(db)?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO rdp_links (name, host, port, username, folder_id, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![name, host, link.port, user, link.folder_id, link.notes],
            )
            .map_err(db)?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// The `.rdp` file for a link: the user's Default.rdp settings (if any) with
/// the address and user name replaced. Lines that name another computer or
/// carry a saved password are dropped, not copied to this one; the rest
/// (screen, redirections, an RD Gateway) are the user's preferences.
fn rdp_file(link: &RdpLink, defaults: Option<&str>) -> String {
    const OWN: &[&str] = &["full address:", "alternate full address:", "server port:", "username:", "password 51:"];
    let mut out = String::new();
    for line in defaults.unwrap_or("").lines() {
        let l = line.trim_end_matches('\r');
        let lower = l.to_ascii_lowercase();
        if l.trim().is_empty() || OWN.iter().any(|k| lower.starts_with(k)) {
            continue;
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    out.push_str(&format!("full address:s:{}\r\n", address(&link.host, link.port)));
    let user = link.username.trim();
    if !user.is_empty() {
        out.push_str(&format!("username:s:{}\r\n", user));
    }
    out
}

/// Default.rdp is UTF-16 LE with a byte-order mark, as mstsc writes it; read
/// UTF-8 too.
#[cfg(any(windows, test))]
fn decode_rdp(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return String::from_utf16(&crate::local_open::utf16le_units(rest)).ok();
    }
    String::from_utf8(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes).to_vec()).ok()
}

fn encode_rdp(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

#[tauri::command]
pub async fn rdp_list(state: tauri::State<'_, DbState>) -> Result<Vec<RdpLink>, String> {
    let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
    let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
    list(conn).map_err(|e| format!("[DATABASE] RDP_LIST_FAILED: {}", e))
}

#[tauri::command]
pub async fn rdp_save(state: tauri::State<'_, DbState>, link: RdpLink) -> Result<i64, String> {
    let id = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        save(conn, &link)?
    };
    crate::save_vault_internal(&state)?;
    Ok(id)
}

#[tauri::command]
pub async fn rdp_delete(state: tauri::State<'_, DbState>, id: i64) -> Result<(), String> {
    {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        conn.execute("DELETE FROM rdp_links WHERE id=?1", params![id])
            .map_err(|e| format!("[DATABASE] RDP_DELETE_FAILED: {}", e))?;
    }
    crate::save_vault_internal(&state)?;
    Ok(())
}

/// Open a link in the system's Remote Desktop client.
#[tauri::command]
pub async fn rdp_open(state: tauri::State<'_, DbState>, id: i64) -> Result<(), String> {
    let link = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        list(conn)
            .map_err(|e| format!("[DATABASE] RDP_LIST_FAILED: {}", e))?
            .into_iter()
            .find(|l| l.id == Some(id))
            .ok_or("That Remote Desktop link no longer exists.")?
    };
    // Checked again here: the row may predate a rule, or come from an import.
    validate(&link)?;
    launch(&link)
}

#[cfg(target_os = "android")]
fn launch(_link: &RdpLink) -> Result<(), String> {
    Err("Remote Desktop links open a desktop Remote Desktop client, which Android doesn't have here.".into())
}

#[cfg(not(target_os = "android"))]
fn launch(link: &RdpLink) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mstsc = crate::local_open::system32().join("mstsc.exe");
        if link.username.trim().is_empty() {
            std::process::Command::new(&mstsc)
                .arg(format!("/v:{}", address(&link.host, link.port)))
                .spawn()
                .map_err(|e| format!("Couldn't start Remote Desktop: {}", e))?;
            return Ok(());
        }
        let defaults = directories::UserDirs::new()
            .and_then(|d| d.document_dir().map(|p| p.join("Default.rdp")))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| decode_rdp(&b));
        let file = write_temp_rdp(link, defaults.as_deref())?;
        std::process::Command::new(&mstsc)
            .arg(&file)
            .spawn()
            .map_err(|e| format!("Couldn't start Remote Desktop: {}", e))?;
        remove_later(file);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let file = write_temp_rdp(link, None)?;
        open::that(&file).map_err(|e| format!("Couldn't open a Remote Desktop app: {}", e))?;
        remove_later(file);
        Ok(())
    }
}

#[cfg(not(target_os = "android"))]
fn write_temp_rdp(link: &RdpLink, defaults: Option<&str>) -> Result<std::path::PathBuf, String> {
    use rand::Rng;
    let dir = crate::app_temp_root().join("rdp");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't write the connection file: {}", e))?;
    let mut tag = [0u8; 8];
    rand::rng().fill_bytes(&mut tag);
    let file = dir.join(format!("{}.rdp", hex::encode(tag)));
    std::fs::write(&file, encode_rdp(&rdp_file(link, defaults)))
        .map_err(|e| format!("Couldn't write the connection file: {}", e))?;
    Ok(file)
}

/// The client reads the file as it starts; it isn't needed after that.
#[cfg(not(target_os = "android"))]
fn remove_later(file: std::path::PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let _ = std::fs::remove_file(file);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(host: &str, port: u16, user: &str) -> RdpLink {
        RdpLink { id: None, name: "box".into(), host: host.into(), port, username: user.into(), folder_id: None, notes: String::new() }
    }

    #[test]
    fn hosts_and_users_that_could_inject_are_refused() {
        for bad in ["", "-admin", "/edit", "a b", "host\r\nalternate shell:s:cmd", "x;y", "h\"q"] {
            assert!(validate(&link(bad, 3389, "")).is_err(), "{bad:?}");
        }
        assert!(validate(&link("pc1", 3389, "u\r\nalternate shell:s:cmd")).is_err());
        for good in ["pc1", "pc1.corp.example", "10.0.0.5", "fe80::1", "[fe80::1]", "srv_01"] {
            assert!(validate(&link(good, 3389, r"CORP\me")).is_ok(), "{good}");
        }
        assert!(validate(&link("pc1", 0, "")).is_err());
    }

    #[test]
    fn addresses_carry_the_port_only_when_it_isnt_3389() {
        assert_eq!(address("pc1", 3389), "pc1");
        assert_eq!(address("pc1", 3390), "pc1:3390");
        assert_eq!(address("fe80::1", 3390), "[fe80::1]:3390");
        assert_eq!(address("[fe80::1]", 3389), "[fe80::1]");
    }

    #[test]
    fn the_file_keeps_default_settings_but_not_another_computers() {
        let defaults = "screen mode id:i:2\r\nfull address:s:old-pc\r\nusername:s:someone\r\npassword 51:b:01000000D08C\r\nredirectclipboard:i:1\r\n";
        let f = rdp_file(&link("pc1", 3390, r"CORP\me"), Some(defaults));
        assert_eq!(f, "screen mode id:i:2\r\nredirectclipboard:i:1\r\nfull address:s:pc1:3390\r\nusername:s:CORP\\me\r\n");
        assert_eq!(rdp_file(&link("pc1", 3389, ""), None), "full address:s:pc1\r\n");
    }

    #[test]
    fn rdp_files_round_trip_as_utf16() {
        let text = "full address:s:pc1\r\nusername:s:Jürgen\r\n";
        assert_eq!(decode_rdp(&encode_rdp(text)).as_deref(), Some(text));
        assert_eq!(decode_rdp(b"\xEF\xBB\xBFscreen mode id:i:1").as_deref(), Some("screen mode id:i:1"));
    }

    #[test]
    fn links_save_and_list() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let mut l = link("pc1", 3389, "me");
        let id = save(&conn, &l).unwrap();
        l.id = Some(id);
        l.name = "renamed".into();
        save(&conn, &l).unwrap();
        assert_eq!(list(&conn).unwrap(), vec![l]);
        assert!(save(&conn, &link("bad host", 3389, "")).is_err());
    }
}
