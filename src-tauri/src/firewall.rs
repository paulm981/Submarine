//! The Network tab's firewall summary where only root can read the
//! rules (iptables or nft, through root_ssh.rs).
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use firewall::*`.

use crate::*;

/// The Network tab's firewall summary for a host where only root can read
/// the rules (its probe printed `FW:iptables-denied` / `FW:nft-denied`): the
/// same `FW:` header and rows the probe prints, from the rules read as root
/// (run_as_root: sudo or dzdo, with `password` when the tool asked for one).
#[tauri::command]
pub(crate) async fn ssh_firewall_as_root(
    state: tauri::State<'_, SshState>,
    session_id: String,
    engine: String,
    password: Option<String>,
) -> Result<String, String> {
    match engine.as_str() {
        "iptables" => {
            let mut out = String::from("FW:iptables-sudo\n");
            let mut password = password;
            for table in ["filter", "nat", "mangle", "raw"] {
                let args = ["iptables", "-t", table, "-S"].map(String::from);
                let (code, rules, _) = run_as_root(&state, &session_id, &args, password.take(), 15).await?;
                if code != 0 {
                    // A table the kernel doesn't have is left out, as the
                    // probe leaves it out; without `filter` there's nothing.
                    if table == "filter" {
                        return Err(format!("iptables: {}", rules.trim()));
                    }
                    continue;
                }
                out.push_str(&iptables_chain_summary(table, &rules));
            }
            Ok(out)
        }
        "nft" => {
            let args = ["nft", "list", "ruleset"].map(String::from);
            let (code, ruleset, _) = run_as_root(&state, &session_id, &args, password, 15).await?;
            if code != 0 {
                return Err(format!("nft: {}", ruleset.trim()));
            }
            Ok(format!("FW:nft-sudo\n{}", nft_chain_summary(&ruleset)))
        }
        _ => Err("invalid firewall engine".to_string()),
    }
}

/// `table|chain|policy|count` for each chain in `iptables -t <table> -S`
/// output, as the probe's awk prints them: `-P` is a built-in chain and its
/// policy, `-N` a chain of the user's (policy `-`), `-A` one rule.
pub(crate) fn iptables_chain_summary(table: &str, rules: &str) -> String {
    let mut order: Vec<&str> = Vec::new();
    let mut policy: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let mut count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in rules.lines() {
        let mut f = line.split_whitespace();
        let (Some(op), Some(chain)) = (f.next(), f.next()) else { continue };
        match op {
            "-P" => {
                policy.insert(chain, f.next().unwrap_or("-"));
            }
            "-N" => {
                policy.entry(chain).or_insert("-");
            }
            "-A" => {
                *count.entry(chain).or_default() += 1;
                policy.entry(chain).or_insert("-");
            }
            _ => continue,
        }
        if !order.contains(&chain) {
            order.push(chain);
        }
    }
    order
        .iter()
        .map(|c| format!("{}|{}|{}|{}\n", table, c, policy.get(c).unwrap_or(&"-"), count.get(c).unwrap_or(&0)))
        .collect()
}

/// `family|table|chain|type|count` for each chain in `nft list ruleset`
/// output, as the probe's awk prints them: the chain's `type` (filter, nat,
/// ...) and how many rules it holds.
pub(crate) fn nft_chain_summary(ruleset: &str) -> String {
    struct Chain<'a> {
        name: &'a str,
        kind: &'a str,
        rules: usize,
    }
    fn flush(out: &mut String, family: &str, table: &str, chain: &mut Option<Chain>) {
        if let Some(c) = chain.take() {
            out.push_str(&format!("{}|{}|{}|{}|{}\n", family, table, c.name, c.kind, c.rules));
        }
    }
    let (mut family, mut table) = ("", "");
    let mut chain: Option<Chain> = None;
    let mut out = String::new();
    for line in ruleset.lines() {
        if let Some(rest) = line.strip_prefix("table ") {
            let mut f = rest.split_whitespace();
            family = f.next().unwrap_or("");
            table = f.next().unwrap_or("");
            chain = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("\tchain ") {
            chain = rest.split_whitespace().next().map(|name| Chain { name, kind: "", rules: 0 });
            continue;
        }
        if ["\tset ", "\tmap ", "\tflowtable ", "\tct "].iter().any(|p| line.starts_with(p)) || line.starts_with("\t}") {
            flush(&mut out, family, table, &mut chain);
            continue;
        }
        let Some(c) = chain.as_mut() else { continue };
        let mut f = line.split_whitespace();
        match f.next() {
            Some("type") => c.kind = f.next().unwrap_or(""),
            Some("hook" | "policy" | "priority" | "flags" | "device") | None => {}
            Some(_) => c.rules += 1,
        }
    }
    out
}

#[cfg(test)]
mod firewall_summary_tests {
    #[test]
    fn iptables_rules_become_one_row_per_chain() {
        let rules = "-P INPUT DROP\n-P FORWARD ACCEPT\n-N web\n-A INPUT -i lo -j ACCEPT\n-A INPUT -j web\n-A web -p tcp --dport 443 -j ACCEPT\n";
        assert_eq!(
            super::iptables_chain_summary("filter", rules),
            "filter|INPUT|DROP|2\nfilter|FORWARD|ACCEPT|0\nfilter|web|-|1\n"
        );
        assert_eq!(super::iptables_chain_summary("nat", ""), "");
    }

    #[test]
    fn an_nft_ruleset_becomes_one_row_per_chain() {
        let ruleset = "table inet filter {\n\tset blocked {\n\t\ttype ipv4_addr\n\t}\n\n\tchain input {\n\t\ttype filter hook input priority filter; policy drop;\n\t\tct state established,related accept\n\t\ttcp dport 22 accept\n\t}\n\n\tchain mine {\n\t}\n}\ntable ip nat {\n\tchain postrouting {\n\t\ttype nat hook postrouting priority srcnat; policy accept;\n\t\tmasquerade\n\t}\n}\n";
        assert_eq!(
            super::nft_chain_summary(ruleset),
            "inet|filter|input|filter|2\ninet|filter|mine||0\nip|nat|postrouting|nat|1\n"
        );
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
                ("", "mod firewall;", 1),
                ("", "use firewall::*;", 1),
                ("run", "ssh_firewall_as_root", 1),
            ],
        );
    }
}
