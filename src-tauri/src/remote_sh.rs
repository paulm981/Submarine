//! Commands for a server's shell. sshd runs what the app sends through the
//! account's login shell (`$SHELL -c '<command>'`), which may be sh, bash,
//! zsh, fish, csh or tcsh, and a root shell (`dzdo su -`) reads its line in
//! root's own shell. So the app's scripts never travel as text those shells
//! would parse: each goes as one word of octal escapes (`\ooo`, only a
//! backslash and digits), which a POSIX `sh` turns back into the script with
//! `printf` and runs. csh and tcsh reject a line break inside quotes, and
//! read `!` as history; neither ever appears in what they see.
//!
//! Arguments go as single-quoted words (`quote`), the one quoting every one
//! of those shells reads the same way.

/// `s` as one shell word: single-quoted, a quote inside closing the word,
/// adding an escaped quote and reopening it.
pub(crate) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Each of `words` as one shell word, separated by spaces.
pub(crate) fn quote_words(words: &[String]) -> String {
    words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

/// Each byte of `script` as a `\ooo` escape, which `printf` turns back into
/// the byte. Only backslashes and digits: no `%` for printf, nothing any
/// shell reads as special inside single quotes.
pub(crate) fn octal(script: &str) -> String {
    let mut out = String::with_capacity(script.len() * 4);
    for b in script.bytes() {
        out.push('\\');
        out.push(char::from(b'0' + (b >> 6)));
        out.push(char::from(b'0' + ((b >> 3) & 7)));
        out.push(char::from(b'0' + (b & 7)));
    }
    out
}

/// What decodes and runs the script (the word after it, `$0`) in POSIX sh:
/// no quote, backslash, `!` or line break inside its single quotes.
const DECODE: &str = r#"'eval "$(printf "$0")"'"#;

/// A command that runs `script` (POSIX sh, any number of lines) with `sh`,
/// whatever the login shell is: `env K=V... sh -c <decode> <script> <args>`.
/// The script sees `args` as `"$@"` and `env` as variables; stdin is the
/// command's own, so a password written to it reaches what the script runs.
pub(crate) fn sh_command(env: &[(&str, &str)], script: &str, args: &[String]) -> String {
    sh_command_with("sh", env, script, args)
}

/// sh_command with `sh` at a path of the caller's (`/bin/sh` for a root
/// shell, whose PATH may be anything).
pub(crate) fn sh_command_with(sh: &str, env: &[(&str, &str)], script: &str, args: &[String]) -> String {
    debug_assert!(sh.chars().all(|c| c.is_ascii_alphanumeric() || c == '/'));
    let mut cmd = String::new();
    if !env.is_empty() {
        cmd.push_str("env");
        for (k, v) in env {
            debug_assert!(!k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
            cmd.push(' ');
            cmd.push_str(k);
            cmd.push('=');
            cmd.push_str(&quote(v));
        }
        cmd.push(' ');
    }
    cmd.push_str(sh);
    cmd.push_str(" -c ");
    cmd.push_str(DECODE);
    cmd.push_str(" '");
    cmd.push_str(&octal(script));
    cmd.push('\'');
    if !args.is_empty() {
        cmd.push(' ');
        cmd.push_str(&quote_words(args));
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "if [ \"$1\" = 'a b' ]; then echo one; fi\nfor w; do printf '%s|' \"$w\"; done\necho \"$SUB_X\" | tr \"\\n\" ' ' ; echo; echo 'done!'\n";

    fn args() -> Vec<String> {
        ["a b", "it's", "$(id)", "!x", "back\\slash", "line\nbreak", "100%"].map(String::from).to_vec()
    }

    const EXPECTED: &str = "one\na b|it's|$(id)|!x|back\\slash|line\nbreak|100%|v w \ndone!\n";

    #[test]
    fn words_are_single_quoted() {
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote_words(&["a b".into(), "".into()]), "'a b' ''");
        assert_eq!(octal("a'\n%"), r"\141\047\012\045");
    }

    /// What every login shell is given: one line, and nothing csh or tcsh
    /// read inside quotes (a line break) or anywhere (`!`, history). The
    /// script's own words are all octal.
    #[test]
    fn the_command_is_one_line_without_history_marks() {
        let cmd = sh_command(&[("SUB_X", "v w")], SCRIPT, &args()[..3]);
        assert!(!cmd.contains('\n') && !cmd.contains('\r'), "{cmd}");
        assert!(!cmd.contains('!'), "{cmd}");
        let inner = DECODE.trim_matches('\'');
        assert!(!inner.contains(['\'', '\\', '!', '\n']));
        let after = cmd.split_once(&format!("{} '", DECODE)).unwrap().1;
        let word = after.split_once('\'').unwrap().0;
        assert!(word.len() == SCRIPT.len() * 4 && word.chars().all(|c| c == '\\' || c.is_ascii_digit()), "{word}");
    }

    /// The command run by this machine's shells, as sshd runs it
    /// (`<login shell> -c <command>`): sh, and tcsh and csh where installed.
    #[cfg(unix)]
    #[test]
    fn every_login_shell_runs_the_script_as_written() {
        let cmd = sh_command(&[("SUB_X", "v w")], SCRIPT, &args());
        let mut ran = 0;
        for shell in ["sh", "bash", "tcsh", "csh", "zsh"] {
            let Ok(out) = std::process::Command::new(shell).arg("-c").arg(&cmd).output() else {
                continue;
            };
            // `!x` and the line break are arguments: only the POSIX shells
            // take them. csh and tcsh take the rest as written.
            if matches!(shell, "tcsh" | "csh") {
                let safe = sh_command(&[("SUB_X", "v w")], SCRIPT, &args()[..3]);
                let out = std::process::Command::new(shell).arg("-c").arg(&safe).output().unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout),
                    "one\na b|it's|$(id)|v w \ndone!\n",
                    "{shell}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            } else {
                assert_eq!(String::from_utf8_lossy(&out.stdout), EXPECTED, "{shell}: {}", String::from_utf8_lossy(&out.stderr));
            }
            ran += 1;
        }
        assert!(ran >= 1);
    }

    /// A password written to the command's stdin reaches what the script runs.
    #[cfg(unix)]
    #[test]
    fn stdin_reaches_the_script() {
        use std::io::Write;
        let cmd = sh_command(&[], "IFS= read -r p; printf '[%s]' \"$p\"", &[]);
        for shell in ["sh", "tcsh"] {
            let Ok(mut child) = std::process::Command::new(shell)
                .arg("-c")
                .arg(&cmd)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
            else {
                continue;
            };
            child.stdin.take().unwrap().write_all(b"pa'ss $x\n").unwrap();
            let out = child.wait_with_output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "[pa'ss $x]", "{shell}");
        }
    }
}
