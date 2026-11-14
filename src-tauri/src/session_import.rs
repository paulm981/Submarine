//! Reading other apps' session lists for the import: OpenSSH client
//! config (`parse_ssh_config_text`), PuTTY's `.reg` export
//! (`parse_putty_reg`), MobaXterm's `.mxtsessions` export
//! (`parse_mobaxterm_sessions`) and its path placeholders, FileZilla's site
//! list (`parse_filezilla_sites`, `read_filezilla_sites`), and what each
//! host comes in as (`ImportedHost`).
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use session_import::*`.

use crate::*;

/// The hosts in an OpenSSH client config, one per alias that names a single
/// host (not `*`, `?` or `!`), each with what OpenSSH would use for it:
///
/// - every `Host` block whose patterns match the alias counts, in file order,
///   so a `Host *` (or `Host web*`) block's settings are its defaults;
/// - the first value found for a setting wins, as in OpenSSH, so a block
///   further down can't override one above it;
/// - lines before the first `Host` apply to every host;
/// - a `Match` block's lines are left out, up to the next `Host`: they
///   depend on things only known when connecting.
///
/// Wildcard blocks are never imported as hosts themselves.
#[cfg(not(target_os = "android"))]
pub(crate) fn parse_ssh_config_text(contents: &str) -> Vec<ImportedHost> {
    #[derive(Default, Clone)]
    struct Settings {
        hostname: Option<String>,
        port: Option<u16>,
        user: Option<String>,
        identity_file: Option<String>,
        /// As written: `none` is kept, so it still wins over a later value.
        proxy_jump: Option<String>,
    }
    impl Settings {
        fn fill_from(&mut self, other: &Settings) {
            self.hostname = self.hostname.take().or_else(|| other.hostname.clone());
            self.port = self.port.or(other.port);
            self.user = self.user.take().or_else(|| other.user.clone());
            self.identity_file = self.identity_file.take().or_else(|| other.identity_file.clone());
            self.proxy_jump = self.proxy_jump.take().or_else(|| other.proxy_jump.clone());
        }
    }
    /// One `Host` block (or the lines before the first one, as `*`). A
    /// `Match` block has no patterns, so it applies to nothing.
    struct Block {
        patterns: Vec<String>,
        settings: Settings,
    }
    fn matches(patterns: &[String], alias: &str) -> bool {
        let mut hit = false;
        for p in patterns {
            match p.strip_prefix('!') {
                Some(negated) if ssh_config::glob_matches(negated, alias, true) => return false,
                Some(_) => {}
                None => hit |= ssh_config::glob_matches(p, alias, true),
            }
        }
        hit
    }

    let mut blocks = vec![Block { patterns: vec!["*".into()], settings: Settings::default() }];
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `Directive value` or `Directive=value`.
        let (key, rest) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => (&line[..i], line[i..].trim_start_matches(|c: char| c.is_whitespace() || c == '=')),
            None => (line, ""),
        };
        let key = key.to_ascii_lowercase();
        match key.as_str() {
            "host" => blocks.push(Block { patterns: ssh_config::arguments(rest), settings: Settings::default() }),
            "match" => blocks.push(Block { patterns: Vec::new(), settings: Settings::default() }),
            "hostname" | "port" | "user" | "identityfile" | "proxyjump" => {
                let Some(val) = ssh_config::arguments(rest).into_iter().next().filter(|v| !v.is_empty()) else {
                    continue;
                };
                let s = &mut blocks.last_mut().expect("the lines before any Host").settings;
                match key.as_str() {
                    "hostname" => { s.hostname.get_or_insert(val); }
                    "port" => {
                        if s.port.is_none() {
                            s.port = val.parse::<u16>().ok();
                        }
                    }
                    "user" => { s.user.get_or_insert(val); }
                    "identityfile" => { s.identity_file.get_or_insert(val); }
                    _ => { s.proxy_jump.get_or_insert(val); }
                }
            }
            // Include chains and every other directive are out of scope.
            _ => {}
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<ImportedHost> = Vec::new();
    for block in &blocks[1..] {
        for alias in &block.patterns {
            if alias.contains(['*', '?']) || alias.starts_with('!') || !seen.insert(alias.to_lowercase()) {
                continue;
            }
            let mut s = Settings::default();
            for b in blocks.iter().filter(|b| matches(&b.patterns, alias)) {
                s.fill_from(&b.settings);
            }
            // `%h` in a HostName (often from a wildcard block: `HostName
            // %h.prod.example`) is the alias. A HostName with a token it
            // doesn't take is left out, as is a key path or jump with a token
            // other than `%h` and `%%`, rather than saved with the `%` in it.
            let hostname = s
                .hostname
                .and_then(|h| ssh_config::expand_hostname(&h, alias).ok())
                .filter(|h| !h.trim().is_empty())
                .unwrap_or_else(|| alias.clone());
            let identity_file = s.identity_file.and_then(|f| ssh_config::expand_hostname(&f, &hostname).ok());
            out.push(ImportedHost {
                host_alias: alias.clone(),
                hostname,
                port: s.port.unwrap_or(22),
                user: s.user.unwrap_or_default(),
                identity_file,
                // `none` turns off a ProxyJump set by a wildcard block.
                proxy_jump: s.proxy_jump.filter(|j| !j.eq_ignore_ascii_case("none") && !j.contains('%')),
                ..Default::default()
            });
        }
    }
    resolve_proxy_jump_aliases(&mut out);
    for (i, h) in out.iter_mut().enumerate() {
        h.key = i.to_string();
    }
    out
}

/// OpenSSH looks up each ProxyJump hop in the same config, so `ProxyJump
/// bastion` means the `Host bastion` block's HostName, User and Port. Spell
/// such hops out, so the import can match or save the real host whether or
/// not the bastion block itself is picked.
#[cfg(not(target_os = "android"))]
pub(crate) fn resolve_proxy_jump_aliases(hosts: &mut [ImportedHost]) {
    let by_alias: std::collections::HashMap<String, (String, String, u16)> = hosts
        .iter()
        .map(|h| (h.host_alias.clone(), (h.user.clone(), h.hostname.clone(), h.port)))
        .collect();
    for h in hosts.iter_mut() {
        let Some(spec) = h.proxy_jump.as_deref() else { continue };
        let hops: Vec<String> = spec
            .split(',')
            .map(str::trim)
            .map(|hop| match by_alias.get(hop) {
                Some((user, host, port)) if !hop.contains(['@', ':']) => {
                    let mut out = String::new();
                    if !user.is_empty() {
                        out.push_str(user);
                        out.push('@');
                    }
                    out.push_str(host);
                    if *port != 22 {
                        out.push_str(&format!(":{}", port));
                    }
                    out
                }
                _ => hop.to_string(),
            })
            .collect();
        h.proxy_jump = Some(hops.join(","));
    }
}

/// MobaXterm writes the folders that differ between machines as
/// placeholders. `_CurrentDrive_` is the drive MobaXterm ran from — nearly
/// always C. A key file that isn't there is skipped by the import, not fatal.
pub(crate) fn expand_mobaxterm_path(path: &str, profile_dir: Option<&str>, documents: Option<&str>) -> String {
    let mut out = path.replace("_CurrentDrive_", "C");
    if let Some(home) = profile_dir {
        out = out.replace("_ProfileDir_", home);
    }
    if let Some(docs) = documents {
        out = out.replace("_MyDocuments_", docs);
    }
    out
}

/// The sessions a MobaXterm export holds that can't be imported (not SSH or
/// RDP), as one line for the import dialog: "Not imported: 2 VNC sessions …".
pub(crate) fn mobaxterm_skipped(text: &str) -> Option<String> {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut in_bookmarks = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_bookmarks = line.starts_with("[Bookmarks");
            continue;
        }
        let Some((name, value)) = line.split_once('=') else { continue };
        if !in_bookmarks || matches!(name.trim(), "SubRep" | "ImgNum") {
            continue;
        }
        let Some(settings) = value.split('#').nth(2) else { continue };
        let kind = match settings.split('%').next().unwrap_or("").trim() {
            "0" | "4" => continue,
            "1" => "Telnet",
            "2" => "Rsh",
            "3" => "XDMCP",
            "5" => "VNC",
            "6" => "FTP",
            "7" => "SFTP",
            "8" => "serial",
            "9" => "file",
            "10" => "local shell",
            "11" => "browser",
            "12" => "Mosh",
            "13" => "S3",
            "14" => "WSL",
            _ => "other",
        };
        *counts.entry(kind).or_default() += 1;
    }
    if counts.is_empty() {
        return None;
    }
    let parts: Vec<String> = counts.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
    let total: usize = counts.values().sum();
    let list = match parts.len() {
        1 => parts[0].clone(),
        _ => format!("{} and {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    };
    Some(format!(
        "Not imported: {} {} — Submarine imports SSH and RDP sessions.",
        list,
        if total == 1 { "session" } else { "sessions" }
    ))
}

/// What the pasted-import parser found: the hosts, and a line about the
/// sessions it can't bring in (MobaXterm's VNC, serial, …).
#[derive(serde::Serialize)]
pub(crate) struct ClientImport {
    pub(crate) hosts: Vec<ImportedHost>,
    pub(crate) skipped: Option<String>,
}

/// Save the hosts picked in the import dialog: folders, Logins, jump hosts
/// and servers in one transaction and one vault save (host_import.rs).
#[tauri::command]
pub(crate) async fn import_hosts(
    state: tauri::State<'_, DbState>,
    rows: Vec<host_import::ImportRow>,
) -> Result<host_import::ImportReport, String> {
    let report = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("[DATABASE] IMPORT_FAILED: {}", e))?;
        let report = host_import::import_rows(&tx, &rows)?;
        tx.commit().map_err(|e| format!("[DATABASE] IMPORT_FAILED: {}", e))?;
        report
    };
    save_vault_internal(&state)?;
    Ok(report)
}

/// One resolved entry from an OpenSSH client config `Host` block. The
/// frontend picks a subset of these and turns each into a fresh server row
/// via the existing `add_server` command. A host that named an `IdentityFile`
/// gets that key registered and linked; everything else lands password-less
/// for the user to finish.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub(crate) struct ImportedHost {
    /// The alias the user actually types (`ssh <alias>`) — becomes the
    /// server's display name after import.
    pub(crate) host_alias: String,
    /// Resolved `HostName`, or the alias itself if the block didn't set one
    /// (matches OpenSSH's own behaviour when HostName is omitted).
    pub(crate) hostname: String,
    /// Resolved `Port`, defaulting to the standard SSH port.
    pub(crate) port: u16,
    /// Resolved `User`. Empty string when unset — the frontend can fall
    /// back to whatever it uses elsewhere.
    pub(crate) user: String,
    /// Resolved `IdentityFile`, still in the config's own spelling (`~` and
    /// all). The importer feeds it to `import_ssh_key_file`, which expands and
    /// reads it, so a host that names a key comes in as a key-authenticated
    /// row rather than one the user has to go back and finish.
    pub(crate) identity_file: Option<String>,
    /// Resolved `ProxyJump`: `[user@]host[:port]`, comma-separated hops.
    /// The import links a single hop to a saved server (host_import.rs).
    pub(crate) proxy_jump: Option<String>,
    /// Unique within one parse; the picker keys its rows by it (MobaXterm's
    /// folder and name, else the row's place in the list). Empty means
    /// `host_alias`.
    #[serde(default)]
    pub(crate) key: String,
    /// Folder path, outermost first (MobaXterm's session folders).
    #[serde(default)]
    pub(crate) folder: Vec<String>,
    /// A MobaXterm saved login (`[name]` as the user name): the host signs in
    /// with that Login rather than a user of its own.
    #[serde(default)]
    pub(crate) credential: Option<String>,
    /// Typed into the first terminal on connect (MobaXterm's "Execute command").
    #[serde(default)]
    pub(crate) run_on_connect: Option<String>,
    /// The key the jump host signs in with (MobaXterm's jump host key).
    #[serde(default)]
    pub(crate) jump_identity_file: Option<String>,
    /// The saved login the jump host signs in with (MobaXterm `[name]`).
    #[serde(default)]
    pub(crate) jump_credential: Option<String>,
    /// A Remote Desktop session (MobaXterm's RDP): imported as an RDP link
    /// (rdp_links.rs), not a server.
    #[serde(default)]
    pub(crate) rdp: bool,
    /// A telnet session (PuTTY's): imported as a telnet server.
    #[serde(default)]
    pub(crate) telnet: bool,
    /// An FTP site's kind ("ftps", "ftps-implicit" or "ftp", see ftp.rs);
    /// None for an SSH host. Only FileZilla's site list has FTP sites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) protocol: Option<String>,
    /// A password the source kept for the site (FileZilla keeps them in
    /// base64, which is no protection): it goes into the vault with the
    /// server, after the page asks when the profile is shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) password: Option<String>,
}

pub(crate) fn is_putty_reg(trimmed: &str) -> bool {
    trimmed.to_ascii_lowercase().contains("windows registry editor")
        || trimmed.contains("[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions")
        || trimmed.contains("[HKEY_USERS\\") && trimmed.contains("SimonTatham\\PuTTY\\Sessions")
}

/// Parse `regedit /e` output of PuTTY's session key: the SSH and telnet
/// sessions (`putty_sessions`), or why there are none.
pub(crate) fn parse_putty_reg(text: &str) -> Result<Vec<ImportedHost>, String> {
    let (mut out, skipped) = putty_sessions(text);
    // Names may repeat (two exports pasted together): the picker keys rows
    // by this.
    for (i, h) in out.iter_mut().enumerate() {
        h.key = i.to_string();
    }
    if out.is_empty() {
        return Err(match putty_skipped(&skipped) {
            Some(why) => format!("No PuTTY SSH or telnet sessions found. {}", why),
            None => "No PuTTY sessions found in the pasted text.".into(),
        });
    }
    Ok(out)
}

/// The sessions in a PuTTY `.reg` export, and how many of each other kind
/// (serial, raw, rlogin, …) were left out. The format is deterministic (one
/// `[...]` header line per session, then `"key"=type:val` lines) so a
/// line-oriented walk covers it. We pull the fields that translate to a
/// Submarine row: HostName, PortNumber, UserName and Protocol. An SSH
/// session becomes an SSH server, a telnet one a telnet server. A session
/// with no host name ("Default Settings", say) isn't one to connect to and
/// is left out. Session-name percent-escapes (%20 for space, etc.) are
/// undone so the alias reads naturally.
pub(crate) fn putty_sessions(text: &str) -> (Vec<ImportedHost>, std::collections::BTreeMap<String, usize>) {
    struct Session {
        name: String,
        host: String,
        port: u16,
        user: String,
        /// PuTTY's own word; a session without one is SSH, PuTTY's default.
        protocol: String,
    }
    let mut out: Vec<ImportedHost> = Vec::new();
    let mut skipped: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut current: Option<Session> = None;
    let mut close = |cur: &mut Option<Session>| {
        let Some(s) = cur.take() else { return };
        if s.host.trim().is_empty() {
            return;
        }
        let telnet = match s.protocol.as_str() {
            "" | "ssh" => false,
            "telnet" => true,
            other => {
                *skipped.entry(other.to_string()).or_default() += 1;
                return;
            }
        };
        let usual = if telnet { 23 } else { 22 };
        out.push(ImportedHost {
            host_alias: s.name,
            hostname: s.host.trim().to_string(),
            port: if s.port == 0 { usual } else { s.port },
            user: s.user,
            identity_file: None,
            proxy_jump: None,
            telnet,
            ..Default::default()
        });
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() { continue; }
        if line.starts_with('[') && line.ends_with(']') {
            close(&mut current);
            // Only sessions — skip other PuTTY keys (SshHostKeys, Jumplist).
            let inner = &line[1..line.len() - 1];
            let Some(idx) = inner.rfind("\\Sessions\\") else { continue; };
            let name_raw = &inner[idx + "\\Sessions\\".len()..];
            let name = putty_unescape(name_raw);
            if name.is_empty() { continue; }
            current = Some(Session { name, host: String::new(), port: 0, user: String::new(), protocol: String::new() });
            continue;
        }
        let Some((key, val_raw)) = line.split_once('=') else { continue; };
        let Some(cur) = current.as_mut() else { continue; };
        let key = key.trim().trim_matches('"');
        let val = val_raw.trim();
        match key {
            "HostName" => {
                if let Some(v) = strip_reg_string(val) { cur.host = v; }
            }
            "PortNumber" => {
                if let Some(n) = strip_reg_dword(val) {
                    if let Ok(p) = u16::try_from(n) { cur.port = p; }
                }
            }
            "UserName" => {
                if let Some(v) = strip_reg_string(val) { cur.user = v; }
            }
            "Protocol" => {
                if let Some(v) = strip_reg_string(val) { cur.protocol = v.trim().to_ascii_lowercase(); }
            }
            _ => {}
        }
    }
    close(&mut current);
    (out, skipped)
}

/// "Not imported: 2 serial and 1 raw sessions — …" for the PuTTY sessions
/// `putty_sessions` left out; None when it left out none.
pub(crate) fn putty_skipped(counts: &std::collections::BTreeMap<String, usize>) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let parts: Vec<String> = counts.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
    let total: usize = counts.values().sum();
    let list = match parts.len() {
        1 => parts[0].clone(),
        _ => format!("{} and {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    };
    Some(format!(
        "Not imported: {} {} — Submarine imports PuTTY's SSH and telnet sessions.",
        list,
        if total == 1 { "session" } else { "sessions" }
    ))
}

/// Parse a MobaXterm `.mxtsessions` export. It's an INI file: each section
/// (`[Bookmarks]`, `[Bookmarks_1]`, …) is a folder, with `SubRep=` its path
/// (`\`-separated) and `ImgNum=` its icon, and every other line is one
/// session:
///
/// `<name>= #<icon>#<settings>#<terminal settings>#…`
///
/// `<settings>` is `%`-separated and starts with the session type (0 = SSH,
/// 4 = RDP, 5 = VNC, 7 = SFTP, …). For SSH, field 1 is the host, 2 the port,
/// 3 the user (`<default>` = none set, `[name]` = the saved login `name`),
/// 7 the command run after login, 8/9/10 the jump hosts' names, ports and
/// users (`__PIPE__` between hops), 14 the key file and 15 the jump host's
/// key file. Paths use MobaXterm's placeholders (see
/// `expand_mobaxterm_path`). The icon is the user's pick, so it says nothing
/// about the type. SSH and RDP sessions are imported.
pub(crate) fn parse_mobaxterm_sessions(text: &str) -> Result<Vec<ImportedHost>, String> {
    let dirs = directories::UserDirs::new();
    let home = dirs.as_ref().map(|d| d.home_dir().to_string_lossy().into_owned());
    let docs = dirs.as_ref().and_then(|d| d.document_dir()).map(|p| p.to_string_lossy().into_owned());
    parse_mobaxterm_sessions_in(text, home.as_deref(), docs.as_deref())
}

pub(crate) fn parse_mobaxterm_sessions_in(text: &str, profile_dir: Option<&str>, documents: Option<&str>) -> Result<Vec<ImportedHost>, String> {
    const SSH: &str = "0";
    const RDP: &str = "4";
    let mut out: Vec<ImportedHost> = Vec::new();
    // Session names repeat, within a folder too; the import list keys rows by
    // folder and name.
    let mut names: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    // The next number to try for a name already taken in a folder.
    let mut next_number: std::collections::HashMap<(String, String), u32> = std::collections::HashMap::new();
    let mut in_bookmarks = false;
    let mut folder: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') { continue; }
        if line.starts_with('[') {
            in_bookmarks = line.starts_with("[Bookmarks");
            folder.clear();
            continue;
        }
        if !in_bookmarks { continue; }
        let Some((name, value)) = line.split_once('=') else { continue; };
        let name = name.trim();
        if name == "SubRep" {
            folder = value.split('\\').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            continue;
        }
        if name == "ImgNum" { continue; }
        let mut blocks = value.trim().split('#');
        let (Some(_reconnect), Some(_icon), Some(settings)) = (blocks.next(), blocks.next(), blocks.next()) else {
            continue;
        };
        let fields: Vec<&str> = settings.split('%').map(str::trim).collect();
        let field = |i: usize| fields.get(i).copied().unwrap_or("");
        let rdp = match field(0) {
            SSH => false,
            RDP => true,
            _ => continue,
        };
        let hostname = field(1);
        if hostname.is_empty() { continue; }
        let port = field(2).parse::<u16>().ok().filter(|p| *p != 0).unwrap_or(if rdp { 3389 } else { 22 });
        // `[name]`: sign in with MobaXterm's saved login `name`.
        let (user, credential) = match field(3) {
            "<default>" => ("", None),
            u => match u.strip_prefix('[').and_then(|u| u.strip_suffix(']')) {
                Some(login) if !login.trim().is_empty() => ("", Some(login.trim().to_string())),
                _ => (u, None),
            },
        };
        let path = |i: usize| {
            Some(field(i))
                .filter(|p| !p.is_empty())
                .map(|p| expand_mobaxterm_path(p, profile_dir, documents))
        };
        // RDP sessions keep only host, port, user and folder: their other
        // fields mean something else.
        let proxy_jump = (!rdp).then(|| mobaxterm_jump_hosts(field(8), field(9), field(10))).flatten();
        let jump_identity_file = proxy_jump.as_ref().and_then(|_| path(15));
        let jump_credential = proxy_jump.as_ref().and_then(|_| mobaxterm_jump_login(field(8), field(10)));
        let folder_key = folder.join("\\");
        let mut alias = name.to_string();
        if !names.insert((folder_key.clone(), alias.clone())) {
            // Go on from where this name left off in this folder, not from 2
            // each time: a list of N sessions with one name must not cost N²
            // tries (an export of 20,000 of them froze the window for a
            // minute).
            let next = next_number.entry((folder_key.clone(), name.to_string())).or_insert(2);
            loop {
                alias = format!("{} ({})", name, next);
                *next += 1;
                if names.insert((folder_key.clone(), alias.clone())) {
                    break;
                }
            }
        }
        out.push(ImportedHost {
            key: format!("{}\\{}", folder_key, alias),
            host_alias: alias,
            hostname: hostname.to_string(),
            port,
            user: user.to_string(),
            identity_file: (!rdp).then(|| path(14)).flatten(),
            proxy_jump,
            folder: folder.clone(),
            credential,
            run_on_connect: (!rdp).then(|| Some(field(7)).filter(|c| !c.is_empty()).map(mobaxterm_unescape)).flatten(),
            jump_identity_file,
            jump_credential,
            rdp,
            telnet: false,
            ..Default::default()
        });
    }
    if out.is_empty() {
        return Err("No MobaXterm SSH or RDP sessions found in the pasted text.".into());
    }
    Ok(out)
}

/// MobaXterm writes the characters its own format uses — `;`, `|`, `#`, `%`
/// and `"` — as words in a setting's text: `cd /opt__PTVIRG__ ls`. Back to
/// what the user typed.
pub(crate) fn mobaxterm_unescape(text: &str) -> String {
    text.replace("__PTVIRG__", ";")
        .replace("__PIPE__", "|")
        .replace("__DIEZE__", "#")
        .replace("__PERCENT__", "%")
        .replace("__DBLQUO__", "\"")
}

/// The saved login a MobaXterm jump host signs in with: `[name]` as the
/// jump user, for a single jump host.
pub(crate) fn mobaxterm_jump_login(hosts: &str, users: &str) -> Option<String> {
    let hops = hosts.split("__PIPE__").filter(|h| !h.trim().is_empty()).count();
    let user = users.split("__PIPE__").next().unwrap_or("").trim();
    let login = user.strip_prefix('[')?.strip_suffix(']')?.trim();
    (hops == 1 && !login.is_empty()).then(|| login.to_string())
}

/// The most of FileZilla's site list that is read (a list of thousands of
/// sites is a few hundred KiB).
const FILEZILLA_MAX: usize = 16 * 1024 * 1024;

/// FileZilla's site list (`sitemanager.xml`, or what File → Export writes)
/// as hosts. FTP sites become FTP servers with at least the encryption
/// FileZilla used: its "FTP" (TLS when the server offers it) and "FTPES" are
/// FTPS with TLS after connecting, required here (no silent fall back to
/// plain FTP); implicit FTPS stays implicit; only FileZilla's own "plain FTP
/// (insecure)", chosen there on purpose, is plain. SFTP sites become SSH
/// servers. Other kinds (S3, WebDAV, cloud storage) are left out.
///
/// FileZilla keeps passwords in base64, which protects nothing, unless its
/// master password encrypts them: those stay behind and are asked when
/// connecting. A site in folders is named by its folders and its name, so
/// two of the same name stay apart. The XML may have no DTD (roxmltree's
/// default), so no entity can grow it or reach outside it.
pub(crate) fn parse_filezilla_sites(text: &str) -> Result<Vec<ImportedHost>, String> {
    use base64::Engine as _;
    if text.len() > FILEZILLA_MAX {
        return Err("The site list is too big to read".into());
    }
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("This isn't a FileZilla site list: {e}"))?;
    let root = doc.root_element();
    if !root.has_tag_name("FileZilla3") {
        return Err("This isn't a FileZilla site list".into());
    }
    let child_text = |node: roxmltree::Node, name: &str| -> Option<String> {
        node.children()
            .find(|c| c.has_tag_name(name))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    };
    // A node's own text (not its children's): a folder's name, and a site's
    // in older lists that have no <Name>.
    let own_text = |node: roxmltree::Node| -> String {
        node.children().filter(|c| c.is_text()).filter_map(|c| c.text()).collect::<String>().trim().to_string()
    };
    let mut out: Vec<ImportedHost> = Vec::new();
    let mut names = std::collections::HashSet::new();
    for site in root.descendants().filter(|n| n.has_tag_name("Server")) {
        let Some(host) = child_text(site, "Host") else { continue };
        if host.chars().any(|c| c.is_whitespace() || c.is_control()) || host.len() > 253 {
            continue;
        }
        let kind = child_text(site, "Protocol").and_then(|p| p.parse::<i32>().ok()).unwrap_or(0);
        let (protocol, usual_port) = match kind {
            0 | 4 => (Some("ftps"), crate::ftp::DEFAULT_PORT),
            3 => (Some("ftps-implicit"), crate::ftp::IMPLICIT_PORT),
            6 => (Some("ftp"), crate::ftp::DEFAULT_PORT),
            1 => (None, 22),
            _ => continue,
        };
        let port = child_text(site, "Port").and_then(|p| p.parse::<u16>().ok()).filter(|p| *p > 0).unwrap_or(usual_port);
        // 0 anonymous, 1 normal, 2 ask, 3 interactive, 4 account, 5 key file.
        let logon = child_text(site, "Logontype").and_then(|l| l.parse::<i32>().ok()).unwrap_or(1);
        let user = if logon == 0 { String::new() } else { child_text(site, "User").unwrap_or_default() };
        if user.contains(['\r', '\n', '\0']) {
            continue;
        }
        let password = if matches!(logon, 1 | 4) {
            site.children().find(|c| c.has_tag_name("Pass")).and_then(|p| {
                let raw = p.text()?.trim();
                match p.attribute("encoding") {
                    Some("base64") => base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok()),
                    // Lists from before FileZilla 3.26 keep it as it is.
                    None => Some(raw.to_string()),
                    // "crypt": FileZilla's master password protects it.
                    Some(_) => None,
                }
            })
        } else {
            None
        }
        .filter(|p| !p.is_empty() && !p.contains(['\r', '\n', '\0']));
        let identity_file = if kind == 1 && logon == 5 { child_text(site, "Keyfile") } else { None };
        let own = child_text(site, "Name").unwrap_or_else(|| own_text(site));
        let own = if own.is_empty() { host.clone() } else { own };
        let mut folders: Vec<String> = site
            .ancestors()
            .skip(1)
            .filter(|a| a.has_tag_name("Folder"))
            .map(own_text)
            .filter(|f| !f.is_empty())
            .collect();
        folders.reverse();
        folders.push(own);
        let base = folders.join(" / ");
        let mut name = base.clone();
        let mut n = 2;
        while !names.insert(name.clone()) {
            name = format!("{base} ({n})");
            n += 1;
        }
        out.push(ImportedHost {
            host_alias: name,
            hostname: host,
            port,
            user,
            identity_file,
            protocol: protocol.map(str::to_string),
            password,
            ..Default::default()
        });
    }
    if out.is_empty() {
        return Err("The site list has no FTP or SFTP sites to import.".into());
    }
    Ok(out)
}

/// FileZilla's own site list on this computer, read where FileZilla keeps it:
/// %APPDATA%\FileZilla on Windows, ~/.config/filezilla elsewhere (and the
/// older ~/.filezilla). The page offers it beside ~/.ssh/config. A lab build
/// with `SUBMARINE_LAB_HOME` reads only below that folder, as for
/// ~/.ssh/config (ssh_config::home_dir), never the tester's own list.
#[tauri::command]
pub(crate) fn read_filezilla_sites() -> Result<Vec<ImportedHost>, String> {
    let lab = cfg!(debug_assertions) && std::env::var("SUBMARINE_LAB_HOME").is_ok_and(|h| !h.is_empty());
    let mut places: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(windows)]
    if let (false, Some(appdata)) = (lab, std::env::var_os("APPDATA")) {
        places.push(std::path::PathBuf::from(appdata).join("FileZilla").join("sitemanager.xml"));
    }
    if let (false, Some(config)) = (lab, std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty())) {
        places.push(std::path::PathBuf::from(config).join("filezilla").join("sitemanager.xml"));
    }
    if let Some(home) = crate::ssh_config::home_dir().map(std::path::PathBuf::from) {
        places.push(home.join(".config").join("filezilla").join("sitemanager.xml"));
        places.push(home.join(".filezilla").join("sitemanager.xml"));
    }
    for place in places {
        let Ok(meta) = std::fs::metadata(&place) else { continue };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > FILEZILLA_MAX as u64 {
            return Err("FileZilla's site list is too big to read".into());
        }
        let text = std::fs::read_to_string(&place).map_err(|e| format!("FileZilla's site list can't be read: {e}"))?;
        return parse_filezilla_sites(&text);
    }
    Err("No FileZilla site list on this computer. In FileZilla, File → Export saves one; paste it under Paste.".into())
}

// The original project's tests of FileZilla's site list, moved here with it
// (#246): lib.rs keeps only the call in parse_client_hosts.
#[cfg(test)]
mod filezilla_tests {
    use crate::parse_client_import;

    /// FileZilla's site list: the kinds it has, passwords as FileZilla keeps
    /// them, folders in the names, what is left out.
    #[test]
    fn filezilla_site_lists() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>
<FileZilla3 version="3.69.0" platform="windows">
  <Servers>
    <Server>
      <Host>ftp.example.com</Host><Port>21</Port><Protocol>0</Protocol><Type>0</Type>
      <User>me@example.com</User><Pass encoding="base64">czNjcjN0</Pass><Logontype>1</Logontype>
      <Name>Web host</Name>
    </Server>
    <Folder expanded="1">Clients
      <Folder>Acme
        <Server>
          <Host>files.acme.test</Host><Port>990</Port><Protocol>3</Protocol>
          <User>acme</User><Pass encoding="crypt" pubkey="x">abc</Pass><Logontype>1</Logontype>
          <Name>Files</Name>
        </Server>
      </Folder>
      <Server>
        <Host>10.0.0.5</Host><Protocol>6</Protocol><Logontype>0</Logontype><Name>Old box</Name>
      </Server>
      <Server>
        <Host>sftp.acme.test</Host><Port>2222</Port><Protocol>1</Protocol>
        <User>deploy</User><Logontype>5</Logontype><Keyfile>C:\keys\deploy.ppk</Keyfile>
        <Name>Web host</Name>
      </Server>
      <Server><Host>bucket</Host><Protocol>7</Protocol><Name>S3</Name></Server>
      <Server><Host>ask.example</Host><Protocol>4</Protocol><User>u</User><Pass encoding="base64">eA==</Pass><Logontype>2</Logontype>Legacy name</Server>
    </Folder>
  </Servers>
</FileZilla3>"#;
        let hosts = parse_client_import(xml.to_string()).unwrap().hosts;
        let by = |n: &str| hosts.iter().find(|h| h.host_alias == n).unwrap_or_else(|| panic!("{n} missing: {:?}", hosts.iter().map(|h| &h.host_alias).collect::<Vec<_>>()));
        assert_eq!(hosts.len(), 5, "S3 is left out");
        let web = by("Web host");
        assert_eq!((web.protocol.as_deref(), web.port, web.user.as_str(), web.password.as_deref()), (Some("ftps"), 21, "me@example.com", Some("s3cr3t")));
        let files = by("Clients / Acme / Files");
        assert_eq!((files.protocol.as_deref(), files.port, files.password.as_deref()), (Some("ftps-implicit"), 990, None));
        let old = by("Clients / Old box");
        assert_eq!((old.protocol.as_deref(), old.user.as_str(), old.port), (Some("ftp"), "", 21));
        let sftp = by("Clients / Web host");
        assert_eq!((sftp.protocol.as_deref(), sftp.port, sftp.identity_file.as_deref()), (None, 2222, Some("C:\\keys\\deploy.ppk")));
        // "Ask for password": the user, not the password; a name in the old place.
        let ask = by("Clients / Legacy name");
        assert_eq!((ask.protocol.as_deref(), ask.user.as_str(), ask.password.as_deref()), (Some("ftps"), "u", None));
    }

    #[test]
    fn filezilla_lists_with_tricks_are_refused_or_cleaned() {
        // No DTD, so no entity can grow the list or read a file.
        let dtd = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa">]><FileZilla3><Servers><Server><Host>&a;</Host></Server></Servers></FileZilla3>"#;
        assert!(parse_client_import(dtd.to_string()).is_err());
        // A host with a space and a user with a line break are left out.
        let odd = "<FileZilla3><Servers><Server><Host>a b</Host></Server><Server><Host>ok.test</Host><User>x&#10;DELE y</User></Server></Servers></FileZilla3>";
        assert!(parse_client_import(odd.to_string()).is_err());
        assert!(parse_client_import("<FileZilla3><Servers/></FileZilla3>".to_string()).is_err());
    }
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod session_import;", 1),
                ("", "use session_import::*;", 1),
                ("parse_ssh_config", "parse_ssh_config_text(", 1),
                ("parse_client_import", "mobaxterm_skipped(", 1),
                ("parse_client_import", "putty_skipped(", 1),
                ("parse_client_hosts", "parse_putty_reg(", 1),
                ("parse_client_hosts", "parse_mobaxterm_sessions(", 1),
                ("parse_client_hosts", "parse_filezilla_sites(", 1),
                ("run", "import_hosts", 1),
                ("run", "read_filezilla_sites", 1),
            ],
        );
    }
}
