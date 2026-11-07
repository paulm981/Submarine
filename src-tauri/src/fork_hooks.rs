//! Tests only: checks that the fork's hooks are still where they belong.
//!
//! The fork keeps its rules in files of its own (FORK.md, "Keeping up with
//! the original project"). The original project's code calls them through
//! one-line hooks, so a sync's conflict touches a line, not the rule. But a
//! merge can drop such a line, and the rule's own tests still pass: they
//! call the rule directly. Each rule file's `the_hooks_are_in_place` test
//! names its hooks here, with the function each one sits in.
//!
//! A hook is looked for in that function's body only, so a call left in a
//! test, or a comment that names it, doesn't stand in for a lost one.
//!
//! A rule that reads the app's tables is tested on them (`app_table`): the
//! very statement a new vault is made with, so a column a sync renames fails
//! the rule's tests instead of the app.

/// The text of `fn name` in `src`, from its `fn` line to its closing brace
/// (the first line after it that is the `fn` line's indent and `}`, as
/// rustfmt writes it). None when there is no such function.
fn body_of<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let mut offset = 0;
    let mut start: Option<(usize, String)> = None;
    for line in src.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        match &start {
            None => {
                let code = text.trim_start();
                let indent = &text[..text.len() - code.len()];
                let after_fn = ["pub(crate) async fn ", "pub(crate) fn ", "pub async fn ", "pub fn ", "async fn ", "fn "]
                    .iter()
                    .find_map(|p| code.strip_prefix(p));
                if after_fn.is_some_and(|rest| rest.strip_prefix(name).is_some_and(|r| r.starts_with(['(', '<']))) {
                    // A body on the same line: `fn d() {}`.
                    if code.ends_with('}') {
                        return Some(line);
                    }
                    start = Some((offset, indent.to_string()));
                }
            }
            Some((from, indent)) => {
                if text == format!("{indent}}}") {
                    return Some(&src[*from..offset + line.len()]);
                }
            }
        }
        offset += line.len();
    }
    None
}

/// Assert that each `(function, hook, times)` holds in `src`, the text of
/// `file`: `function` exists and calls `hook` exactly `times` times. An
/// empty `function` is the whole file, for a hook outside any function (a
/// `use`): give it in a form no comment or test would repeat.
pub(crate) fn assert_hooks(file: &str, src: &str, hooks: &[(&str, &str, usize)]) {
    let mut missing = Vec::new();
    for (function, hook, times) in hooks {
        let body = if function.is_empty() { Some(src) } else { body_of(src, function) };
        match body {
            None => missing.push(format!("{file} has no `fn {function}` any more, which called `{hook}`")),
            Some(body) => {
                let found = body.matches(hook).count();
                if found != *times {
                    missing.push(format!("`{function}` in {file} should call `{hook}` {times}x, not {found}x"));
                }
            }
        }
    }
    assert!(missing.is_empty(), "a fork hook was lost or moved (FORK.md, \"Keeping up with the original project\"):\n{}", missing.join("\n"));
}

/// The `CREATE TABLE` statement a new vault makes `table` with (lib.rs,
/// open_or_create_vault), to run in a test's own database.
pub(crate) fn app_table(table: &str) -> String {
    let lib = include_str!("lib.rs").replace("\r\n", "\n");
    let head = format!("CREATE TABLE {table} (");
    assert_eq!(lib.matches(&head).count(), 1, "lib.rs should make `{table}` in one place");
    let start = lib.find(&head).unwrap();
    let end = start + lib[start..].find(");").expect("the statement ends") + 2;
    lib[start..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "fn a() {\r\n    hook(1);\r\n}\r\n\r\npub(crate) async fn b<T>(x: T) {\r\n    if x {\r\n        hook(2);\r\n    }\r\n    hook(3);\r\n}\r\n\r\nimpl S {\r\n    pub fn c(&self) {\r\n        hook(4);\r\n    }\r\n}\r\n\r\n// hook(5) in a comment of fn d\r\nfn d() {}\r\n";

    #[test]
    fn a_body_runs_to_its_own_closing_brace() {
        assert_eq!(body_of(SRC, "a").unwrap().matches("hook(").count(), 1);
        assert_eq!(body_of(SRC, "b").unwrap().matches("hook(").count(), 2);
        assert_eq!(body_of(SRC, "c").unwrap().matches("hook(").count(), 1);
        assert!(body_of(SRC, "e").is_none());
        // A one-line body is that line, not the rest of the file.
        assert_eq!(body_of(SRC, "d").unwrap().matches("hook(").count(), 0);
        // `fn ab` isn't `fn a`.
        assert!(body_of("fn ab() {\n}\n", "a").is_none());
    }

    #[test]
    fn the_app_tables_come_from_lib_rs() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for table in ["folders", "servers", "known_hosts"] {
            conn.execute_batch(&app_table(table)).unwrap();
        }
        conn.execute("INSERT INTO servers (name, host, port, jump_host_id) VALUES ('a', 'h', 22, NULL)", []).unwrap();
    }

    // The fork's tests in files of their own (#136): a sync that drops one of
    // the lines that bring them in would stop them running, without a word.
    #[test]
    fn the_fork_tests_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[("", "    mod fork;", 3), ("", "mod monitor_resume_tests;", 1), ("", "mod clone_server_tests;", 1)],
        );
        assert_hooks("monitor.rs", include_str!("monitor.rs"), &[("", "mod jump_host_tests;", 1)]);
        assert_hooks("docker.rs", include_str!("docker.rs"), &[("", "mod cmd_tests;", 1)]);
        assert_hooks("about.rs", include_str!("about.rs"), &[("", "mod tests;", 1)]);
    }

    #[test]
    fn a_lost_or_moved_hook_is_named() {
        assert_hooks("x.rs", SRC, &[("a", "hook(", 1), ("b", "hook(", 2)]);
        let lost = std::panic::catch_unwind(|| assert_hooks("x.rs", SRC, &[("a", "hook(", 2)]));
        assert!(lost.is_err());
        let gone = std::panic::catch_unwind(|| assert_hooks("x.rs", SRC, &[("e", "hook(", 1)]));
        assert!(gone.is_err());
    }
}
