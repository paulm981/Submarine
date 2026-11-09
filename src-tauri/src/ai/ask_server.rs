//! The AI asks for a server by name (#32, "asking for a server by name"):
//! `request_server`, and the card in the AI panel that answers it.
//!
//! - **Only an exact name, ignoring case, finds a saved SSH server**
//!   (`Lookup`), read from the vault off the async workers. The AI never
//!   gets a list of your servers, and there's no searching by part of a
//!   name. An RDP, VNC or telnet server isn't found: it can't be shared.
//! - **Every name gets a card,** one that matches no server too ("no server
//!   has that name"), and the AI hears back only once you've answered it,
//!   in the same words whether you said no or there was nothing to share
//!   (`NOT_SHARED`). So it can't tell a name you have from one you don't,
//!   and its probing shows up on your screen.
//! - **Share** opens the server's tab (the page does that, through the
//!   session tree, which holds back a server a teammate moved), waits for it
//!   to connect (CONNECT_WAIT), and shares it as Share with AI does: its
//!   own approval level applies from then on. While it connects, the card
//!   stays, marked so (`connecting`), and Stop, Don't share, the AI
//!   withdrawing or its access ending all still win: it's shared only if
//!   the request is still there once the tab is up.
//! - **Limits:** one such request waits at a time for each AI tab, counted
//!   against its rate and its waiting requests as a command is; a name
//!   longer than MAX_NAME, or with a control character, is refused.
//! - **Logged** in the audit log: the request, and your answer.

use super::*;
use std::time::Duration;

/// The longest name the AI may ask for, in characters.
pub(crate) static MAX_NAME: usize = 128;
/// How long a server you said yes to has to connect.
pub(crate) static CONNECT_WAIT: Duration = Duration::from_secs(90);
/// What the AI hears when it asks again while a request waits.
static ONE_AT_A_TIME: &str = "A request for a server already waits for the user. Wait for it first.";
/// What the AI hears when nothing is shared: you said no, or no server has
/// the name. The same words for both.
pub(crate) static NOT_SHARED: &str = "The user didn't share a server by that name. Ask them to share it from its tab, or use one list_shared names.";

/// What a name finds among the saved servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// One saved server has that name: its local id and its name as saved.
    One(i64, String),
    /// None has.
    None,
    /// More than one has: the card says so, and you share the one you mean
    /// from its tab.
    Many,
}

/// Finds a saved server by its name: the vault in the app, a list in tests.
pub(crate) type Lookup = Arc<dyn Fn(&str) -> Found + Send + Sync>;

/// The saved SSH servers named `name`, ignoring case in any alphabet
/// (`ÉCOLE` finds `école`, which SQLite's `lower` wouldn't): exact names
/// only, not one deleted, nor an RDP, VNC or telnet server.
pub(crate) fn find(conn: &rusqlite::Connection, name: &str) -> Found {
    let want = name.to_lowercase();
    let ids: Vec<(i64, String)> = conn
        .prepare("SELECT id, name FROM servers WHERE deleted = 0 AND (protocol IS NULL OR protocol IN ('', 'ssh'))")
        .and_then(|mut st| {
            st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))?
                .filter_map(Result::ok)
                .filter(|(_, saved)| saved.to_lowercase() == want)
                .take(2)
                .map(Ok)
                .collect()
        })
        .unwrap_or_default();
    match ids.as_slice() {
        [(id, saved)] => Found::One(*id, saved.clone()),
        [] => Found::None,
        _ => Found::Many,
    }
}

/// The name as the AI gave it, checked: there, trimmed, not too long, and
/// with no control or hidden characters (they'd hide what the card says).
pub(crate) fn check_name(name: Option<&serde_json::Value>) -> Result<String, String> {
    let name = match name {
        Some(serde_json::Value::String(s)) => s.trim(),
        None | Some(serde_json::Value::Null) => return Err("name is missing: the server's name, as the user calls it.".into()),
        Some(_) => return Err("name must be the server's name, as text.".into()),
    };
    if name.is_empty() {
        return Err("name is empty.".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("name is longer than {MAX_NAME} characters."));
    }
    if name.chars().any(|c| c.is_control() || super::trust::hidden(c)) {
        return Err("name has a control or hidden character in it.".into());
    }
    Ok(name.to_string())
}

/// What you said on the card.
#[derive(Debug)]
pub(crate) enum Answer {
    /// It's shared now, under this name.
    Shared(String),
    /// You said yes, but it couldn't be shared: why, for the AI.
    Failed(String),
    /// You said no, or dismissed a card with nothing to share.
    No,
}

/// A request for a server, waiting for you.
pub(crate) struct Ask {
    pub view: AskView,
    pub decide: oneshot::Sender<Answer>,
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct AskView {
    pub id: u64,
    pub tab_id: String,
    pub tab_label: String,
    /// The name as the AI asked for it.
    pub asked: String,
    /// The saved server it found, if exactly one: its local id and name.
    pub server_id: Option<i64>,
    pub server_name: Option<String>,
    /// More than one saved server has the name.
    pub many: bool,
    pub asked_at: String,
    /// You said Share, and its tab is connecting.
    pub connecting: bool,
}

impl AiState {
    /// Set how names are found: the app's vault, or a test's list.
    pub(crate) fn set_server_lookup(&self, lookup: Lookup) {
        *self.0.server_lookup.lock().unwrap_or_else(|e| e.into_inner()) = Some(lookup);
    }

    /// `request_server`: a card for the name, and the answer once you've
    /// given one.
    pub(crate) async fn request_server(&self, tab_id: &str, args: &serde_json::Value, mut cancel: watch::Receiver<bool>) -> ToolResult {
        let name = match check_name(args.get("name")) {
            Ok(n) => n,
            Err(why) => return ToolResult::err(why),
        };
        // One at a time (checked again as the card is added, below, as two
        // can come at once). A server already shared is found by its saved
        // id once it's looked up, not by the name it's shared as: a quick
        // connect called "db-01" isn't the saved db-01.
        {
            let s = self.lock();
            if !s.tabs.iter().any(|t| t.id == tab_id) {
                return ToolResult::err("This AI tab's access has ended.");
            }
            if s.server_asks.iter().any(|a| a.view.tab_id == tab_id) {
                return ToolResult::err(ONE_AT_A_TIME);
            }
        }
        let label = match self.admit_group(tab_id) {
            Ok(label) => label,
            Err((_, why)) => return ToolResult::err(why),
        };
        let lookup = self.0.server_lookup.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let found = {
            let n = name.clone();
            match lookup {
                Some(f) => tokio::task::spawn_blocking(move || f(&n)).await.unwrap_or(Found::None),
                None => Found::None,
            }
        };
        // A server shared already, by another name: it's that one.
        if let Found::One(id, _) = &found {
            let s = self.lock();
            if let Some(x) = s.tabs.iter().find(|t| t.id == tab_id).and_then(|t| t.servers.iter().find(|x| x.server_id == Some(*id))) {
                return ToolResult::ok(format!("It's already shared with you as {}.", x.name));
            }
        }
        let (tx, rx) = oneshot::channel();
        let view = {
            let mut s = self.lock();
            if !s.tabs.iter().any(|t| t.id == tab_id) {
                return ToolResult::err("This AI tab's access has ended.");
            }
            if s.server_asks.iter().any(|a| a.view.tab_id == tab_id) {
                return ToolResult::err(ONE_AT_A_TIME);
            }
            s.next_id += 1;
            let (server_id, server_name) = match &found {
                Found::One(id, saved) => (Some(*id), Some(saved.clone())),
                _ => (None, None),
            };
            let view = AskView {
                id: s.next_id,
                tab_id: tab_id.to_string(),
                tab_label: label.clone(),
                asked: name.clone(),
                server_id,
                server_name,
                many: found == Found::Many,
                asked_at: now(),
                connecting: false,
            };
            s.server_asks.push(Ask { view: view.clone(), decide: tx });
            view
        };
        let mut e = audit::Entry::new("server_asked", &label);
        e.server = Some(for_log(&name, MAX_LOGGED_NAME));
        e.note = Some(match &found {
            Found::One(..) => "a saved server has this name".into(),
            Found::None => "no saved server has this name".into(),
            Found::Many => "more than one saved server has this name".into(),
        });
        self.audit(e);
        if self.0.flash.load(std::sync::atomic::Ordering::Relaxed) {
            (self.0.attention)();
        }
        self.changed();
        let answer = tokio::select! {
            biased;
            a = rx => a.ok(),
            _ = withdrawal(&mut cancel) => {
                self.lock().server_asks.retain(|a| a.view.id != view.id);
                self.changed();
                return ToolResult::err("Cancelled before the user decided: the request was withdrawn.");
            }
        };
        match answer {
            Some(Answer::Shared(as_name)) => {
                ToolResult::ok(format!("The user shared it with you as {as_name}. Its own approval level applies; call list_shared to see it."))
            }
            Some(Answer::Failed(why)) => ToolResult::err(why),
            Some(Answer::No) => ToolResult::err(NOT_SHARED),
            // Dropped: stopped, or access ended.
            None => ToolResult::err("The request ended before a server was shared: the user stopped it, or this AI tab's access ended."),
        }
    }

    /// Take a waiting request off the list, for its answer.
    fn take_ask(&self, id: u64) -> Option<Ask> {
        let mut s = self.lock();
        let at = s.server_asks.iter().position(|a| a.view.id == id)?;
        Some(s.server_asks.remove(at))
    }

    fn log_answer(&self, view: &AskView, decision: &'static str, note: Option<String>) {
        let mut e = audit::Entry::new("server_asked", &view.tab_label);
        e.server = Some(for_log(view.server_name.as_deref().unwrap_or(&view.asked), MAX_LOGGED_NAME));
        e.decision = Some(decision);
        e.by = Some(BY_YOU);
        e.note = note;
        self.audit(e);
    }

    /// You said no, or dismissed a card with nothing to share.
    pub(crate) fn decline_ask(&self, id: u64) -> Result<(), String> {
        let ask = self.take_ask(id).ok_or("That request isn't waiting any more.")?;
        self.log_answer(&ask.view, "denied", None);
        let _ = ask.decide.send(Answer::No);
        self.changed();
        Ok(())
    }

    /// You said yes: once `connected` finds the server's tab connected (its
    /// session id, its login user), it's shared, and the AI hears so. The
    /// page has opened the tab by then, or opens it now. Until then the
    /// request stays on the list, marked connecting, so Stop, Don't share,
    /// the AI withdrawing or its access ending take it off as before; it's
    /// shared only if it's still there once the tab is up.
    pub(crate) async fn share_ask<F, Fut>(&self, id: u64, connected: F) -> Result<(), String>
    where
        F: Fn(i64) -> Fut,
        Fut: std::future::Future<Output = Option<(String, Option<String>)>>,
    {
        let view = {
            let mut s = self.lock();
            let ask = s.server_asks.iter_mut().find(|a| a.view.id == id && !a.view.connecting).ok_or("That request isn't waiting any more.")?;
            ask.view.connecting = true;
            ask.view.clone()
        };
        let (Some(server_id), Some(name)) = (view.server_id, view.server_name.clone()) else {
            // Nothing to share (no saved server, or several): as a no.
            if let Some(ask) = self.take_ask(id) {
                self.log_answer(&ask.view, "denied", Some("nothing to share".into()));
                let _ = ask.decide.send(Answer::No);
            }
            self.changed();
            return Err("There's no server to share for this request.".into());
        };
        self.changed();
        let started = tokio::time::Instant::now();
        let still_there = |s: &AiState| s.lock().server_asks.iter().any(|a| a.view.id == id && !a.decide.is_closed());
        let session = loop {
            if !still_there(self) {
                break None;
            }
            if let Some(found) = connected(server_id).await {
                break Some(found);
            }
            if started.elapsed() >= CONNECT_WAIT {
                break None;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        // Taken off the list now, and only if it's still there: whatever
        // ended it meanwhile wins.
        let Some(ask) = self.take_ask(id) else {
            self.log_answer(&view, "allowed", Some("ended before it was shared".into()));
            self.changed();
            return Err("That request ended before the server was shared.".into());
        };
        self.changed();
        if ask.decide.is_closed() {
            self.log_answer(&view, "allowed", Some("withdrawn before it was shared".into()));
            return Err("The AI withdrew the request before the server was shared.".into());
        }
        let Some((session_id, user)) = session else {
            let why = format!("{name} didn't connect within {} seconds, so it isn't shared.", CONNECT_WAIT.as_secs());
            self.log_answer(&view, "allowed", Some(why.clone()));
            let _ = ask.decide.send(Answer::Failed(why.clone()));
            return Err(why);
        };
        if let Err(why) = self.share_as(&view.tab_id, &session_id, &name, user, Some(server_id)) {
            let _ = ask.decide.send(Answer::Failed(why.clone()));
            return Err(why);
        }
        let as_name = self
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == view.tab_id)
            .and_then(|t| t.servers.iter().find(|x| x.session_id == session_id))
            .map(|x| x.name.clone())
            .unwrap_or(name);
        self.log_answer(&view, "allowed", Some(format!("shared as {as_name}")));
        let _ = ask.decide.send(Answer::Shared(as_name));
        Ok(())
    }
}

/// Your answer on a request for a server (mod.rs's ai_answer_server_ask):
/// share it, finding its tab among SshState's connections, or not.
pub(crate) async fn answer(state: &AiState, ssh: &crate::ssh_manager::SshState, id: u64, share: bool) -> Result<(), String> {
    if !share {
        return state.decline_ask(id);
    }
    state
        .share_ask(id, |server_id| async move {
            // The tab opened for that saved server, once it's connected.
            let sessions = ssh.session_servers.lock().await;
            let session = sessions.iter().find(|(sid, &sv)| i64::from(sv) == server_id && !sid.contains("::")).map(|(sid, _)| sid.clone())?;
            drop(sessions);
            if !ssh.connections.lock().await.contains_key(&session) {
                return None;
            }
            let user = ssh.session_users.lock().await.get(&session).cloned();
            Some((session, user))
        })
        .await
}

#[cfg(test)]
mod tests;
