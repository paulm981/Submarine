//! Sessions that survive a reconnect (#126): a terminal's shell kept running
//! on the server in tmux, or GNU screen where there's no tmux, so a dropped
//! connection, sleep or a closed tab leaves it running and the next terminal
//! picks it up again: the same shell, what was running still running.
//!
//! - **This device's choice**: a setting for all servers, and a server's own
//!   (Use the default, Always, Never) by its sync id, both kept in the page
//!   and never synced, so a change elsewhere never makes this computer start
//!   anything. The page tells this module which tabs want it
//!   (`keep_session_tab`). Nothing ever asks: a closed tab detaches, a
//!   reconnect attaches again.
//! - **Nothing is installed on a server.** One quick exec per new terminal
//!   asks which of tmux and screen it has and what's running (`PROBE`). With
//!   neither, the terminal is a plain shell and says so.
//! - **Names.** `submarine-<server>-<n>`: `<server>` is 8 hex digits of the
//!   SHA-256 of the server's sync id, the same on all the user's computers so
//!   a session left on another one is found; `<n>` is its slot, 1 to 16. A
//!   new terminal takes the first slot no other terminal of this app shows,
//!   a session waiting there (not shown anywhere) first, else a new one.
//!   Only names of exactly that shape are ever listed, attached or ended, and
//!   each goes on the command line single-quoted too; screen's by process id
//!   as well, as screen matches a bare name by its start.
//! - **Ending is the user's:** `exit` in the shell ends the session, or End
//!   in the sessions list. A terminal whose session is gone when it opens
//!   again (a reconnect) gets a plain shell, never a session made anew.
//! - **On-connect commands** go only into a new session, never typed into one
//!   still running (`keep_session_state`).
//!
//! open_terminal (lib.rs) starts the terminal's shell with `start`'s command,
//! and shows its note first (`early`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::remote_sh;
use crate::ssh_manager::SshState;

const PREFIX: &str = "submarine-";
/// Hex digits of the server's id in a name.
const KEY_LEN: usize = 8;
/// Sessions one server can keep.
const SLOTS: u8 = 16;
/// The longest sync id taken.
const MAX_UUID: usize = 128;
/// The longest tab or terminal id taken from the page.
const MAX_ID: usize = 256;
/// Tabs, and terminals, this device keeps track of at once.
const MAX_TABS: usize = 256;
const MAX_TERMINALS: usize = 1024;
/// A tab's terminals the page names at once.
const MAX_TAB_TERMINALS: usize = 256;
/// What's read of the server's answer, and how long it may take.
const PROBE_MAX_BYTES: usize = 64 * 1024;
const PROBE_SECS: u64 = 8;
/// Sessions listed, and the longest line of a listing that's read.
const MAX_LISTED: usize = 64;
const MAX_LINE: usize = 512;

/// Which of tmux and screen the server has, and its sessions. POSIX sh, sent
/// as `remote_sh::sh_command`, so the account's own shell never parses it.
/// The last line says the answer came whole.
const PROBE: &str = r#"if command -v tmux >/dev/null 2>&1; then
  echo __SUB_KEEP:tmux
  tmux list-sessions -F '#{session_name} #{session_created} #{session_attached}' 2>/dev/null
elif command -v screen >/dev/null 2>&1; then
  echo __SUB_KEEP:screen
  screen -ls 2>/dev/null
else
  echo __SUB_KEEP:none
fi
echo __SUB_KEEP:end
"#;

const NOTE_NEITHER: &str = "\x1b[33mSubmarine: no tmux or screen on this server, so this terminal isn't kept.\x1b[0m\r\n";
const NOTE_UNKNOWN: &str = "\x1b[33mSubmarine couldn't check this server for tmux or screen, so this terminal isn't kept.\x1b[0m\r\n";
const NOTE_ENDED: &str = "\x1b[33mSubmarine: this terminal's kept session has ended, so this shell isn't kept.\x1b[0m\r\n";
const NOTE_FULL: &str = "\x1b[33mSubmarine: this server's 16 kept sessions are all in use, so this terminal isn't kept.\x1b[0m\r\n";

/// What keeps the session on the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Tool {
    Tmux,
    Screen,
}

/// A server's part of its sessions' names: 8 hex digits of its sync id's
/// SHA-256. The id comes from a sync, so it's only ever hashed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServerKey(String);

impl ServerKey {
    pub(crate) fn of(uuid: &str) -> Option<Self> {
        let uuid = uuid.trim();
        if uuid.is_empty() || uuid.len() > MAX_UUID {
            return None;
        }
        Some(Self(hex::encode(&Sha256::digest(uuid.as_bytes())[..KEY_LEN / 2])))
    }
}

/// A kept session's name, `submarine-<8 hex>-<1..16>`: nothing else is one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct SessionName(String);

impl SessionName {
    fn new(key: &ServerKey, slot: u8) -> Option<Self> {
        (1..=SLOTS).contains(&slot).then(|| Self(format!("{PREFIX}{}-{slot}", key.0)))
    }

    /// `s` if it is exactly a kept session's name.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let rest = s.strip_prefix(PREFIX)?;
        let (key, slot) = rest.split_at_checked(KEY_LEN)?;
        let slot = slot.strip_prefix('-')?;
        let ok = key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && matches!(slot.as_bytes(), [b'1'..=b'9'] | [b'1'..=b'9', b'0'..=b'9'])
            && slot.parse::<u8>().is_ok_and(|n| n <= SLOTS);
        ok.then(|| Self(s.to_string()))
    }

    fn key(&self) -> &str {
        &self.0[PREFIX.len()..PREFIX.len() + KEY_LEN]
    }

    fn slot(&self) -> u8 {
        self.0[PREFIX.len() + KEY_LEN + 1..].parse().unwrap_or(0)
    }
}

impl std::fmt::Display for SessionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A kept session the server listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    name: SessionName,
    /// screen's process id, part of what names it exactly.
    pid: Option<u32>,
    /// When it started (tmux), in seconds since 1970.
    created: Option<u64>,
    /// Whether a terminal somewhere shows it now.
    attached: bool,
}

/// The server's answer to `PROBE`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    tool: Option<Tool>,
    sessions: Vec<Listed>,
    /// The answer came whole: its last line was there.
    complete: bool,
}

/// `out`, the server's answer to `PROBE`; None when it didn't answer as one.
/// Only kept sessions' names are taken, at most MAX_LISTED of them; any
/// other line, or one that doesn't parse, is passed over.
pub(crate) fn parse_probe(out: &str) -> Option<Probe> {
    let mut found: Option<Option<Tool>> = None;
    let mut sessions = Vec::new();
    let mut complete = false;
    for line in out.lines() {
        if line.len() > MAX_LINE {
            continue;
        }
        if let Some(marker) = line.trim().strip_prefix("__SUB_KEEP:") {
            match (marker, found) {
                ("end", Some(_)) => {
                    complete = true;
                    break;
                }
                ("tmux", None) => found = Some(Some(Tool::Tmux)),
                ("screen", None) => found = Some(Some(Tool::Screen)),
                ("none", None) => found = Some(None),
                _ => {}
            }
            continue;
        }
        let listed = match found {
            Some(Some(Tool::Tmux)) => parse_tmux_line(line),
            Some(Some(Tool::Screen)) => parse_screen_line(line),
            _ => None,
        };
        if let Some(listed) = listed {
            if sessions.len() < MAX_LISTED {
                sessions.push(listed);
            }
        }
    }
    Some(Probe { tool: found?, sessions, complete })
}

/// A line of `tmux list-sessions -F '#{session_name} #{session_created}
/// #{session_attached}'`.
fn parse_tmux_line(line: &str) -> Option<Listed> {
    let mut fields = line.split_whitespace();
    let (name, created, attached) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() {
        return None;
    }
    Some(Listed {
        name: SessionName::parse(name)?,
        pid: None,
        created: Some(created.parse().ok()?),
        attached: attached.parse::<u32>().ok()? > 0,
    })
}

/// A line of `screen -ls`: `<tab>12345.name<tab>(date)<tab>(Detached)`, the
/// date there only in some versions.
fn parse_screen_line(line: &str) -> Option<Listed> {
    if !line.starts_with('\t') {
        return None;
    }
    let fields: Vec<&str> = line.split('\t').map(str::trim).filter(|f| !f.is_empty()).collect();
    let (first, status) = match fields.as_slice() {
        [first, status] | [first, _, status] => (*first, *status),
        _ => return None,
    };
    let (pid, name) = first.split_once('.')?;
    if pid.is_empty() || pid.len() > 10 || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let attached = match status.to_ascii_lowercase().as_str() {
        "(attached)" | "(multi, attached)" => true,
        "(detached)" | "(multi, detached)" => false,
        _ => return None,
    };
    Some(Listed { name: SessionName::parse(name)?, pid: Some(pid.parse().ok()?), created: None, attached })
}

/// The command a terminal's shell starts as, to show `name`: attached to it
/// when it's running (`running`, as listed), else made.
pub(crate) fn attach_command(tool: Tool, name: &SessionName, running: Option<&Listed>) -> String {
    match (tool, running.and_then(|l| l.pid)) {
        // -A: attach when it's there, make it when it isn't. -u: the
        // terminal takes UTF-8, whatever the server's locale says.
        (Tool::Tmux, _) => format!("tmux -u new-session -A -s {}", remote_sh::quote(&name.0)),
        // -x: attach, even where it's shown already. By its process id too,
        // so no other session is taken for it.
        (Tool::Screen, Some(pid)) => format!("screen -U -x {}", remote_sh::quote(&format!("{pid}.{name}"))),
        (Tool::Screen, None) => format!("screen -U -S {}", remote_sh::quote(&name.0)),
    }
}

/// The command that ends `name` for good. screen's needs its process id.
pub(crate) fn end_command(tool: Tool, name: &SessionName, pid: Option<u32>) -> Result<String, String> {
    match (tool, pid) {
        (Tool::Tmux, _) => Ok(format!("tmux kill-session -t {}", remote_sh::quote(&name.0))),
        (Tool::Screen, Some(pid)) => Ok(format!("screen -S {} -X quit", remote_sh::quote(&format!("{pid}.{name}")))),
        (Tool::Screen, None) => Err("That screen session has no process id".into()),
    }
}

fn probe_command() -> String {
    remote_sh::sh_command(&[], PROBE, &[])
}

/// A terminal that keeps its session.
#[derive(Debug)]
struct Kept {
    /// Its tab's session id.
    tab: String,
    name: SessionName,
    /// Its session was running when it last opened: it was picked up again,
    /// not made.
    running: bool,
    /// Opened from the sessions list: it shows that session whatever the
    /// tab's choice is.
    pinned: bool,
    /// Its session was gone when it opened again (`exit`, End): it isn't
    /// made anew for this terminal, which then holds no slot.
    ended: bool,
}

/// What a terminal about to open is to show.
#[derive(Debug, PartialEq, Eq)]
enum Want {
    /// A plain shell: the tab doesn't keep sessions.
    Plain,
    /// The session it showed before (a reconnect), or was opened for.
    Known(SessionName),
    /// A session of this server's, the first free one.
    Fresh(ServerKey),
}

#[derive(Default)]
struct Registry {
    /// Tabs that keep their terminals' sessions, with their server's key.
    tabs: HashMap<String, ServerKey>,
    terminals: HashMap<String, Kept>,
    /// A note for a terminal about to open, taken by `early`.
    notes: HashMap<String, &'static str>,
}

impl Registry {
    fn want(&mut self, tab: &str, terminal: &str) -> Want {
        let keeps = self.tabs.get(tab);
        match self.terminals.get(terminal) {
            Some(kept) if kept.ended => Want::Plain,
            Some(kept) if kept.pinned || keeps.is_some() => Want::Known(kept.name.clone()),
            Some(_) => {
                self.terminals.remove(terminal);
                Want::Plain
            }
            None => keeps.cloned().map_or(Want::Plain, Want::Fresh),
        }
    }

    /// The slot a new terminal takes: of `key`'s names no other terminal
    /// here shows, the first with a session waiting (listed, shown nowhere),
    /// else the first with none.
    fn free_name(&self, key: &ServerKey, terminal: &str, probe: &Probe) -> Option<SessionName> {
        let free: Vec<SessionName> = (1..=SLOTS)
            .filter_map(|slot| SessionName::new(key, slot))
            .filter(|name| !self.terminals.iter().any(|(id, kept)| id != terminal && !kept.ended && kept.name == *name))
            .collect();
        let listed = |name: &SessionName| probe.sessions.iter().find(|l| l.name == *name);
        free.iter()
            .find(|name| listed(name).is_some_and(|l| !l.attached))
            .or_else(|| free.iter().find(|name| listed(name).is_none()))
            .cloned()
    }


    fn state(&self, terminal: &str) -> KeptState {
        self.terminals.get(terminal).filter(|k| !k.ended).map_or_else(KeptState::default, |k| KeptState { kept: true, running: k.running })
    }

    fn keep(&mut self, terminal: &str, kept: Kept) -> Result<(), String> {
        if self.terminals.len() >= MAX_TERMINALS && !self.terminals.contains_key(terminal) {
            return Err("Too many kept terminals are open".into());
        }
        self.terminals.insert(terminal.to_string(), kept);
        Ok(())
    }

    /// The tab's choice, and the terminals it has open (`open`): what its
    /// closed terminals showed is free again. With the choice off, only
    /// terminals opened from the sessions list stay kept.
    fn set_tab(&mut self, tab: &str, key: Option<ServerKey>, open: &[String]) -> Result<(), String> {
        match key {
            Some(key) => {
                if self.tabs.len() >= MAX_TABS && !self.tabs.contains_key(tab) {
                    return Err("Too many tabs keep their sessions".into());
                }
                self.tabs.insert(tab.to_string(), key);
            }
            None => {
                self.tabs.remove(tab);
            }
        }
        let keeps = self.tabs.contains_key(tab);
        self.terminals.retain(|id, kept| kept.tab != tab || (open.contains(id) && (keeps || kept.pinned)));
        Ok(())
    }
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

/// The registry. Never held across an await. A panic while it was held
/// leaves nothing half-written that matters, so a poisoned lock is used.
fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// A tab's session id, for any of its connections (`<tab>::term-…`).
fn tab_of(session_id: &str) -> &str {
    session_id.split_once("::").map_or(session_id, |(tab, _)| tab)
}

fn checked_id(id: &str) -> Result<&str, String> {
    if id.is_empty() || id.len() > MAX_ID {
        return Err("That isn't a tab or terminal".into());
    }
    Ok(id)
}

/// How a terminal's shell starts.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Start {
    /// The command it starts as, keeping its session; None: as it would.
    pub(crate) command: Option<String>,
    note: Option<&'static str>,
}

impl Start {
    /// `command`, else `otherwise` (the shell it would start).
    pub(crate) fn command_or(&self, otherwise: Option<String>) -> Option<String> {
        self.command.clone().or(otherwise)
    }
}

/// What the terminal shows before its shell's output: a note when it was to
/// be kept and can't be. Once.
pub(crate) fn early(terminal_id: &str) -> Vec<u8> {
    registry().notes.remove(terminal_id).map(|n| n.as_bytes().to_vec()).unwrap_or_default()
}

/// What `terminal` shows, now that the server answered `probe` (None: it
/// didn't), recorded for its next open. Atomic with the name's choice, so
/// two terminals opening at once never take the same one.
fn decide(reg: &mut Registry, tab: &str, terminal: &str, want: Want, probe: Option<&Probe>) -> Start {
    let note = |note| Start { command: None, note: Some(note) };
    let Some(probe) = probe else {
        reg.terminals.remove(terminal);
        return note(NOTE_UNKNOWN);
    };
    let Some(tool) = probe.tool else {
        reg.terminals.remove(terminal);
        return note(NOTE_NEITHER);
    };
    let pinned = reg.terminals.get(terminal).is_some_and(|k| k.pinned);
    let name = match want {
        Want::Plain => return Start::default(),
        Want::Known(name) => {
            // Ended since it was shown (`exit`, End): a plain shell, not a
            // session made anew under its name.
            if probe.complete && !probe.sessions.iter().any(|l| l.name == name) {
                if let Some(kept) = reg.terminals.get_mut(terminal) {
                    kept.ended = true;
                    kept.running = false;
                }
                return note(NOTE_ENDED);
            }
            name
        }
        Want::Fresh(key) => match reg.free_name(&key, terminal, probe) {
            Some(name) => name,
            None => return note(NOTE_FULL),
        },
    };
    let listed = probe.sessions.iter().find(|l| l.name == name);
    // An answer cut short may have left it out: taken as running, so no
    // on-connect command is typed into what may be.
    let running = listed.is_some() || !probe.complete;
    let command = attach_command(tool, &name, listed);
    match reg.keep(terminal, Kept { tab: tab.to_string(), name, running, pinned, ended: false }) {
        Ok(()) => Start { command: Some(command), note: None },
        Err(_) => note(NOTE_FULL),
    }
}

/// How `terminal`, opening on `session_id`'s connection, starts its shell:
/// in its kept session where its tab keeps them, asking the server first. A
/// plain shell, with no exec at all, where it doesn't. The page hears
/// whether it's kept (`KEPT_EVENT`).
pub(crate) async fn start(app: &tauri::AppHandle, session: &crate::SessionHandleArc, session_id: &str, terminal_id: &str) -> Start {
    let tab = tab_of(session_id);
    let want = registry().want(tab, terminal_id);
    let start = if want == Want::Plain {
        Start::default()
    } else {
        let out = crate::exec_with_stdin_capture_upto(session, &probe_command(), None, PROBE_SECS, PROBE_MAX_BYTES).await;
        let probe = out.ok().as_deref().and_then(parse_probe);
        let mut reg = registry();
        let start = decide(&mut reg, tab, terminal_id, want, probe.as_ref());
        reg.notes.remove(terminal_id);
        if let Some(note) = start.note {
            if reg.notes.len() < MAX_TERMINALS {
                reg.notes.insert(terminal_id.to_string(), note);
            }
        }
        start
    };
    tell_page(app, terminal_id, start.command.is_some());
    start
}

/// What the page hears when a terminal turns out kept or not: its mark.
const KEPT_EVENT: &str = "keep-session";

fn tell_page(app: &tauri::AppHandle, terminal_id: &str, kept: bool) {
    use tauri::Emitter;
    let _ = app.emit(KEPT_EVENT, serde_json::json!({ "terminalId": terminal_id, "kept": kept }));
}

/// This tab's choice (`on`, for the server with sync id `server_uuid`), and
/// the terminals it has open. Sent when it opens, when either changes, and
/// with `on` false and no terminals when it closes.
#[tauri::command]
pub(crate) fn keep_session_tab(session_id: String, server_uuid: Option<String>, on: bool, terminals: Vec<String>) -> Result<(), String> {
    let tab = checked_id(&session_id)?;
    if terminals.len() > MAX_TAB_TERMINALS || terminals.iter().any(|t| checked_id(t).is_err()) {
        return Err("Too many terminals".into());
    }
    let key = if on { server_uuid.as_deref().and_then(ServerKey::of) } else { None };
    registry().set_tab(tab, key, &terminals)
}

/// A new terminal of this tab, about to open, shows the session `name`
/// (from the sessions list).
#[tauri::command]
pub(crate) fn keep_session_open(session_id: String, terminal_id: String, name: String) -> Result<(), String> {
    let tab = checked_id(&session_id)?;
    let terminal = checked_id(&terminal_id)?;
    let name = SessionName::parse(&name).ok_or("That isn't one of Submarine's sessions")?;
    registry().keep(terminal, Kept { tab: tab.to_string(), name, running: false, pinned: true, ended: false })
}

/// Whether a terminal is kept now, and whether, when it last opened, it
/// picked up a session that was running: its on-connect commands aren't
/// typed into it then.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub(crate) struct KeptState {
    kept: bool,
    running: bool,
}

#[tauri::command]
pub(crate) fn keep_session_state(terminal_id: String) -> KeptState {
    registry().state(&terminal_id)
}

/// A session in the sessions list.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct SessionRow {
    name: SessionName,
    pid: Option<u32>,
    created: Option<u64>,
    attached: bool,
    /// This server's (not another saved server's on the same machine), and
    /// its terminal's number.
    ours: bool,
    terminal: u8,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct SessionList {
    /// None: the server has neither tmux nor screen.
    tool: Option<Tool>,
    sessions: Vec<SessionRow>,
}

fn session_list(probe: Probe, key: Option<&ServerKey>) -> SessionList {
    let sessions = probe
        .sessions
        .into_iter()
        .map(|l| SessionRow {
            ours: key.is_some_and(|k| l.name.key() == k.0),
            terminal: l.name.slot(),
            name: l.name,
            pid: l.pid,
            created: l.created,
            attached: l.attached,
        })
        .collect();
    SessionList { tool: probe.tool, sessions }
}

async fn connection(state: &SshState, session_id: &str) -> Result<crate::SessionHandleArc, String> {
    let tab = tab_of(checked_id(session_id)?);
    state.connections.lock().await.get(tab).cloned().ok_or_else(|| "Connect to the server first".to_string())
}

/// The kept sessions running on the tab's server.
#[tauri::command]
pub(crate) async fn keep_session_list(state: tauri::State<'_, SshState>, session_id: String, server_uuid: Option<String>) -> Result<SessionList, String> {
    let session = connection(&state, &session_id).await?;
    let out = crate::exec_with_stdin_capture_upto(&session, &probe_command(), None, PROBE_SECS, PROBE_MAX_BYTES).await?;
    let probe = parse_probe(&out).ok_or("The server didn't say which sessions it has")?;
    Ok(session_list(probe, server_uuid.as_deref().and_then(ServerKey::of).as_ref()))
}

/// Ends a kept session for good: what runs in it stops.
#[tauri::command]
pub(crate) async fn keep_session_end(state: tauri::State<'_, SshState>, session_id: String, tool: Tool, name: String, pid: Option<u32>) -> Result<(), String> {
    let name = SessionName::parse(&name).ok_or("That isn't one of Submarine's sessions")?;
    let command = end_command(tool, &name, pid)?;
    let session = connection(&state, &session_id).await?;
    crate::exec_with_stdin_capture_upto(&session, &command, None, PROBE_SECS, 4096).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork_hooks::assert_hooks;

    fn key() -> ServerKey {
        ServerKey::of("6f1c2a52-8b9e-4c1f-9d3a-0e5b7c4d2f10").unwrap()
    }

    fn name(slot: u8) -> SessionName {
        SessionName::new(&key(), slot).unwrap()
    }

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod keep_session;", 1),
                ("run", "keep_session::keep_session_tab", 1),
                ("open_terminal", "keep_session::start(", 1),
                ("open_terminal", "kept.command_or(", 1),
                ("open_terminal", "keep_session::early(&terminal_id)", 1),
            ],
        );
    }

    #[test]
    fn a_name_comes_from_the_servers_sync_id() {
        let n = name(1);
        // The same id makes the same name, on any computer.
        assert_eq!(n, SessionName::new(&ServerKey::of(" 6f1c2a52-8b9e-4c1f-9d3a-0e5b7c4d2f10 ").unwrap(), 1).unwrap());
        assert_eq!(n.0.len(), PREFIX.len() + KEY_LEN + 2);
        assert!(n.0.starts_with("submarine-") && n.0.ends_with("-1"));
        assert_eq!(SessionName::parse(&n.0), Some(n.clone()));
        assert_eq!((n.key(), n.slot()), (&key().0[..], 1));
        assert_ne!(key(), ServerKey::of("another server").unwrap());
        // Whatever the id holds, only hex digits reach a name.
        assert!(ServerKey::of("'; rm -rf / #\n$(reboot)").unwrap().0.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(ServerKey::of(""), None);
        assert_eq!(ServerKey::of("   "), None);
        assert_eq!(ServerKey::of(&"a".repeat(MAX_UUID + 1)), None);
        assert_eq!(SessionName::new(&key(), 0), None);
        assert_eq!(SessionName::new(&key(), 17), None);
        assert_eq!(name(9).slot(), 9);
        assert_eq!(name(16).slot(), 16);
        assert!(name(16).0.ends_with("-16"));
    }

    #[test]
    fn only_a_name_of_exactly_that_shape_is_one() {
        assert!(SessionName::parse("submarine-0123abcd-5").is_some());
        assert!(SessionName::parse("submarine-0123abcd-10").is_some());
        assert!(SessionName::parse("submarine-0123abcd-16").is_some());
        for bad in [
            "",
            "submarine-",
            "submarine-0123abcd",
            "submarine-0123abcd-",
            "submarine-0123abcd-0",
            "submarine-0123abcd-01",
            "submarine-0123abcd-17",
            "submarine-0123abcd-99",
            "submarine-0123abcd-100",
            "submarine-0123abcd-+1",
            "submarine-0123ABCD-1",
            "submarine-0123abcg-1",
            "submarine-0123abc-1",
            "submarine-0123abcde-1",
            "Submarine-0123abcd-1",
            "submarine-0123abcd-1 ",
            " submarine-0123abcd-1",
            "submarine-0123abcd-1;reboot",
            "submarine-0123abcd_1",
            "submarine-0123abcd-١",
            "submarine-é123abcd-1",
            "mine",
        ] {
            assert_eq!(SessionName::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_commands_are_exactly_these() {
        let n = SessionName::parse("submarine-0123abcd-2").unwrap();
        let running = Listed { name: n.clone(), pid: Some(4242), created: None, attached: false };
        assert_eq!(attach_command(Tool::Tmux, &n, None), "tmux -u new-session -A -s 'submarine-0123abcd-2'");
        assert_eq!(attach_command(Tool::Tmux, &n, Some(&running)), "tmux -u new-session -A -s 'submarine-0123abcd-2'");
        assert_eq!(attach_command(Tool::Screen, &n, None), "screen -U -S 'submarine-0123abcd-2'");
        assert_eq!(attach_command(Tool::Screen, &n, Some(&running)), "screen -U -x '4242.submarine-0123abcd-2'");
        assert_eq!(end_command(Tool::Tmux, &n, None).unwrap(), "tmux kill-session -t 'submarine-0123abcd-2'");
        assert_eq!(end_command(Tool::Screen, &n, Some(4242)).unwrap(), "screen -S '4242.submarine-0123abcd-2' -X quit");
        assert!(end_command(Tool::Screen, &n, None).is_err());
    }

    #[test]
    fn the_probe_travels_as_octal_only() {
        let cmd = probe_command();
        assert!(cmd.starts_with("sh -c "));
        // Nothing of the script itself for the account's shell to read.
        assert!(!cmd.contains("tmux") && !cmd.contains('#') && !cmd.contains('\n'));
    }

    #[test]
    fn a_tmux_listing_is_read() {
        let out = "motd from .bashrc\n__SUB_KEEP:tmux\nsubmarine-0123abcd-1 1762500000 1\nsubmarine-0123abcd-2 1762500100 0\nwork 1762500200 0\n__SUB_KEEP:end\n";
        let p = parse_probe(out).unwrap();
        assert_eq!(p.tool, Some(Tool::Tmux));
        assert!(p.complete);
        assert_eq!(
            p.sessions,
            vec![
                Listed { name: SessionName::parse("submarine-0123abcd-1").unwrap(), pid: None, created: Some(1762500000), attached: true },
                Listed { name: SessionName::parse("submarine-0123abcd-2").unwrap(), pid: None, created: Some(1762500100), attached: false },
            ]
        );
        // None running: tmux printed nothing.
        let p = parse_probe("__SUB_KEEP:tmux\r\n__SUB_KEEP:end\r\n").unwrap();
        assert_eq!((p.tool, p.sessions.len(), p.complete), (Some(Tool::Tmux), 0, true));
    }

    #[test]
    fn a_screen_listing_is_read() {
        let out = "__SUB_KEEP:screen\nThere are screens on:\n\t4242.submarine-0123abcd-1\t(11/07/2026 10:00:01 AM)\t(Detached)\n\t4243.submarine-0123abcd-2\t(Attached)\n\t4244.submarine-0123abcd-3\t(Multi, attached)\n\t99.pts-0.host\t(Detached)\n\t4245.submarine-0123abcd-4\t(Dead ???)\n3 Sockets in /run/screen/S-me.\n__SUB_KEEP:end\n";
        let p = parse_probe(out).unwrap();
        assert_eq!(p.tool, Some(Tool::Screen));
        let got: Vec<(String, Option<u32>, bool)> = p.sessions.iter().map(|l| (l.name.0.clone(), l.pid, l.attached)).collect();
        assert_eq!(
            got,
            vec![
                ("submarine-0123abcd-1".into(), Some(4242), false),
                ("submarine-0123abcd-2".into(), Some(4243), true),
                ("submarine-0123abcd-3".into(), Some(4244), true),
            ]
        );
        assert!(p.sessions.iter().all(|l| l.created.is_none()));
        // As screen 4.09 writes it: its own lines end in CR LF.
        let real = "__SUB_KEEP:screen\nThere are screens on:\r\n\t1813.submarine-0123abcd-1\t(11/07/26 18:44:53)\t(Attached)\n\t1815.submarine-0123abcd-12\t(11/07/26 18:44:53)\t(Detached)\n2 Sockets in /run/screen/S-root.\r\n__SUB_KEEP:end\n";
        let p = parse_probe(real).unwrap();
        assert_eq!(p.sessions.iter().map(|l| (l.name.0.as_str(), l.pid, l.attached)).collect::<Vec<_>>(), vec![("submarine-0123abcd-1", Some(1813), true), ("submarine-0123abcd-12", Some(1815), false)]);
        let p = parse_probe("__SUB_KEEP:screen\nNo Sockets found in /run/screen/S-me.\n\n__SUB_KEEP:end\n").unwrap();
        assert_eq!((p.sessions.len(), p.complete), (0, true));
    }

    #[test]
    fn neither_and_no_answer() {
        let p = parse_probe("__SUB_KEEP:none\n__SUB_KEEP:end\n").unwrap();
        assert_eq!((p.tool, p.sessions.len(), p.complete), (None, 0, true));
        assert_eq!(parse_probe(""), None);
        assert_eq!(parse_probe("'sh' is not recognized as an internal or external command\r\n"), None);
        // The end before any tool isn't an answer.
        assert_eq!(parse_probe("__SUB_KEEP:end\n"), None);
    }

    #[test]
    fn odd_or_hostile_lines_are_passed_over() {
        let long = format!("submarine-0123abcd-1 1 0{}", " ".repeat(MAX_LINE));
        let out = format!(
            "__SUB_KEEP:tmux\n\
             submarine-0123abcd-1 x 0\n\
             submarine-0123abcd-1 1 -1\n\
             submarine-0123abcd-1 1 0 extra\n\
             submarine-0123abcd-1;reboot 1 0\n\
             submarine-0123abcd-1 99999999999999999999999 0\n\
             \x1b]0;title\x07 1 0\n\
             {long}\n\
             __SUB_KEEP:screen\n\
             submarine-0123abcd-2 2 1\n\
             __SUB_KEEP:end\n\
             submarine-0123abcd-3 3 0\n"
        );
        let p = parse_probe(&out).unwrap();
        // The second tool marker is passed over; the line after it is still
        // read as tmux's, and nothing after the end is.
        assert_eq!(p.tool, Some(Tool::Tmux));
        assert_eq!(p.sessions.iter().map(|l| l.name.0.as_str()).collect::<Vec<_>>(), vec!["submarine-0123abcd-2"]);
        // screen lines: a pid that isn't one, a name with a dot, no status.
        for bad in [
            "\tx.submarine-0123abcd-1\t(Detached)",
            "\t.submarine-0123abcd-1\t(Detached)",
            "\t12345678901.submarine-0123abcd-1\t(Detached)",
            "\t1.submarine-0123abcd-1.x\t(Detached)",
            "\t1.submarine-0123abcd-1",
            "\t1.submarine-0123abcd-1\ta\tb\t(Detached)",
            "1.submarine-0123abcd-1\t(Detached)",
        ] {
            assert_eq!(parse_screen_line(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_huge_answer_stays_bounded() {
        let mut out = String::from("__SUB_KEEP:tmux\n");
        for i in 0..10_000 {
            out.push_str(&format!("submarine-0123abcd-{} {i} 0\n", 1 + i % 16));
        }
        // Cut short, as the capture's limit would: no end line.
        let p = parse_probe(&out).unwrap();
        assert_eq!(p.sessions.len(), MAX_LISTED);
        assert!(!p.complete);
    }

    fn probe(tool: Option<Tool>, running: &[&SessionName], complete: bool) -> Probe {
        let sessions = running.iter().map(|n| Listed { name: (*n).clone(), pid: Some(7), created: None, attached: false }).collect();
        Probe { tool, sessions, complete }
    }

    #[test]
    fn a_tab_that_doesnt_keep_sessions_starts_a_plain_shell() {
        let mut reg = Registry::default();
        assert_eq!(reg.want("s1", "s1-term-0"), Want::Plain);
        reg.set_tab("s1", ServerKey::of(""), &[]).unwrap();
        assert_eq!(reg.want("s1", "s1-term-0"), Want::Plain);
    }

    #[test]
    fn terminals_take_the_first_free_names_and_keep_them() {
        let mut reg = Registry::default();
        let open = ["s1-term-0".to_string(), "s1-term-1".to_string()];
        reg.set_tab("s1", Some(key()), &open).unwrap();
        let p = probe(Some(Tool::Tmux), &[], true);
        let want = reg.want("s1", "s1-term-0");
        assert_eq!(want, Want::Fresh(key()));
        let a = decide(&mut reg, "s1", "s1-term-0", want, Some(&p));
        assert_eq!(a.command.as_deref(), Some(&*format!("tmux -u new-session -A -s '{}'", name(1))));
        assert!(a.note.is_none());
        let want = reg.want("s1", "s1-term-1");
        let b = decide(&mut reg, "s1", "s1-term-1", want, Some(&p));
        assert_eq!(b.command.as_deref(), Some(&*format!("tmux -u new-session -A -s '{}'", name(2))));
        // A reconnect: the same names again, whatever else is free.
        assert_eq!(reg.want("s1", "s1-term-1"), Want::Known(name(2)));
        // Another tab of the same server takes the next.
        reg.set_tab("s2", Some(key()), &["s2-term-0".into()]).unwrap();
        let want = reg.want("s2", "s2-term-0");
        let c = decide(&mut reg, "s2", "s2-term-0", want, Some(&p));
        assert!(c.command.unwrap().ends_with(&format!("'{}'", name(3))));
        // A closed terminal's name is free again.
        reg.set_tab("s1", Some(key()), &open[..1]).unwrap();
        reg.set_tab("s1", Some(key()), &["s1-term-0".into(), "s1-term-9".into()]).unwrap();
        let want = reg.want("s1", "s1-term-9");
        let d = decide(&mut reg, "s1", "s1-term-9", want, Some(&p));
        assert!(d.command.unwrap().ends_with(&format!("'{}'", name(2))));
    }

    #[test]
    fn a_waiting_session_is_picked_up_first_and_one_shown_elsewhere_is_left() {
        let mut reg = Registry::default();
        reg.set_tab("s1", Some(key()), &["s1-term-0".into(), "s1-term-1".into()]).unwrap();
        // 1 is shown on another computer, 3 waits, 2 isn't there.
        let mut p = probe(Some(Tool::Tmux), &[&name(1), &name(3)], true);
        p.sessions[0].attached = true;
        let want = reg.want("s1", "s1-term-0");
        let a = decide(&mut reg, "s1", "s1-term-0", want, Some(&p));
        assert!(a.command.unwrap().ends_with(&format!("'{}'", name(3))));
        assert!(reg.terminals["s1-term-0"].running);
        // Nothing else waits: the first slot with no session, made new.
        let want = reg.want("s1", "s1-term-1");
        let b = decide(&mut reg, "s1", "s1-term-1", want, Some(&p));
        assert!(b.command.unwrap().ends_with(&format!("'{}'", name(2))));
        assert!(!reg.terminals["s1-term-1"].running);
    }

    #[test]
    fn sixteen_at_most() {
        let mut reg = Registry::default();
        let open: Vec<String> = (0..17).map(|i| format!("s1-term-{i}")).collect();
        reg.set_tab("s1", Some(key()), &open).unwrap();
        let p = probe(Some(Tool::Tmux), &[], true);
        for t in &open[..16] {
            let want = reg.want("s1", t);
            assert!(decide(&mut reg, "s1", t, want, Some(&p)).command.is_some());
        }
        assert_eq!(reg.terminals["s1-term-15"].name, name(16));
        let want = reg.want("s1", &open[16]);
        let last = decide(&mut reg, "s1", &open[16], want, Some(&p));
        assert_eq!((last.command, last.note), (None, Some(NOTE_FULL)));
    }

    #[test]
    fn on_connect_commands_go_only_into_a_new_session() {
        let mut reg = Registry::default();
        reg.set_tab("s1", Some(key()), &["s1-term-0".into()]).unwrap();
        let t = "s1-term-0";
        // New: made, so the commands are typed.
        let want = reg.want("s1", t);
        decide(&mut reg, "s1", t, want, Some(&probe(Some(Tool::Tmux), &[], true)));
        assert!(!reg.terminals[t].running);
        // A reconnect finds it running: they aren't.
        let want = reg.want("s1", t);
        decide(&mut reg, "s1", t, want, Some(&probe(Some(Tool::Tmux), &[&name(1)], true)));
        assert!(reg.terminals[t].running);
        assert_eq!(keep_state(&reg, t), KeptState { kept: true, running: true });
        // An answer cut short can't say: taken as running.
        let want = reg.want("s1", t);
        decide(&mut reg, "s1", t, want, Some(&probe(Some(Tool::Tmux), &[], false)));
        assert!(reg.terminals[t].running);
        // Another server's session of the same number isn't this one.
        let mut reg = Registry::default();
        reg.set_tab("s1", Some(key()), &[t.into()]).unwrap();
        let other = SessionName::new(&ServerKey::of("other").unwrap(), 1).unwrap();
        let want = reg.want("s1", t);
        decide(&mut reg, "s1", t, want, Some(&probe(Some(Tool::Tmux), &[&other], true)));
        assert_eq!(reg.terminals[t].name, name(1));
        assert!(!reg.terminals[t].running);
    }

    fn keep_state(reg: &Registry, t: &str) -> KeptState {
        reg.state(t)
    }

    #[test]
    fn a_session_ended_with_exit_is_never_made_anew() {
        let mut reg = Registry::default();
        reg.set_tab("s1", Some(key()), &["s1-term-0".into(), "s1-term-1".into()]).unwrap();
        let t = "s1-term-0";
        let want = reg.want("s1", t);
        decide(&mut reg, "s1", t, want, Some(&probe(Some(Tool::Tmux), &[], true)));
        // `exit` ended it; the connection then dropped, and the terminal
        // opens again: its session is gone, so a plain shell, said once.
        let want = reg.want("s1", t);
        assert_eq!(want, Want::Known(name(1)));
        let s = decide(&mut reg, "s1", t, want, Some(&probe(Some(Tool::Tmux), &[], true)));
        assert_eq!((s.command, s.note), (None, Some(NOTE_ENDED)));
        assert_eq!(keep_state(&reg, t), KeptState::default());
        // And every time after: no exec at all.
        assert_eq!(reg.want("s1", t), Want::Plain);
        // Its slot is free for a new terminal.
        let want = reg.want("s1", "s1-term-1");
        let n = decide(&mut reg, "s1", "s1-term-1", want, Some(&probe(Some(Tool::Tmux), &[], true)));
        assert!(n.command.unwrap().ends_with(&format!("'{}'", name(1))));
    }

    #[test]
    fn screen_attaches_by_process_id_when_running() {
        let mut reg = Registry::default();
        reg.set_tab("s1", Some(key()), &["s1-term-0".into(), "s1-term-1".into()]).unwrap();
        let want = reg.want("s1", "s1-term-0");
        let s = decide(&mut reg, "s1", "s1-term-0", want, Some(&probe(Some(Tool::Screen), &[&name(1)], true)));
        assert_eq!(s.command.unwrap(), format!("screen -U -x '7.{}'", name(1)));
        let want = reg.want("s1", "s1-term-1");
        let s = decide(&mut reg, "s1", "s1-term-1", want, Some(&probe(Some(Tool::Screen), &[&name(1)], true)));
        assert_eq!(s.command.unwrap(), format!("screen -U -S '{}'", name(2)));
    }

    #[test]
    fn neither_or_no_answer_is_a_plain_shell_with_a_note() {
        let mut reg = Registry::default();
        reg.set_tab("s1", Some(key()), &["s1-term-0".into()]).unwrap();
        let want = reg.want("s1", "s1-term-0");
        let s = decide(&mut reg, "s1", "s1-term-0", want, Some(&probe(None, &[], true)));
        assert_eq!((s.command_or(None), s.note), (None, Some(NOTE_NEITHER)));
        assert!(!reg.terminals.contains_key("s1-term-0"));
        let want = reg.want("s1", "s1-term-0");
        let s = decide(&mut reg, "s1", "s1-term-0", want, None);
        assert_eq!(s.note, Some(NOTE_UNKNOWN));
        // The login shell it would start is kept then.
        assert_eq!(s.command_or(Some("login".into())).as_deref(), Some("login"));
    }

    #[test]
    fn turning_it_off_leaves_only_what_was_opened_from_the_list() {
        let mut reg = Registry::default();
        let open = ["s1-term-0".to_string(), "s1-term-1".to_string()];
        reg.set_tab("s1", Some(key()), &open).unwrap();
        let picked = SessionName::parse("submarine-0123abcd-4").unwrap();
        let p = probe(Some(Tool::Tmux), &[&picked], true);
        let want = reg.want("s1", "s1-term-0");
        decide(&mut reg, "s1", "s1-term-0", want, Some(&p));
        keep_into(&mut reg, "s1", "s1-term-1", &picked);
        reg.set_tab("s1", None, &open).unwrap();
        assert_eq!(reg.want("s1", "s1-term-0"), Want::Plain);
        assert_eq!(reg.want("s1", "s1-term-1"), Want::Known(picked.clone()));
        let s = decide(&mut reg, "s1", "s1-term-1", Want::Known(picked.clone()), Some(&p));
        assert_eq!(s.command.unwrap(), "tmux -u new-session -A -s 'submarine-0123abcd-4'");
        assert!(reg.terminals["s1-term-1"].pinned && reg.terminals["s1-term-1"].running);
        // The tab closes: nothing of it is kept.
        reg.set_tab("s1", None, &[]).unwrap();
        assert!(reg.terminals.is_empty() && reg.tabs.is_empty());
    }

    fn keep_into(reg: &mut Registry, tab: &str, terminal: &str, name: &SessionName) {
        reg.keep(terminal, Kept { tab: tab.into(), name: name.clone(), running: false, pinned: true, ended: false }).unwrap();
    }

    #[test]
    fn what_the_page_sends_is_checked() {
        assert!(keep_session_open("s-x".into(), "s-x-term-1".into(), "submarine-0123abcd-1;reboot".into()).is_err());
        assert!(keep_session_open(String::new(), "t".into(), "submarine-0123abcd-1".into()).is_err());
        assert!(keep_session_open("s".into(), "t".repeat(MAX_ID + 1), "submarine-0123abcd-1".into()).is_err());
        assert!(keep_session_tab("s-y".into(), Some("u".into()), true, vec![String::new()]).is_err());
        assert!(keep_session_tab("s-y".into(), Some("u".into()), true, vec!["t".into(); MAX_TAB_TERMINALS + 1]).is_err());
        assert_eq!(keep_session_state("nothing-like-it".into()), KeptState::default());
        assert_eq!(tab_of("s1::term-s1-term-3"), "s1");
        assert_eq!(tab_of("s1"), "s1");
    }

    #[test]
    fn the_registry_is_bounded() {
        let mut reg = Registry::default();
        for i in 0..MAX_TABS {
            reg.set_tab(&format!("s{i}"), Some(key()), &[]).unwrap();
        }
        assert!(reg.set_tab("one-more", Some(key()), &[]).is_err());
        // One already there can still change.
        reg.set_tab("s0", Some(key()), &[]).unwrap();
        for i in 0..MAX_TERMINALS {
            keep_into(&mut reg, "s0", &format!("t{i}"), &name(1));
        }
        assert!(reg.keep("one-more", Kept { tab: "s0".into(), name: name(1), running: false, pinned: true, ended: false }).is_err());
    }

    #[test]
    fn a_note_is_shown_once() {
        registry().notes.insert("note-term".into(), NOTE_NEITHER);
        assert_eq!(early("note-term"), NOTE_NEITHER.as_bytes());
        assert!(early("note-term").is_empty());
    }

    #[test]
    fn the_list_marks_this_servers_sessions() {
        let other = SessionName::new(&ServerKey::of("other").unwrap(), 4).unwrap();
        let list = session_list(probe(Some(Tool::Tmux), &[&name(2), &other], true), Some(&key()));
        assert_eq!(list.tool, Some(Tool::Tmux));
        assert_eq!(list.sessions.iter().map(|r| (r.ours, r.terminal)).collect::<Vec<_>>(), vec![(true, 2), (false, 4)]);
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json["tool"], "tmux");
        assert_eq!(json["sessions"][0]["name"], name(2).0);
        assert_eq!(serde_json::to_value(session_list(probe(None, &[], true), None)).unwrap()["tool"], serde_json::Value::Null);
    }
}
