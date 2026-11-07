//! How this vault's records compare with a sync partner's (`sync_diff`).
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use sync_diff::*`.

use crate::*;

/// How this vault's records compare with a sync partner's, item by item —
/// each side's newest stamp per item (a live row or its tombstone).
pub(crate) fn sync_diff(
    local: &[SyncRecord],
    server: &[SyncRecord],
    server_names: &std::collections::HashMap<String, String>,
) -> SyncDiff {
    use std::collections::HashMap;
    fn newest(records: &[SyncRecord]) -> HashMap<&str, (&str, &str)> {
        let mut m: HashMap<&str, (&str, &str)> = HashMap::new();
        // The reserved escrow record is bookkeeping, not user data.
        for r in records.iter().filter(|r| r.entity_type != ESCROW_ETYPE) {
            let e = m.entry(r.uuid.as_str()).or_insert((r.updated_at.as_str(), r.entity_type.as_str()));
            if r.updated_at.as_str() > e.0 {
                *e = (r.updated_at.as_str(), r.entity_type.as_str());
            }
        }
        m
    }
    let local_map = newest(local);
    let server_map = newest(server);

    let mut in_sync = 0usize;
    let mut needs_push = 0usize;
    let mut needs_pull = 0usize;
    let mut out: Vec<String> = Vec::new();

    let mut uuids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    uuids.extend(local_map.keys());
    uuids.extend(server_map.keys());
    for u in uuids {
        let l = local_map.get(u).map(|(ua, _)| *ua).unwrap_or("");
        let s = server_map.get(u).map(|(ua, _)| *ua).unwrap_or("");
        let is_server = local_map.get(u).map(|(_, et)| *et == "servers").unwrap_or(false)
            || server_map.get(u).map(|(_, et)| *et == "servers").unwrap_or(false);
        if l == s {
            in_sync += 1;
        } else {
            if l > s {
                needs_push += 1;
            } else {
                needs_pull += 1;
            }
            if is_server && out.len() < 20 {
                if let Some(name) = server_names.get(u) {
                    out.push(name.clone());
                }
            }
        }
    }
    SyncDiff { in_sync, needs_push, needs_pull, out_of_sync_nodes: out }
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
                ("", "mod sync_diff;", 1),
                ("", "use sync_diff::*;", 1),
                ("profile_sync_stats", "sync_diff(", 1),
            ],
        );
    }
}
