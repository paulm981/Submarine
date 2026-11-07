//! Reading other apps' session lists for the import: OpenSSH client
//! config (`parse_ssh_config_text`) and MobaXterm's path placeholders.
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
                ("parse_mobaxterm_sessions_in", "expand_mobaxterm_path(", 1),
                ("run", "import_hosts", 1),
            ],
        );
    }
}
