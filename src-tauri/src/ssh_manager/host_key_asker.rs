//! The host-key questions waiting for an answer, one at a time (`HostKeyAsker`).
//!
//! The fork's own, moved here out of ssh_manager.rs (#136) so a sync with the
//! original project doesn't conflict on it. ssh_manager.rs brings it back in with
//! `use host_key_asker::*`.

use super::*;

/// One "trust this host key?" question at a time per host, port and key.
/// Opening a folder of servers behind one jump host — or several sessions
/// to the same host — put up the same prompt once per session; now the
/// first one asks and the rest take its answer. The value is `None` until
/// it's answered.
pub(crate) type HostKeyQuestion = (String, u16, String);
pub(crate) type AskedQuestions = HashMap<HostKeyQuestion, (u64, tokio::sync::watch::Receiver<Option<HostKeyAnswer>>)>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HostKeyAnswer {
    Accepted,
    Rejected,
    /// Nobody answered in time: the sessions waiting on it give up too,
    /// rather than putting the prompt up again one after another.
    Unanswered,
}
pub(crate) static HOST_KEY_QUESTIONS: std::sync::Mutex<Option<AskedQuestions>> = std::sync::Mutex::new(None);
pub(crate) static HOST_KEY_QUESTION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) enum HostKeyTurn {
    /// Another session is asking this: its answer comes through here.
    Follow(tokio::sync::watch::Receiver<Option<HostKeyAnswer>>),
    /// This session asks.
    Ask(HostKeyAsker),
}

/// The session asking a host-key question, until it's done (dropped).
pub(crate) struct HostKeyAsker {
    pub(crate) question: HostKeyQuestion,
    pub(crate) id: u64,
    pub(crate) tx: tokio::sync::watch::Sender<Option<HostKeyAnswer>>,
}

impl HostKeyAsker {
    pub(crate) fn answer(&self, answer: HostKeyAnswer) {
        let _ = self.tx.send(Some(answer));
    }
}

impl Drop for HostKeyAsker {
    fn drop(&mut self) {
        let mut guard = HOST_KEY_QUESTIONS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(map) = guard.as_mut() {
            if map.get(&self.question).is_some_and(|(id, _)| *id == self.id) {
                map.remove(&self.question);
            }
        }
    }
}

pub(crate) fn host_key_turn(question: HostKeyQuestion) -> HostKeyTurn {
    let mut guard = HOST_KEY_QUESTIONS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some((_, rx)) = map.get(&question) {
        return HostKeyTurn::Follow(rx.clone());
    }
    let (tx, rx) = tokio::sync::watch::channel(None);
    let id = HOST_KEY_QUESTION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    map.insert(question.clone(), (id, rx));
    HostKeyTurn::Ask(HostKeyAsker { question, id, tx })
}

/// The answer the asking session got: `None` when it went away without one
/// (its tab closed, or it was replaced) — then the next one asks.
pub(crate) async fn asked_answer(mut rx: tokio::sync::watch::Receiver<Option<HostKeyAnswer>>) -> Option<HostKeyAnswer> {
    let answered = rx.wait_for(|v| v.is_some()).await.map(|v| *v);
    answered.unwrap_or_else(|_| *rx.borrow())
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
/// check_server_key's log line, as its `log` does: printed, and shown in
/// the tab's log only while this attempt is the tab's current one.
fn log_line(h: &ClientHandler, msg: &str, ty: &str) {
    println!("[LOG-{}] {}", h.session_id, msg);
    if h.attempt.is_current_now() {
        let _ = h.app.emit(&format!("session-log-{}", h.session_id), serde_json::json!({"msg": msg, "type": ty}));
    }
}

/// A key the fork's trust policy vouches for (trust_policy.rs, #43): trusted
/// without a prompt, and, vouched for by known_hosts with nothing on file
/// here (`save`), saved as if approved, so Monitoring and the changed-key
/// warning know it from now on (#43, D4).
pub(crate) fn trusted_key(h: &ClientHandler, why: &str, save: bool, fp_str: &str, key_type: &str) -> Result<bool, russh::Error> {
    if save {
        let saved = match h.db.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(conn) => save_approved_host_key(conn, &h.server_host, h.server_port, fp_str, key_type, false).is_ok(),
                None => false,
            },
            Err(_) => false,
        };
        log_line(h, why, "success");
        if saved {
            log_line(h, "Saved to this app's trusted host keys.", "info");
        }
    } else {
        log_line(h, why, "success");
    }
    // Mark as auto-accepted (no prompt was shown) so the connect
    // driver knows host-key wasn't the failure mode for any
    // downstream error.
    h.fp_outcome.store(1, std::sync::atomic::Ordering::SeqCst);
    Ok(true)
}

/// The same host-key question already on screen for another session: wait
/// for its answer instead of asking again. `Ok(asker)`: this session asks
/// (nobody else is, or the other went unanswered); `Err(answer)`: what
/// check_server_key returns, the other session's answer, or false for an
/// attempt replaced or closed while it waited.
pub(crate) async fn ask_or_follow(h: &ClientHandler, question: HostKeyQuestion) -> Result<HostKeyAsker, Result<bool, russh::Error>> {
    loop {
        let rx = match host_key_turn(question.clone()) {
            HostKeyTurn::Ask(a) => return Ok(a),
            HostKeyTurn::Follow(rx) => rx,
        };
        let _ = h.app.emit(&format!("session-log-{}", h.session_id), serde_json::json!({
            "msg": "Another session is asking about this same host key — waiting for that answer.",
            "type": "info"
        }));
        // Waiting on a person, so the handshake timeout holds off.
        h.prompt_pending.store(true, std::sync::atomic::Ordering::SeqCst);
        let wait = asked_answer(rx);
        tokio::pin!(wait);
        let answer = loop {
            tokio::select! {
                a = &mut wait => break Some(a),
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    if !h.attempt.may_prompt().await {
                        break None;
                    }
                }
            }
        };
        h.prompt_pending.store(false, std::sync::atomic::Ordering::SeqCst);
        let (msg, outcome) = match answer {
            // This attempt was replaced or closed while waiting.
            None => {
                h.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
                return Err(Ok(false));
            }
            Some(None) => continue,
            Some(Some(HostKeyAnswer::Accepted)) => ("Host key accepted (answered in another session).", 1),
            Some(Some(HostKeyAnswer::Rejected)) => ("Host key rejected (answered in another session).", 0),
            Some(Some(HostKeyAnswer::Unanswered)) => {
                ("Host key verification timed out (the prompt in another session wasn't answered).", 2)
            }
        };
        let _ = h.app.emit(&format!("session-log-{}", h.session_id), serde_json::json!({
            "msg": msg,
            "type": if outcome == 1 { "success" } else { "error" }
        }));
        h.fp_outcome.store(outcome, std::sync::atomic::Ordering::SeqCst);
        return Err(Ok(outcome == 1));
    }
}

impl ClientHandler {
    pub(crate) fn keys_on_file(&self, fingerprint: &str) -> Result<HostKeyVerdict, String> {
        keys_on_file(&self.db, &self.server_host, self.server_port, fingerprint)
    }
}

/// What the app's trusted keys say about the key `fingerprint` for
/// host:port (judge_host_key). Fails closed: a poisoned lock, a failed
/// lookup, or no profile open (nothing to check against, and nowhere to
/// save an answer) is an `Err` with the reason, never "first time". The
/// std mutex guard is dropped before the caller's next `.await`.
pub(crate) fn keys_on_file(
    db: &std::sync::Mutex<Option<rusqlite::Connection>>,
    host: &str,
    port: u16,
    fingerprint: &str,
) -> Result<HostKeyVerdict, String> {
    let guard = db.lock().map_err(|_| "the trusted host keys couldn't be read; restart the app".to_string())?;
    let conn = guard.as_ref().ok_or("no profile is open, so the server's key can't be checked")?;
    judge_host_key(conn, host, port, fingerprint).map_err(|_| "the trusted host keys couldn't be read; restart the app".to_string())
}

#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "ssh_manager.rs",
            include_str!("../ssh_manager.rs"),
            &[
                ("", "mod host_key_asker;", 1),
                ("", "pub(crate) use host_key_asker::*;", 1),
            ],
        );
    }
}
