//! The matcher, the decision, and the tables, including every way around
//! them we know of.

use super::*;

fn rules(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// -- Allow rules ---------------------------------------------------------------

#[test]
fn an_exact_rule_matches_only_that_command() {
    assert!(allow_matches("uptime", "uptime"));
    assert!(!allow_matches("uptime", "uptime -p"));
    assert!(!allow_matches("uptime", "uptim"));
    assert!(!allow_matches("uptime", "uptimex"));
    assert!(!allow_matches("uptime", "/usr/bin/uptime"));
    // Case counts: allowing is never looser than what you wrote.
    assert!(!allow_matches("uptime", "UPTIME"));
}

#[test]
fn a_space_before_the_star_is_a_word_boundary() {
    assert!(allow_matches("ls *", "ls"));
    assert!(allow_matches("ls *", "ls -la"));
    assert!(allow_matches("ls *", "ls -la /var/log"));
    assert!(!allow_matches("ls *", "lsof"));
    assert!(!allow_matches("ls *", "lsof -i"));
    assert!(!allow_matches("ls *", "lsblk"));
    assert!(allow_matches("systemctl status *", "systemctl status"));
    assert!(allow_matches("systemctl status *", "systemctl status nginx"));
    assert!(!allow_matches("systemctl status *", "systemctl statusx"));
    assert!(!allow_matches("systemctl status *", "systemctl restart nginx"));
    assert!(!allow_matches("systemctl status *", "systemctl"));
}

#[test]
fn a_star_without_a_space_is_the_rest_of_that_word() {
    assert!(allow_matches("ls*", "lsof"));
    assert!(allow_matches("ls*", "ls"));
    assert!(!allow_matches("ls*", "cat ls"));
    assert!(!allow_matches("ls*", "lsof -i"), "no space after it");
    assert!(allow_matches("cat /var/log/*", "cat /var/log/syslog"));
    assert!(allow_matches("cat /var/log/*", "cat /var/log/nginx/error.log"));
    assert!(!allow_matches("cat /var/log/*", "cat /var/log/syslog /etc/shadow"));
    assert!(!allow_matches("cat /var/log/*", "cat /var/log/../../etc/shadow"));
    assert!(!allow_matches("cat /var/log/*", "cat /var/log/.."));
    assert!(allow_matches("cat /var/log/*", "cat /var/log/x..y"), "only a whole .. part");
    assert!(!allow_matches("type C:\\logs\\*", "type C:\\logs\\..\\secrets.txt"));
}

#[test]
fn the_ai_s_own_glob_isnt_saved_as_a_rule() {
    assert!(glob_as_rule("cat /var/log/*", "cat /var/log/*"));
    assert!(glob_as_rule(" ls * ", "ls *"));
    assert!(!glob_as_rule("cat /var/log/syslog", "cat /var/log/syslog"));
    assert!(!glob_as_rule("cat /var/log/*", "cat /var/log/syslog"));
}

#[test]
fn a_star_anywhere_else_matches_nothing() {
    assert!(!allow_matches("ls * -la", "ls x -la"));
    assert!(!allow_matches("*", "anything"));
    assert!(!allow_matches(" *", "anything"));
    assert!(!allow_matches("", ""));
}

#[test]
fn spaces_and_tabs_at_the_ends_are_trimmed() {
    assert!(allow_matches("uptime", "  uptime  "));
    assert!(allow_matches("uptime", "\tuptime\t"));
    assert!(allow_matches("ls *", "  ls -la "));
    // A tab inside isn't a word boundary to a rule: it asks.
    assert!(!allow_matches("ls *", "ls\t-la"));
    // Other whitespace at the ends isn't trimmed: it asks.
    assert!(!allow_matches("uptime", "uptime\u{A0}"));
    assert!(!allow_matches("uptime", "uptime\n"));
}

#[test]
fn operators_never_match_an_allow_rule() {
    for cmd in [
        "ls; reboot",
        "ls;reboot",
        "ls && reboot",
        "ls & reboot",
        "ls || reboot",
        "ls | sh",
        "ls > /etc/passwd",
        "ls >> f",
        "ls < /dev/sda",
        "ls 2>&1",
        "ls (x)",
        "ls { x; }",
        "ls }",
        "ls &",
    ] {
        assert!(!is_simple(cmd), "{cmd}");
        assert!(!allow_matches("ls *", cmd), "{cmd}");
    }
}

#[test]
fn substitution_and_expansion_never_match() {
    for cmd in ["ls $(reboot)", "ls `reboot`", "ls ${IFS}x", "ls$IFS-la", "ls $HOME", "ls <(reboot)", "ls $'\\x41'"] {
        assert!(!allow_matches("ls *", cmd), "{cmd}");
    }
}

#[test]
fn new_lines_and_continuations_never_match() {
    for cmd in ["ls\nreboot", "ls\rreboot", "ls\r\nreboot", "ls -la \\", "ls \\", "uptime\\"] {
        assert!(!allow_matches("ls *", cmd), "{cmd:?}");
        assert!(!allow_matches("uptime*", cmd), "{cmd:?}");
    }
}

#[test]
fn hidden_characters_never_match() {
    for c in [
        '\u{202E}', // right-to-left override
        '\u{202D}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', // embeddings and isolates
        '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}', // zero-width
        '\u{2028}', '\u{2029}', // line and paragraph separators
        '\u{AD}', '\u{34F}', '\u{115F}', '\u{3164}', '\u{FFA0}', // invisible letters
        '\u{FE0F}', '\u{E0041}', '\u{E0100}', // variation selectors and tags
        '\u{0}', '\u{1B}', '\u{7F}', '\u{85}', '\u{9B}', // controls
    ] {
        let cmd = format!("ls -la{c}x");
        assert!(!is_simple(&cmd), "U+{:04X}", c as u32);
        assert!(!allow_matches("ls *", &cmd), "U+{:04X}", c as u32);
        assert!(!allow_matches(&format!("ls -la{c}x"), &cmd), "an exact rule with it neither: U+{:04X}", c as u32);
    }
}

#[test]
fn look_alikes_are_other_commands() {
    // A Cyrillic р: not the command you allowed.
    assert!(!allow_matches("uptime", "uрtime"));
    assert!(!allow_matches("ls *", "ⅼs -la"));
    // A full-width semicolon isn't an operator to the shell: `ls` gets it
    // as an argument, so nothing else runs.
    assert!(!allow_matches("ls", "ls；reboot"));
}

#[test]
fn quotes_alone_are_fine_but_conservative() {
    assert!(allow_matches("grep *", "grep -r 'needle' /etc"));
    // The shell would run this as one command; a rule doesn't take it.
    assert!(!allow_matches("echo *", "echo \"a;b\""));
    // The command word must be the rule's own.
    assert!(!allow_matches("ls *", "l's' -la"));
}

// -- Always-ask rules ----------------------------------------------------------

#[test]
fn an_always_ask_rule_is_found_anywhere() {
    for cmd in [
        "reboot",
        "reboot now",
        "  reboot",
        "uptime; reboot",
        "uptime;reboot",
        "uptime && reboot",
        "uptime || reboot",
        "uptime | reboot",
        "uptime & reboot",
        "sudo reboot",
        "sudo -n reboot",
        "/sbin/reboot",
        "/usr/sbin/reboot -f",
        "(reboot)",
        "{ reboot; }",
        "$(reboot)",
        "`reboot`",
        "echo x\nreboot",
        "echo x\rreboot",
        "echo x\treboot",
        "r''eboot",
        "\"reboot\"",
        "\\reboot",
        "re\\boot",
        "reb\u{200B}oot",
        "REBOOT",
        "reboot${IFS}now",
        "nohup reboot >/dev/null 2>&1 &",
    ] {
        assert!(ask_matches("reboot", cmd), "{cmd:?}");
    }
    for cmd in ["rebooted", "echo nonreboot", "uptime", "cat /var/log/reboot.log", "rеboot"] {
        assert!(!ask_matches("reboot", cmd), "{cmd:?}");
    }
}

#[test]
fn an_always_ask_rule_of_several_words() {
    assert!(ask_matches("rm -rf *", "cd /; rm -rf /tmp"));
    assert!(ask_matches("rm -rf *", "rm -rf"));
    assert!(ask_matches("rm -rf *", "sudo rm -rf --no-preserve-root /"));
    assert!(ask_matches("rm -rf *", "true&&rm -rf x"));
    assert!(!ask_matches("rm -rf *", "rm -r -f /"), "only the spelling written");
    assert!(!ask_matches("rm -rf *", "rm x -rf"));
    assert!(ask_matches("init 0", "init 0"));
    assert!(ask_matches("init 0", "sudo /sbin/init 0"));
    assert!(!ask_matches("init 0", "init 6"));
    assert!(!ask_matches("init 0", "init"));
}

#[test]
fn an_always_ask_prefix() {
    assert!(ask_matches("mkfs*", "mkfs.ext4 /dev/sdb1"));
    assert!(ask_matches("mkfs*", "sudo /sbin/mkfs -t xfs /dev/sdb"));
    assert!(!ask_matches("mkfs*", "echo nomkfs"));
    assert!(ask_matches("dd *", "dd if=/dev/zero of=/dev/sda"));
    assert!(ask_matches("dd *", "dd"));
    assert!(!ask_matches("dd *", "add x"));
    assert!(!ask_matches("dd *", "ddrescue"));
}

#[test]
fn the_fork_bomb_rule() {
    assert!(ask_matches(":(){", ":(){ :|:& };:"));
    assert!(ask_matches(":(){", "bash -c ':(){ :|:& };:'"));
    assert!(!ask_matches(":(){", "echo a:b"));
}

#[test]
fn every_default_rule_is_valid() {
    for r in DEFAULT_ASK {
        assert_eq!(check_rule("ask", r).as_deref(), Ok(*r));
    }
}

// -- The decision --------------------------------------------------------------

/// The decision at `level`, with `allow` and `ask` lists.
fn dec(level: u8, session: bool, allow: &[String], ask: &[String], command: &str) -> Decision {
    decide(&Policy { level, allow: allow.to_vec(), ask: ask.to_vec(), windows: false }, session, command)
}

fn ask() -> Decision {
    Decision::Ask { rule: None }
}

#[test]
fn level_one_always_asks() {
    let allow = rules(&["uptime"]);
    assert_eq!(dec(1, false, &allow, &[], "uptime"), ask());
    // Anything not 1, 2 or 3 asks every time too.
    assert_eq!(dec(0, false, &allow, &[], "uptime"), ask());
    assert_eq!(dec(9, false, &allow, &[], "uptime"), ask());
}

#[test]
fn level_two_runs_allowed_commands_only() {
    let allow = rules(&["uptime", "systemctl status *"]);
    assert_eq!(dec(2, false, &allow, &[], "uptime"), Decision::Rule("uptime".into()));
    assert_eq!(dec(2, false, &allow, &[], " systemctl status nginx "), Decision::Rule("systemctl status *".into()));
    assert_eq!(dec(2, false, &allow, &[], "systemctl restart nginx"), ask());
    assert_eq!(dec(2, false, &allow, &[], "uptime; reboot"), ask());
    assert_eq!(dec(2, false, &allow, &[], "uptime && id"), ask());
    assert_eq!(dec(2, false, &allow, &[], "uptime\nid"), ask());
    assert_eq!(dec(2, false, &[], &[], "uptime"), ask());
}

#[test]
fn level_two_asks_for_sudo_even_with_a_rule() {
    let allow = rules(&["sudo *", "systemctl *"]);
    let cmd = "sudo systemctl restart nginx";
    assert!(crate::ai::asks_for_root(cmd));
    assert_eq!(dec(2, false, &allow, &[], cmd), ask());
    for cmd in ["su -c id", "doas id", "pkexec id", "dzdo id", "/usr/bin/sudo id"] {
        let allow = rules(&["* id", "su *", "doas *", "pkexec *", "dzdo *", "/usr/bin/sudo *"]);
        assert_eq!(dec(2, false, &allow, &[], cmd), ask(), "{cmd}");
    }
}

#[test]
fn level_three_runs_anything_but_the_always_ask_list() {
    let ask_list = rules(&["reboot", "rm -rf *"]);
    assert_eq!(dec(3, false, &[], &ask_list, "uptime; id | head"), Decision::Anything);
    assert_eq!(dec(3, false, &[], &ask_list, "sudo id"), Decision::Anything, "sudo too, where it needs no password");
    assert_eq!(dec(3, false, &[], &ask_list, "uptime && reboot"), Decision::Ask { rule: Some("reboot".into()) });
    assert_eq!(dec(3, false, &[], &ask_list, "cd /; rm -rf /tmp"), Decision::Ask { rule: Some("rm -rf *".into()) });
}

#[test]
fn the_always_ask_list_beats_allow_rules() {
    let allow = rules(&["shutdown *"]);
    let ask_list = rules(&["shutdown"]);
    assert_eq!(dec(2, false, &allow, &ask_list, "shutdown -h now"), Decision::Ask { rule: Some("shutdown".into()) });
}

#[test]
fn the_session_allow_runs_anything_but_the_always_ask_list() {
    let ask_list = rules(&["reboot"]);
    assert_eq!(dec(1, true, &[], &ask_list, "uptime; id"), Decision::Session);
    assert_eq!(dec(2, true, &[], &ask_list, "sudo id"), Decision::Session);
    assert_eq!(dec(3, true, &[], &ask_list, "id"), Decision::Session);
    assert_eq!(dec(1, true, &[], &ask_list, "id; reboot"), Decision::Ask { rule: Some("reboot".into()) });
}

#[test]
fn how_each_was_approved() {
    assert_eq!(Decision::Rule("uptime".into()).approved_by().as_deref(), Some("rule: uptime"));
    assert_eq!(Decision::Anything.approved_by().as_deref(), Some("level: run anything"));
    assert_eq!(Decision::Session.approved_by().as_deref(), Some("session: allow everything"));
    assert_eq!(ask().approved_by(), None);
}

// -- Checking rules --------------------------------------------------------------

#[test]
fn rules_are_checked() {
    assert_eq!(check_rule("allow", "  uptime ").as_deref(), Ok("uptime"));
    assert_eq!(check_rule("allow", "ls *").as_deref(), Ok("ls *"));
    assert!(check_rule("allow", "").is_err());
    assert!(check_rule("allow", "*").is_err());
    assert!(check_rule("allow", " * ").is_err());
    assert!(check_rule("allow", "ls * x").is_err());
    assert!(check_rule("allow", "ls; reboot").is_err());
    assert!(check_rule("allow", "ls $(x)").is_err());
    assert!(check_rule("allow", "ls\u{202E}").is_err());
    assert!(check_rule("allow", &"x".repeat(MAX_RULE + 1)).is_err());
    // Always-ask rules may hold operators: they're split into words.
    assert_eq!(check_rule("ask", ":(){").as_deref(), Ok(":(){"));
    assert!(check_rule("ask", ";;").is_err());
    assert!(check_rule("other", "x").is_err());
}

// -- The tables ------------------------------------------------------------------

/// A vault with servers 1 and 2: 1 in folder 11, inside 10; 2 at the top.
fn vault() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE folders (id INTEGER PRIMARY KEY, name TEXT, parent_id INTEGER);
         CREATE TABLE servers (id INTEGER PRIMARY KEY, name TEXT, folder_id INTEGER);
         INSERT INTO folders VALUES (10, 'Test', NULL), (11, 'Web', 10);
         INSERT INTO servers VALUES (1, 'web-01', 11), (2, 'db-01', NULL);",
    )
    .unwrap();
    create_schema(&conn).unwrap();
    conn
}

#[test]
fn a_new_vault_starts_with_the_default_always_ask_list_once() {
    let conn = vault();
    let p = policy_for(&conn, Some(2));
    assert_eq!(p.level, 1);
    assert_eq!(p.ask, rules(DEFAULT_ASK));
    // Removed, it doesn't come back at the next open.
    let reboot = view(&conn).unwrap().rules.into_iter().find(|r| r.pattern == "reboot").unwrap();
    remove_rule(&conn, reboot.id).unwrap();
    create_schema(&conn).unwrap();
    assert!(!policy_for(&conn, Some(2)).ask.contains(&"reboot".to_string()));
}

#[test]
fn the_level_comes_from_the_server_then_its_folders_then_the_default() {
    let conn = vault();
    let level = |id| policy_for(&conn, Some(id)).level;
    assert_eq!((level(1), level(2)), (1, 1), "nothing set: ask every time");
    set_level(&conn, "default", 0, Some(2)).unwrap();
    assert_eq!((level(1), level(2)), (2, 2));
    assert_eq!(policy_for(&conn, None).level, 2, "a quick connect follows the default");
    set_level(&conn, "folder", 10, Some(3)).unwrap();
    assert_eq!((level(1), level(2)), (3, 2), "from the folder above its own");
    set_level(&conn, "folder", 11, Some(1)).unwrap();
    assert_eq!(level(1), 1, "its own folder is nearer");
    set_level(&conn, "server", 1, Some(2)).unwrap();
    assert_eq!(level(1), 2, "its own level wins");
    set_level(&conn, "server", 1, None).unwrap();
    set_level(&conn, "folder", 11, None).unwrap();
    assert_eq!(level(1), 3);
    assert!(set_level(&conn, "server", 99, Some(3)).is_err(), "no such server");
    assert!(set_level(&conn, "server", 1, Some(4)).is_err());
    assert!(set_level(&conn, "tab", 1, Some(2)).is_err());
}

#[test]
fn rules_on_the_way_up_all_count() {
    let conn = vault();
    conn.execute("DELETE FROM ai_rules", []).unwrap();
    add_rule(&conn, "server", 1, "allow", "uptime").unwrap();
    add_rule(&conn, "folder", 10, "allow", "df *").unwrap();
    add_rule(&conn, "default", 0, "allow", "id").unwrap();
    add_rule(&conn, "folder", 11, "ask", "reboot").unwrap();
    add_rule(&conn, "server", 2, "allow", "free *").unwrap();
    let p = policy_for(&conn, Some(1));
    assert_eq!(p.allow, rules(&["uptime", "df *", "id"]));
    assert_eq!(p.ask, rules(&["reboot"]));
    let p = policy_for(&conn, Some(2));
    assert_eq!(p.allow, rules(&["free *", "id"]));
    assert!(p.ask.is_empty());
    // The same rule twice is kept once.
    add_rule(&conn, "server", 1, "allow", " uptime ").unwrap();
    assert_eq!(policy_for(&conn, Some(1)).allow.len(), 3);
    assert!(add_rule(&conn, "server", 1, "allow", "uptime; reboot").is_err());
}

#[test]
fn deleting_a_server_or_folder_deletes_its_settings() {
    let conn = vault();
    set_level(&conn, "server", 1, Some(3)).unwrap();
    add_rule(&conn, "server", 1, "allow", "uptime").unwrap();
    set_level(&conn, "folder", 11, Some(3)).unwrap();
    add_rule(&conn, "folder", 11, "ask", "reboot").unwrap();
    conn.execute("DELETE FROM servers WHERE id = 1", []).unwrap();
    conn.execute("DELETE FROM folders WHERE id = 11", []).unwrap();
    let left = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(left("SELECT COUNT(*) FROM ai_levels"), 0);
    assert_eq!(left("SELECT COUNT(*) FROM ai_rules WHERE scope != 'default'"), 0);
    // A server made later with another id starts at the default.
    conn.execute("INSERT INTO servers VALUES (3, 'web-01', NULL)", []).unwrap();
    assert_eq!(policy_for(&conn, Some(3)).level, 1);
}

#[test]
fn a_loop_in_the_folder_tree_ends() {
    let conn = vault();
    conn.execute("UPDATE folders SET parent_id = 11 WHERE id = 10", []).unwrap();
    set_level(&conn, "default", 0, Some(2)).unwrap();
    assert_eq!(policy_for(&conn, Some(1)).level, 2);
    scope_view(&conn, "folder", 10).unwrap();
}

#[test]
fn what_cannot_be_read_asks_every_time() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(policy_for(&conn, Some(1)), Policy::default());
}

#[test]
fn settings_lists_what_is_set() {
    let conn = vault();
    set_level(&conn, "server", 2, Some(3)).unwrap();
    set_level(&conn, "folder", 10, Some(2)).unwrap();
    set_level(&conn, "folder", 11, Some(1)).unwrap();
    set_level(&conn, "default", 0, Some(2)).unwrap();
    add_rule(&conn, "server", 1, "allow", "uptime").unwrap();
    let v = view(&conn).unwrap();
    assert_eq!(v.default_level, 2);
    let names: Vec<(&str, &str, u8)> = v.levels.iter().map(|l| (l.scope.as_str(), l.name.as_str(), l.level)).collect();
    assert_eq!(names, [("server", "db-01", 3), ("folder", "Test", 2)], "above level 1 only");
    let r = v.rules.iter().find(|r| r.pattern == "uptime").unwrap();
    assert_eq!((r.scope.as_str(), r.name.as_str(), r.kind.as_str()), ("server", "web-01", "allow"));
    assert_eq!(v.rules[0].scope, "default");
    let s = scope_view(&conn, "server", 1).unwrap();
    assert_eq!((s.own_level, s.inherited_level, s.inherited_from.as_str()), (None, 1, "Web"));
    assert_eq!(s.rules.len(), 1);
    assert_eq!(s.inherited_ask, DEFAULT_ASK.len());
    let s = scope_view(&conn, "folder", 11).unwrap();
    assert_eq!((s.own_level, s.inherited_level, s.inherited_from.as_str()), (Some(1), 2, "Test"));
    let s = scope_view(&conn, "default", 0).unwrap();
    assert_eq!((s.own_level, s.inherited_level), (Some(2), 1));
    assert_eq!(s.rules.len(), DEFAULT_ASK.len());
}

/// The levels and rules never sync: they aren't synced tables, nor sync
/// entities, and have no sync columns.
#[test]
fn the_tables_are_not_synced() {
    for t in ["ai_levels", "ai_rules"] {
        assert!(!crate::SYNCED_TABLES.contains(&t), "{t}");
        assert!(!crate::ENTITIES.iter().any(|e| e.table == t), "{t}");
    }
    let conn = vault();
    for t in ["ai_levels", "ai_rules"] {
        let cols: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{t}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!cols.iter().any(|c| c == "uuid" || c == "updated_at"), "{t}: {cols:?}");
    }
}

// -- Windows and full-width forms -----------------------------------------------

#[test]
fn full_width_forms_never_match() {
    for c in ['\u{FF1B}', '\u{FF06}', '\u{FF5C}', '\u{FF04}', '\u{FF41}', '\u{FFE0}'] {
        let cmd = format!("ls -la{c}x");
        assert!(!is_simple(&cmd), "U+{:04X}", c as u32);
        assert!(!allow_matches("ls *", &cmd), "U+{:04X}", c as u32);
    }
}

#[test]
fn percent_and_caret_are_operators_on_windows() {
    for cmd in ["dir %COMSPEC%", "dir ^^x", "dir x^"] {
        assert!(allow_matches_on("dir *", cmd, false), "{cmd} is simple to a POSIX shell");
        assert!(!allow_matches_on("dir *", cmd, true), "{cmd}");
    }
    let windows = Policy { level: 2, allow: rules(&["dir *"]), ask: vec![], windows: true };
    assert_eq!(decide(&windows, false, "dir C:\\Users"), Decision::Rule("dir *".into()));
    assert_eq!(decide(&windows, false, "dir %USERPROFILE%"), ask());
}

#[test]
fn a_windows_server_is_known_by_its_saved_os() {
    let conn = vault();
    conn.execute("ALTER TABLE servers ADD COLUMN os TEXT", []).unwrap();
    conn.execute("UPDATE servers SET os = 'windows' WHERE id = 2", []).unwrap();
    assert!(policy_for(&conn, Some(2)).windows);
    assert!(!policy_for(&conn, Some(1)).windows);
}

// -- Root, read as the always-ask list reads words ----------------------------------

#[test]
fn root_is_seen_however_it_is_written() {
    for cmd in [
        "sudo id",
        "'sudo' id",
        "s\\udo id",
        "\"s\"udo id",
        "SUDO id",
        "/usr/bin/sudo id",
        "sudo.exe id",
        "sudoedit /etc/hosts",
        "run0 id",
        "sud? id",
        "/usr/bin/s[u]do id",
        "su -",
        "doas id",
    ] {
        assert!(mentions_root(cmd), "{cmd}");
        assert!(crate::ai::asks_for_root(cmd), "{cmd}: the card says so too");
        let p = Policy { level: 2, allow: rules(&["* id", "sud? *", "s\\udo *", "'sudo' *", "SUDO *", "sudoedit *", "run0 *", "doas *", "su *"]), ask: vec![], windows: false };
        assert_eq!(decide(&p, false, cmd), ask(), "{cmd}");
    }
    for cmd in ["cat sudoers.txt", "ls -su", "echo pseudo", "ls *", "id", "visudo-ish"] {
        assert!(!mentions_root(cmd), "{cmd}");
    }
}

#[test]
fn rules_for_commands_that_run_commands_are_warned_about() {
    for p in ["timeout *", "env *", "xargs*", "bash *", "python3 *", "python3.12 *", "/usr/bin/find *", "git *", "pwsh.exe *", "awk *", "C:\\Windows\\System32\\cmd.exe *", "'bash' *"] {
        assert!(runner_warning(p).is_some(), "{p}");
    }
    assert_eq!(
        runner_warning("timeout *").as_deref(),
        Some("`timeout *` lets the AI run any command, as root too where sudo needs no password.")
    );
    for p in ["timeout 5 uptime", "ls *", "systemctl status *", "gitk", "uptime"] {
        assert!(runner_warning(p).is_none(), "{p}");
    }
}

#[test]
fn globs_are_matched() {
    let g = |p: &str, t: &str| glob_match(&p.chars().collect::<Vec<_>>(), &t.chars().collect::<Vec<_>>());
    assert!(g("reb?ot", "reboot"));
    assert!(g("r*t", "reboot"));
    assert!(g("re[a-c]oot", "reboot"));
    assert!(!g("re[!b]oot", "reboot"));
    assert!(!g("reb?ot", "rebot"));
    assert!(g("[", "["));
}

