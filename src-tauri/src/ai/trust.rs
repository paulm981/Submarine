//! Approval levels and rules (#115): how much the AI may run on a server
//! without asking you.
//!
//! - **Three levels:** 1 asks every time (the default), 2 runs the commands
//!   you've allowed and asks for the rest, 3 runs anything and logs it.
//! - **Set on a server, a folder or as the default.** A server's own level
//!   wins, then the nearest folder above it that has one, then the default,
//!   then 1.
//! - **Two lists of rules,** on a server, a folder or for every server: what
//!   you always allow, and what always asks. Every list on the way up from
//!   the server counts. An always-ask rule beats every allow rule and level 3.
//! - **Kept on this device only,** in the vault's `ai_levels` and `ai_rules`
//!   tables, by the server's and folder's local ids. They're not in
//!   SYNCED_TABLES, so a shared profile never brings or changes them.
//!   Deleting a server or folder deletes its rows (the triggers in SCHEMA).
//! - **Only you change them,** from the app's own pages (mod.rs's Tauri
//!   commands). The AI has no tool for it: the bridge only reads them, as
//!   each request comes (AiState's `policy`).
//!
//! `decide` is the whole decision, kept pure so it can be tested on its own.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::sync::LazyLock;

pub(crate) const ASK_EVERY_TIME: u8 = 1;
pub(crate) const ALLOWED_COMMANDS: u8 = 2;
pub(crate) const RUN_ANYTHING: u8 = 3;

/// What a rule may be at most, in bytes.
pub(crate) const MAX_RULE: usize = 1024;
/// Rules in one list at most.
const MAX_RULES: i64 = 500;
/// Folders followed up from a server at most: a loop in a broken tree ends.
const MAX_DEPTH: usize = 64;

/// What makes a command more than one simple command, to a POSIX shell (and
/// to cmd.exe and PowerShell): lists, pipes, background, substitution,
/// expansion, redirection, subshells and groups.
const OPERATORS: &[char] = &[';', '&', '|', '`', '$', '>', '<', '(', ')', '{', '}'];

/// The characters the request's card shows as ⟨U+…⟩ (AiPanel's showPlainly:
/// HIDDEN_CONTROLS in src/util/fileName.ts, and the control characters):
/// format characters such as the right-to-left override and zero-width
/// spaces, line and paragraph separators, the invisible letters, variation
/// selectors and tags, and every C0 and C1 control (tab, newline and CR
/// among them).
static HIDDEN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(concat!(
        r"[\p{Cf}\p{Zl}\p{Zp}\u{AD}\u{34F}\u{115F}\u{1160}\u{17B4}\u{17B5}\u{180E}\u{3164}\u{FFA0}",
        r"\u{FE00}-\u{FE0F}\u{E0000}-\u{E007F}\u{E0100}-\u{E01EF}\x00-\x1F\x7F-\x9F]"
    ))
    .unwrap()
});

fn hidden(c: char) -> bool {
    let mut buf = [0u8; 4];
    HIDDEN.is_match(c.encode_utf8(&mut buf))
}

/// The command as rules see it: spaces and tabs at either end taken off.
fn trimmed(command: &str) -> &str {
    command.trim_matches(|c| c == ' ' || c == '\t')
}

/// One simple command, which an allow rule may match: no operator, no
/// newline or CR, no backslash at the end (which would join a next line),
/// and none of the hidden characters. Conservative on purpose:
/// `echo "a;b"` isn't one, though the shell would run it as one.
pub(crate) fn is_simple(command: &str) -> bool {
    let c = trimmed(command);
    !c.is_empty() && !c.ends_with('\\') && !c.chars().any(|ch| OPERATORS.contains(&ch) || hidden(ch))
}

/// Whether the allow rule `rule` matches `command`: `uptime` exactly that,
/// `systemctl status *` that followed by a space and anything, or alone,
/// `mkfs*` anything that starts with `mkfs`. Only a simple command
/// (is_simple) matches, and a rule with `*` anywhere but at its end matches
/// nothing.
pub(crate) fn allow_matches(rule: &str, command: &str) -> bool {
    let cmd = trimmed(command);
    if !is_simple(cmd) {
        return false;
    }
    let rule = rule.trim();
    let (body, wild) = match rule.strip_suffix('*') {
        Some(b) => (b, true),
        None => (rule, false),
    };
    if body.contains('*') || body.trim().is_empty() {
        return false;
    }
    if !wild {
        return cmd == body;
    }
    match body.strip_suffix(' ') {
        // A word boundary: `ls *` is `ls` and `ls -la`, never `lsof`.
        Some(prefix) => {
            let prefix = prefix.trim_end();
            cmd == prefix || cmd.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(' '))
        }
        None => cmd.starts_with(body),
    }
}

/// A word as an always-ask rule compares it: without quotes, backslashes or
/// hidden characters (`r''eboot`, `\reboot` and `re<U+200B>boot` are all
/// `reboot`), and in lower case (cmd.exe doesn't mind case).
fn plain_word(w: &str) -> String {
    w.chars().filter(|c| !matches!(c, '\'' | '"' | '\\') && !hidden(*c)).collect::<String>().to_lowercase()
}

/// The words of a command or a rule: split at the operators and at any
/// whitespace or control character.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c.is_control() || OPERATORS.contains(&c))
        .map(plain_word)
        .filter(|w| !w.is_empty())
        .collect()
}

/// An always-ask rule's words, and whether its last is a prefix (`mkfs*`).
/// A trailing ` *` adds nothing: the words may be followed by anything.
fn ask_words(rule: &str) -> Option<(Vec<String>, bool)> {
    let rule = rule.trim();
    let (body, prefix) = match rule.strip_suffix('*') {
        Some(b) => (b, !b.ends_with(|c: char| c.is_whitespace())),
        None => (rule, false),
    };
    if body.contains('*') {
        return None;
    }
    let w = words(body);
    (!w.is_empty()).then_some((w, prefix))
}

/// Whether the always-ask rule `rule` is in `command`: its words, in order
/// and side by side, as words anywhere in it, whatever comes before or
/// after. So `reboot` is in `uptime; reboot`, `sudo reboot` and
/// `/sbin/reboot now`, and `rm -rf *` in `cd /; rm -rf /tmp`. A command
/// word counts as itself or as the name after its last `/`.
///
/// It catches a rule written plainly, quoted or with backslashes, not a
/// command that builds the name as it runs (`$(echo re)boot`, base64): the
/// always-ask list is a safety net, not a wall.
pub(crate) fn ask_matches(rule: &str, command: &str) -> bool {
    let Some((want, prefix)) = ask_words(rule) else { return false };
    let have = words(command);
    let same = |w: &str, r: &str, last: bool| {
        let base = w.rsplit('/').next().unwrap_or(w);
        if last && prefix {
            w.starts_with(r) || base.starts_with(r)
        } else {
            w == r || base == r
        }
    };
    have.windows(want.len()).any(|win| win.iter().zip(&want).enumerate().all(|(i, (w, r))| same(w, r, i + 1 == want.len())))
}

/// What happens to a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// It waits for you. `rule`: the always-ask rule that made it, if one did.
    Ask { rule: Option<String> },
    /// It runs now: it matches your allow rule.
    Rule(String),
    /// It runs now: the server is at "Run anything".
    Anything,
    /// It runs now: you allowed everything on this server until the AI tab
    /// closes.
    Session,
}

impl Decision {
    /// How it was approved, for the audit log and the panel: None when it
    /// waits for you.
    pub(crate) fn approved_by(&self) -> Option<String> {
        match self {
            Decision::Ask { .. } => None,
            Decision::Rule(r) => Some(format!("rule: {r}")),
            Decision::Anything => Some("level: run anything".into()),
            Decision::Session => Some("session: allow everything".into()),
        }
    }
}

/// The decision for `command` on a server at `level`.
///
/// - An always-ask rule that matches makes it ask, whatever the level.
/// - "Allow everything here until this AI tab closes" runs the rest, as
///   level 3 does.
/// - At level 2, a command that asks for root (`is_root_cmd`: sudo, dzdo,
///   su, doas, pkexec) asks; otherwise an allow rule that matches runs it.
/// - Anything else asks.
pub(crate) fn decide(level: u8, session_allow: bool, allow: &[String], ask: &[String], command: &str, is_root_cmd: bool) -> Decision {
    if let Some(rule) = ask.iter().find(|r| ask_matches(r, command)) {
        return Decision::Ask { rule: Some(rule.clone()) };
    }
    if session_allow || level == RUN_ANYTHING {
        return if session_allow { Decision::Session } else { Decision::Anything };
    }
    if level == ALLOWED_COMMANDS && !is_root_cmd {
        if let Some(rule) = allow.iter().find(|r| allow_matches(r, command)) {
            return Decision::Rule(rule.clone());
        }
    }
    Decision::Ask { rule: None }
}

/// A rule as you typed it, checked: trimmed, not empty, not too long, `*`
/// only at its end and not alone. An allow rule must be one simple command
/// itself, or it could never match.
pub(crate) fn check_rule(kind: &str, pattern: &str) -> Result<String, String> {
    let p = pattern.trim();
    if p.is_empty() {
        return Err("Write a command.".into());
    }
    if p.len() > MAX_RULE {
        return Err(format!("A rule can be at most {MAX_RULE} characters long."));
    }
    let body = p.strip_suffix('*').unwrap_or(p);
    if body.contains('*') {
        return Err("A * can only go at the end of a rule.".into());
    }
    if body.trim().is_empty() {
        return Err("Write the command before the *.".into());
    }
    match kind {
        "allow" if !is_simple(p) => Err(
            "An allowed command must be one simple command: no ; & | ` $ > < ( ) { }, new lines or hidden characters.".into(),
        ),
        "allow" => Ok(p.to_string()),
        "ask" if ask_words(p).is_none() => Err("Write the command's words.".into()),
        "ask" => Ok(p.to_string()),
        _ => Err("That isn't a kind of rule.".into()),
    }
}

// ---------------------------------------------------------------------------
// Kept in the vault, on this device
// ---------------------------------------------------------------------------

/// Device-local, like `walls`: not in SYNCED_TABLES, no sync columns, no
/// sync triggers. `scope` is `server`, `folder` or `default` (with
/// `scope_id` 0).
pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS ai_levels (
    scope TEXT NOT NULL CHECK (scope IN ('server', 'folder', 'default')),
    scope_id INTEGER NOT NULL,
    level INTEGER NOT NULL CHECK (level BETWEEN 1 AND 3),
    PRIMARY KEY (scope, scope_id)
);
CREATE TABLE IF NOT EXISTS ai_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT NOT NULL CHECK (scope IN ('server', 'folder', 'default')),
    scope_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('allow', 'ask')),
    pattern TEXT NOT NULL,
    UNIQUE (scope, scope_id, kind, pattern)
);
CREATE TRIGGER IF NOT EXISTS ai_trust_server_gone AFTER DELETE ON servers BEGIN
    DELETE FROM ai_levels WHERE scope = 'server' AND scope_id = OLD.id;
    DELETE FROM ai_rules WHERE scope = 'server' AND scope_id = OLD.id;
END;
CREATE TRIGGER IF NOT EXISTS ai_trust_folder_gone AFTER DELETE ON folders BEGIN
    DELETE FROM ai_levels WHERE scope = 'folder' AND scope_id = OLD.id;
    DELETE FROM ai_rules WHERE scope = 'folder' AND scope_id = OLD.id;
END;";

/// The always-ask list a vault starts with, for every server. You can
/// remove any of them in Settings › AI tools.
pub(crate) const DEFAULT_ASK: &[&str] =
    &["reboot", "shutdown", "poweroff", "halt", "init 0", "init 6", "rm -rf *", "mkfs*", "dd *", ":(){"];

fn db(code: &'static str) -> impl Fn(rusqlite::Error) -> String {
    move |e| format!("[DATABASE] {code}: {e}")
}

/// Make the tables, and the first time, the default always-ask list. Every
/// vault open runs it, on `servers` and `folders` as they are by then.
pub(crate) fn create_schema(conn: &Connection) -> Result<(), String> {
    let had: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ai_rules'", [], |_| Ok(()))
        .optional()
        .map_err(db("AI_TRUST_SCHEMA"))?
        .is_some();
    conn.execute_batch(SCHEMA).map_err(db("AI_TRUST_SCHEMA"))?;
    if !had {
        for p in DEFAULT_ASK {
            conn.execute("INSERT OR IGNORE INTO ai_rules (scope, scope_id, kind, pattern) VALUES ('default', 0, 'ask', ?1)", [p])
                .map_err(db("AI_TRUST_DEFAULTS"))?;
        }
    }
    Ok(())
}

/// What holds for one server: its level and every rule on the way up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    pub level: u8,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
}

impl Default for Policy {
    /// Ask every time: what holds when nothing can be read.
    fn default() -> Self {
        Policy { level: ASK_EVERY_TIME, allow: Vec::new(), ask: Vec::new() }
    }
}

/// The scopes that count for a server, nearest first: the server, its
/// folder and each one above, the default. Without a saved server (a quick
/// connect), the default only.
fn chain(conn: &Connection, server_id: Option<i64>) -> rusqlite::Result<Vec<(&'static str, i64)>> {
    let mut out = Vec::new();
    if let Some(id) = server_id {
        out.push(("server", id));
        let mut folder: Option<i64> = conn.query_row("SELECT folder_id FROM servers WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        while let Some(f) = folder {
            if out.iter().any(|(s, i)| *s == "folder" && *i == f) || out.len() > MAX_DEPTH {
                break;
            }
            out.push(("folder", f));
            folder = conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [f], |r| r.get(0)).optional()?.flatten();
        }
    }
    out.push(("default", 0));
    Ok(out)
}

fn level_of(conn: &Connection, scope: &str, id: i64) -> rusqlite::Result<Option<u8>> {
    conn.query_row("SELECT level FROM ai_levels WHERE scope = ?1 AND scope_id = ?2", params![scope, id], |r| r.get(0)).optional()
}

fn try_policy(conn: &Connection, server_id: Option<i64>) -> rusqlite::Result<Policy> {
    let mut p = Policy::default();
    let mut level = None;
    let mut stmt = conn.prepare("SELECT kind, pattern FROM ai_rules WHERE scope = ?1 AND scope_id = ?2 ORDER BY id")?;
    for (scope, id) in chain(conn, server_id)? {
        if level.is_none() {
            level = level_of(conn, scope, id)?;
        }
        for row in stmt.query_map(params![scope, id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (kind, pattern) = row?;
            if kind == "allow" { &mut p.allow } else { &mut p.ask }.push(pattern);
        }
    }
    p.level = level.filter(|l| (1..=3).contains(l)).unwrap_or(ASK_EVERY_TIME);
    Ok(p)
}

/// What holds for `server_id`. Anything that can't be read asks every time.
pub(crate) fn policy_for(conn: &Connection, server_id: Option<i64>) -> Policy {
    try_policy(conn, server_id).unwrap_or_default()
}

/// A level set on a server or folder (or the default), for Settings.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub(crate) struct LevelRow {
    pub scope: String,
    pub scope_id: i64,
    /// The server's or folder's name; empty for the default.
    pub name: String,
    pub level: u8,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub(crate) struct RuleRow {
    pub id: i64,
    pub scope: String,
    pub scope_id: i64,
    pub name: String,
    pub kind: String,
    pub pattern: String,
}

/// Everything set, for Settings › AI tools.
#[derive(Serialize, Debug)]
pub(crate) struct TrustView {
    pub default_level: u8,
    /// The servers and folders set above "Ask every time".
    pub levels: Vec<LevelRow>,
    /// Every rule, the default's first.
    pub rules: Vec<RuleRow>,
}

const NAME: &str = "COALESCE(CASE scope WHEN 'server' THEN (SELECT name FROM servers WHERE id = scope_id) \
     WHEN 'folder' THEN (SELECT name FROM folders WHERE id = scope_id) END, '')";

fn rules_where(conn: &Connection, filter: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<RuleRow>, String> {
    let sql = format!(
        "SELECT id, scope, scope_id, {NAME}, kind, pattern FROM ai_rules {filter} \
         ORDER BY scope = 'default' DESC, scope, lower({NAME}), kind, id"
    );
    let mut stmt = conn.prepare(&sql).map_err(db("AI_RULES_LIST"))?;
    let rows = stmt
        .query_map(args, |r| {
            Ok(RuleRow { id: r.get(0)?, scope: r.get(1)?, scope_id: r.get(2)?, name: r.get(3)?, kind: r.get(4)?, pattern: r.get(5)? })
        })
        .map_err(db("AI_RULES_LIST"))?;
    rows.collect::<Result<_, _>>().map_err(db("AI_RULES_LIST"))
}

pub(crate) fn view(conn: &Connection) -> Result<TrustView, String> {
    let default_level = level_of(conn, "default", 0).map_err(db("AI_LEVELS_LIST"))?.unwrap_or(ASK_EVERY_TIME);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT scope, scope_id, {NAME}, level FROM ai_levels WHERE scope != 'default' AND level > 1 ORDER BY scope DESC, lower({NAME})"
        ))
        .map_err(db("AI_LEVELS_LIST"))?;
    let levels = stmt
        .query_map([], |r| Ok(LevelRow { scope: r.get(0)?, scope_id: r.get(1)?, name: r.get(2)?, level: r.get(3)? }))
        .map_err(db("AI_LEVELS_LIST"))?
        .collect::<Result<_, _>>()
        .map_err(db("AI_LEVELS_LIST"))?;
    Ok(TrustView { default_level, levels, rules: rules_where(conn, "", &[])? })
}

/// One server's or folder's own settings, and what it gets from above, for
/// its editor.
#[derive(Serialize, Debug)]
pub(crate) struct ScopeView {
    /// Set on it; None: it follows its folder or the default.
    pub own_level: Option<u8>,
    /// What it follows when it has none of its own.
    pub inherited_level: u8,
    /// Where that comes from: a folder's name, or empty for the default.
    pub inherited_from: String,
    /// Its own rules.
    pub rules: Vec<RuleRow>,
    /// How many rules it gets from above, of each kind.
    pub inherited_allow: usize,
    pub inherited_ask: usize,
}

pub(crate) fn scope_view(conn: &Connection, scope: &str, id: i64) -> Result<ScopeView, String> {
    check_scope(scope)?;
    let id = if scope == "default" { 0 } else { id };
    let full = match scope {
        "server" => chain(conn, Some(id)),
        "folder" => chain(conn, None).map(|d| {
            // The folder and those above it, then the default.
            let mut out = vec![("folder", id)];
            let mut f: Option<i64> = conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| r.get(0)).ok().flatten();
            while let Some(x) = f {
                if out.iter().any(|(_, i)| *i == x) || out.len() > MAX_DEPTH {
                    break;
                }
                out.push(("folder", x));
                f = conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [x], |r| r.get(0)).ok().flatten();
            }
            out.extend(d);
            out
        }),
        _ => Ok(vec![("default", 0)]),
    }
    .map_err(db("AI_SCOPE"))?;
    let own_level = level_of(conn, scope, id).map_err(db("AI_SCOPE"))?;
    let (mut inherited_level, mut inherited_from) = (ASK_EVERY_TIME, String::new());
    for (s, i) in full.iter().skip(1) {
        if let Some(l) = level_of(conn, s, *i).map_err(db("AI_SCOPE"))? {
            inherited_level = l;
            if *s == "folder" {
                inherited_from = conn.query_row("SELECT name FROM folders WHERE id = ?1", [i], |r| r.get(0)).unwrap_or_default();
            }
            break;
        }
    }
    let (mut inherited_allow, mut inherited_ask) = (0, 0);
    for (s, i) in full.iter().skip(1) {
        let count = |kind: &str| -> Result<usize, String> {
            conn.query_row("SELECT COUNT(*) FROM ai_rules WHERE scope = ?1 AND scope_id = ?2 AND kind = ?3", params![s, i, kind], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
                .map_err(db("AI_SCOPE"))
        };
        inherited_allow += count("allow")?;
        inherited_ask += count("ask")?;
    }
    let rules = rules_where(conn, "WHERE scope = ?1 AND scope_id = ?2", &[&scope, &id])?;
    Ok(ScopeView { own_level, inherited_level, inherited_from, rules, inherited_allow, inherited_ask })
}

fn check_scope(scope: &str) -> Result<(), String> {
    if matches!(scope, "server" | "folder" | "default") {
        Ok(())
    } else {
        Err("That isn't a server, a folder or the default.".into())
    }
}

/// The server or folder must exist.
fn check_target(conn: &Connection, scope: &str, id: i64) -> Result<(), String> {
    check_scope(scope)?;
    let table = match scope {
        "server" => "servers",
        "folder" => "folders",
        _ => return Ok(()),
    };
    let found = conn
        .query_row(&format!("SELECT 1 FROM {table} WHERE id = ?1"), [id], |_| Ok(()))
        .optional()
        .map_err(db("AI_TRUST_TARGET"))?
        .is_some();
    if found {
        Ok(())
    } else {
        Err(format!("That {scope} isn't in this profile any more."))
    }
}

/// Set a level, or (None) take it off, so it follows its folder or the
/// default. The default taken off is "Ask every time".
pub(crate) fn set_level(conn: &Connection, scope: &str, id: i64, level: Option<u8>) -> Result<(), String> {
    check_target(conn, scope, id)?;
    let id = if scope == "default" { 0 } else { id };
    match level {
        Some(l) if (1..=3).contains(&l) => conn
            .execute(
                "INSERT INTO ai_levels (scope, scope_id, level) VALUES (?1, ?2, ?3) ON CONFLICT (scope, scope_id) DO UPDATE SET level = excluded.level",
                params![scope, id, l],
            )
            .map(|_| ()),
        Some(_) => return Err("A level is 1, 2 or 3.".into()),
        None => conn.execute("DELETE FROM ai_levels WHERE scope = ?1 AND scope_id = ?2", params![scope, id]).map(|_| ()),
    }
    .map_err(db("AI_LEVEL_SET"))
}

/// Add a rule; one already there is fine. Returns the rule as kept.
pub(crate) fn add_rule(conn: &Connection, scope: &str, id: i64, kind: &str, pattern: &str) -> Result<String, String> {
    check_target(conn, scope, id)?;
    let id = if scope == "default" { 0 } else { id };
    let pattern = check_rule(kind, pattern)?;
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_rules WHERE scope = ?1 AND scope_id = ?2 AND kind = ?3", params![scope, id, kind], |r| r.get(0))
        .map_err(db("AI_RULE_ADD"))?;
    if n >= MAX_RULES {
        return Err(format!("A list holds up to {MAX_RULES} rules."));
    }
    conn.execute("INSERT OR IGNORE INTO ai_rules (scope, scope_id, kind, pattern) VALUES (?1, ?2, ?3, ?4)", params![scope, id, kind, pattern])
        .map_err(db("AI_RULE_ADD"))?;
    Ok(pattern)
}

/// Remove a rule; returns what it was, for the audit log.
pub(crate) fn remove_rule(conn: &Connection, rule_id: i64) -> Result<Option<RuleRow>, String> {
    let row = rules_where(conn, "WHERE id = ?1", &[&rule_id])?.into_iter().next();
    conn.execute("DELETE FROM ai_rules WHERE id = ?1", [rule_id]).map_err(db("AI_RULE_REMOVE"))?;
    Ok(row)
}

/// What a scope is called, for the audit log.
pub(crate) fn describe(conn: &Connection, scope: &str, id: i64) -> String {
    let name = |table: &str| conn.query_row(&format!("SELECT name FROM {table} WHERE id = ?1"), [id], |r| r.get::<_, String>(0)).unwrap_or_default();
    match scope {
        "server" => format!("server {} (id {id})", name("servers")),
        "folder" => format!("folder {} (id {id})", name("folders")),
        _ => "every server".into(),
    }
}

#[cfg(test)]
mod tests;
