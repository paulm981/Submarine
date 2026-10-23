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
    assert!(ask_matches("rm -rf *", "rm -r -f /"), "options in any order, split or long");
    assert!(ask_matches("rm -rf *", "rm x -rf"), "options after the files, as GNU rm takes them");
    assert!(!ask_matches("rm -rf *", "rm -r x"));
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
    decide(&Policy { level, allow: allow.to_vec(), ask: ask.to_vec(), windows: false, force_ask: None }, session, command)
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

/// This install, in the tests.
const HERE: &str = "0123456789abcdef0123456789abcdef";

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
    create_schema(&conn, HERE).unwrap();
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
    create_schema(&conn, HERE).unwrap();
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
    assert_eq!(policy_for(&conn, Some(1)), Policy::unreadable());
    assert_eq!(decide(&Policy::unreadable(), true, "uptime"), Decision::Ask { rule: None }, "even with everything allowed");
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
    let windows = Policy { level: 2, allow: rules(&["dir *"]), ask: vec![], windows: true, force_ask: None };
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
        let p = Policy { level: 2, allow: rules(&["* id", "sud? *", "s\\udo *", "'sudo' *", "SUDO *", "sudoedit *", "run0 *", "doas *", "su *"]), ask: vec![], windows: false, force_ask: None };
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


// -- The collapsed reading -----------------------------------------------------------

#[test]
fn hidden_spellings_of_an_always_ask_rule_ask() {
    let cases: &[(&str, &[&str])] = &[
        // Expansions and quoting.
        ("reboot", &["re$@boot", "re${x}boot", "re``boot", "re$()boot", "$'\\x72eboot'", "re{,}boot", "re$1boot", "re${HOME:0:0}boot", "re$*boot"]),
        // Globs.
        ("reboot", &["/sbin/re{b,}oot", "/sbin/reb?ot", "/sbin/re[b]oot", "sudo /sbin/r*boot"]),
        // A line continuation.
        ("reboot", &["re\\\nboot", "re\\\r\nboot now"]),
        // Windows cmd.
        ("shutdown", &["shutdown.exe /r", "C:\\Windows\\System32\\shutdown.exe /r", "shut^down /r", "shut%PATH:~0,0%down /r", "SHUTDOWN /r /t 0"]),
        // PowerShell.
        ("shutdown", &["shut`down /r"]),
        ("Restart-Computer", &["Restart-Comp`uter -Force", "restart-computer"]),
        // Options in any order, split or long.
        ("rm -rf *", &["rm -fr /", "rm -r -f /", "rm --recursive --force /", "rm -Rf /", "cd /tmp && rm -f -r x", "/bin/rm -rfv x"]),
    ];
    for (rule, commands) in cases {
        for cmd in *commands {
            assert!(ask_matches(rule, cmd), "{rule}: {cmd:?}");
            let p = Policy { level: 3, allow: vec![], ask: rules(&[rule]), windows: false, force_ask: None };
            assert_eq!(decide(&p, true, cmd), Decision::Ask { rule: Some(rule.to_string()) }, "{rule}: {cmd:?}");
        }
    }
}

#[test]
fn the_collapsed_reading_doesnt_ask_for_everything() {
    for cmd in ["uptime", "ls *", "ls -la /var/log/*", "echo $HOME", "cat /etc/os-release", "df -h", "rm -r x", "rm -f x", "rm x -- -rf"] {
        for rule in DEFAULT_ASK {
            if *rule == "rm -rf *" || *rule == "rm -fr *" {
                assert!(!ask_matches(rule, cmd), "{rule}: {cmd}");
                continue;
            }
            assert!(!ask_matches(rule, cmd), "{rule}: {cmd}");
        }
    }
    assert!(!ask_matches("rm -rf *", "rm -r x; ls -f"), "options of another command");
}

#[test]
fn bash_ansi_c_quoting_is_read() {
    assert_eq!(ansi_c("\\x72eboot"), "reboot");
    assert_eq!(ansi_c("\\162eboot"), "reboot");
    assert_eq!(ansi_c("\\u0072eboot"), "reboot");
    assert_eq!(ansi_c("a\\nb"), "a\nb");
}

#[test]
fn new_defaults_are_there_for_a_new_vault() {
    let conn = vault();
    let ask = policy_for(&conn, Some(2)).ask;
    for r in ["rm -fr *", "telinit", "Restart-Computer", "Stop-Computer", "format", "diskpart"] {
        assert!(ask.contains(&r.to_string()), "{r}");
    }
}

#[test]
fn which_servers_a_level_reaches() {
    let conn = vault();
    assert!(reaches(&conn, Some(1), "folder", 10), "a folder above its own");
    assert!(reaches(&conn, Some(1), "folder", 11));
    assert!(!reaches(&conn, Some(2), "folder", 10));
    assert!(reaches(&conn, Some(2), "server", 2) && !reaches(&conn, Some(1), "server", 2));
    assert!(reaches(&conn, None, "default", 0));
}

/// The levels and allow rules belong to the install that set them: a vault
/// opened by another loses them, and keeps its always-ask list.
#[test]
fn levels_and_allow_rules_dont_travel_in_a_copied_vault() {
    let conn = vault();
    set_level(&conn, "server", 1, Some(3)).unwrap();
    set_level(&conn, "default", 0, Some(2)).unwrap();
    add_rule(&conn, "server", 1, "allow", "uptime").unwrap();
    add_rule(&conn, "folder", 10, "ask", "systemctl stop *").unwrap();
    // Opened again here: nothing changes.
    assert!(!create_schema(&conn, HERE).unwrap());
    assert_eq!(policy_for(&conn, Some(1)).level, 3);
    // Opened by another install.
    assert!(create_schema(&conn, "another-install-id-0000000000000").unwrap());
    let p = policy_for(&conn, Some(1));
    assert_eq!(p.level, 1);
    assert!(p.allow.is_empty());
    assert!(p.ask.contains(&"systemctl stop *".to_string()) && p.ask.contains(&"reboot".to_string()), "{:?}", p.ask);
    // With no always-ask rule left, the defaults come back.
    conn.execute("DELETE FROM ai_rules", []).unwrap();
    create_schema(&conn, HERE).unwrap();
    assert_eq!(policy_for(&conn, Some(1)).ask, rules(DEFAULT_ASK));
}

/// A vault that doesn't say which install opened it (from before this)
/// keeps what it has, and is this install's from then on.
#[test]
fn a_vault_without_an_install_keeps_its_levels() {
    let conn = vault();
    set_level(&conn, "server", 1, Some(3)).unwrap();
    conn.execute("DELETE FROM sync_meta WHERE key = 'ai_trust_device'", []).unwrap();
    assert!(create_schema(&conn, "another-install-id-0000000000000").unwrap());
    assert_eq!(policy_for(&conn, Some(1)).level, 3);
    let kept: String = conn.query_row("SELECT value FROM sync_meta WHERE key = 'ai_trust_device'", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, "another-install-id-0000000000000");
}

#[test]
fn what_counts_as_loosened() {
    let p = |level: u8, allow: &[&str], ask: &[&str]| Policy { level, allow: rules(allow), ask: rules(ask), windows: false, force_ask: None };
    assert!(loosened(&p(1, &[], &[]), &p(1, &["uptime"], &[])).is_none(), "allow rules that do nothing at level 1");
    assert_eq!(loosened(&p(2, &[], &[]), &p(2, &["uptime"], &[])), Some((1, vec![])));
    assert_eq!(loosened(&p(1, &[], &[]), &p(3, &[], &[])), Some((0, vec![])));
    assert_eq!(loosened(&p(2, &[], &["reboot"]), &p(2, &[], &[])), Some((0, rules(&["reboot"]))));
    assert!(loosened(&p(3, &["x"], &[]), &p(2, &[], &["reboot"])).is_none(), "less is never held");
}

/// While a move is held, the server asks every time, has no allow rules,
/// and keeps the always-ask rules it lost.
#[test]
fn a_held_move_asks_and_keeps_what_it_lost() {
    let conn = vault();
    conn.execute_batch(
        "ALTER TABLE servers ADD COLUMN uuid TEXT; UPDATE servers SET uuid = 'u' || id;
         CREATE TABLE held_approvals (uuid TEXT NOT NULL, item TEXT NOT NULL, since INTEGER NOT NULL, PRIMARY KEY (uuid, item));",
    )
    .unwrap();
    set_level(&conn, "folder", 11, Some(3)).unwrap();
    add_rule(&conn, "folder", 11, "allow", "uptime").unwrap();
    let held = HeldPolicy { level: 3, allow: 1, lost: rules(&["systemctl stop *"]), moved: "web-01".into(), moved_folder: false, into: "Web".into() };
    conn.execute("INSERT INTO held_approvals VALUES ('u1', ?1, 0)", [format!("{HELD_ITEM}{}", serde_json::to_string(&held).unwrap())]).unwrap();
    let p = policy_for(&conn, Some(1));
    assert_eq!((p.level, p.allow.len(), p.force_ask.as_deref()), (1, 0, Some(HELD_NOTE)));
    assert!(p.ask.contains(&"systemctl stop *".to_string()));
    assert_eq!(decide(&p, true, "uptime"), Decision::Ask { rule: None });
    // Setting the server's level yourself lets it go.
    set_level(&conn, "server", 1, Some(2)).unwrap();
    let p = policy_for(&conn, Some(1));
    assert_eq!((p.level, p.force_ask), (2, None));
}

// -- Second review ---------------------------------------------------------------------

/// Globs the AI writes can't make matching slow: every one of these used to
/// take seconds to hours.
#[test]
fn pathological_globs_are_quick() {
    let stars = |n: usize| "*".repeat(n);
    let cases = [
        format!("ls{}z", stars(10)),
        format!("ls{}z", stars(15)),
        format!("ls{}z", "*a".repeat(40)),
        format!("sudoedit{}", stars(34)),
        format!("s{}x", "?*".repeat(120)),
        format!("[a]{}", "*[!b]".repeat(60)),
        "x".repeat(300) + "*",
    ];
    for word in &cases {
        let cmd = format!("{word} {word} {word}");
        let t = std::time::Instant::now();
        let p = Policy { level: 2, allow: rules(&["ls *"]), ask: rules(DEFAULT_ASK), windows: false, force_ask: None };
        let _ = decide(&p, true, &cmd);
        let _ = asks_for_root(&cmd);
        assert!(t.elapsed() < std::time::Duration::from_millis(50), "{word}: {:?}", t.elapsed());
    }
    // A glob too long to read counts as one that could name anything.
    assert!(word_is(&format!("{}x", "?".repeat(MAX_GLOB + 1)), "reboot", false));
}

#[test]
fn prefix_always_ask_rules_catch_globs_with_text_after() {
    for cmd in ["/sbin/mkf[s].xfs /dev/sdb", "/sbin/mk?s.ext4 /dev/sdb", "mkf*.vfat x", "mk[a-z]s"] {
        assert!(ask_matches("mkfs*", cmd), "{cmd}");
    }
    assert!(!ask_matches("mkfs*", "/sbin/mk?x.ext4"));
    assert!(!ask_matches("mkfs*", "mkf[!s]"));
}

#[test]
fn a_prefix_rule_takes_no_glob_in_the_rest() {
    assert!(allow_matches("cat /var/log/*", "cat /var/log/syslog.1"));
    for cmd in ["cat /var/log/.?/.?/etc/shadow", "cat /var/log/*", "cat /var/log/sys?og", "cat /var/log/[s]yslog"] {
        assert!(!allow_matches("cat /var/log/*", cmd), "{cmd}");
    }
}

#[test]
fn more_ways_to_root_and_more_runners() {
    for cmd in ["runuser -u root id", "machinectl shell", "machinectl shell root@ /bin/sh", "/usr/bin/machinectl shell .host"] {
        assert!(asks_for_root(cmd), "{cmd}");
    }
    assert!(!asks_for_root("machinectl list"));
    for p in ["sed *", "time *", "strace *", "systemd-run *", "tmux *", "at *", "iex *", "Invoke-Expression *", "Start-Process *", "wsl *", "nsenter*"] {
        assert!(runner_warning(p).is_some(), "{p}");
    }
}
