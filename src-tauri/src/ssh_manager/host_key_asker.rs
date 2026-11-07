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
