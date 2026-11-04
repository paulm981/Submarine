//! Settings → Troubleshooting → "Copy diagnostics", with the profile's
//! names replaced by placeholders.
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use diagnostics::*`.

use crate::*;

/// Settings → Troubleshooting → "Copy diagnostics" (applog.rs): the build,
/// the system, what the page says about itself (`ui`: its settings and the
/// freezes it saw) and the end of the log. The open profile's server, folder,
/// login and key names, its host and user names, IPv4 and e-mail addresses
/// and the home folder are replaced with placeholders, so it can be pasted to
/// whoever is helping without giving the servers away.
#[tauri::command]
pub(crate) async fn diagnostics_report(db_state: tauri::State<'_, DbState>, ui: String) -> Result<String, String> {
    let names = diagnostic_names(&db_state);
    let webview = tauri::webview_version().unwrap_or_else(|_| "unknown".into());
    let text = format!(
        "Submarine diagnostics, {}\n\
         Build: {}\n\
         System: {} {}, webview {}, portable: {}\n\
         Names, hosts and addresses: {}\n\
         Window: {}\n\
         \n--- The end of the log ---\n{}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        about::build_label(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        webview,
        if portable::current().is_portable() { "yes" } else { "no" },
        if names.is_some() {
            "replaced with placeholders"
        } else {
            "NOT replaced (no profile is open): check the text before sharing it"
        },
        ui.trim(),
        applog::tail(400),
    );
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok();
    Ok(applog::redact(&text, names.as_deref().unwrap_or(&[]), home.as_deref()))
}

/// What "Copy diagnostics" hides: every name, host and user name in the open
/// profile, each with what it is. None when no profile is open.
pub(crate) fn diagnostic_names(db_state: &DbState) -> Option<Vec<(String, &'static str)>> {
    let guard = db_state.conn.lock().ok()?;
    let conn = guard.as_ref()?;
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut add = |sql: &str, kinds: &[&'static str]| {
        let Ok(mut stmt) = conn.prepare(sql) else { return };
        let Ok(rows) = stmt.query_map([], |r| {
            (0..kinds.len()).map(|i| r.get::<_, Option<String>>(i)).collect::<rusqlite::Result<Vec<_>>>()
        }) else {
            return;
        };
        for row in rows.flatten() {
            for (value, kind) in row.into_iter().zip(kinds) {
                if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
                    out.push((v, kind));
                }
            }
        }
    };
    add("SELECT name, host, username, proxy_host FROM servers", &["server", "host", "user", "host"]);
    add("SELECT name FROM folders", &["folder"]);
    add("SELECT name, username FROM credentials", &["login", "user"]);
    add("SELECT name FROM ssh_keys", &["key"]);
    drop(guard);
    if let Some(profile) = db_state.active_profile.lock().ok().and_then(|g| g.clone()) {
        out.push((profile, "profile"));
    }
    Some(out)
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
                ("", "mod diagnostics;", 1),
                ("", "use diagnostics::*;", 1),
                ("run", "diagnostics_report", 1),
            ],
        );
    }
}
