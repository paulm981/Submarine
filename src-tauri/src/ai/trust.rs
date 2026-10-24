//! Approval levels and rules (#115): how much the AI may run on a server
//! without asking you.
//!
//! - **Three levels:** 1 asks every time (the default), 2 runs the commands
//!   you've allowed and asks for the rest, 3 runs anything and logs it.
//! - **Set on a server, a folder or as the default.** A server's own level
//!   wins, then the nearest folder above it that has one, then the default,
//!   then 1.
//! - **Two lists of rules,** on a server, a folder or for every server: what
//!   you always allow, and what always asks. Every list on the way up from
//!   the server counts. An always-ask rule beats every allow rule and level 3.
//! - **Kept on this device only,** in the vault's `ai_levels` and `ai_rules`
//!   tables, by the server's and folder's local ids. They're not in
//!   SYNCED_TABLES, so a shared profile never brings or changes them.
//!   Deleting a server or folder deletes its rows (the triggers in SCHEMA).
//! - **Only you change them,** from the app's own pages (mod.rs's Tauri
//!   commands). The AI has no tool for it: the bridge only reads them, as
//!   each request comes (AiState's `policy`).
//!
//! `decide` is the whole decision, kept pure so it can be tested on its own.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::sync::LazyLock;

pub(crate) const ASK_EVERY_TIME: u8 = 1;
pub(crate) const ALLOWED_COMMANDS: u8 = 2;
pub(crate) const RUN_ANYTHING: u8 = 3;

/// What a rule may be at most, in bytes.
pub(crate) const MAX_RULE: usize = 1024;
/// Rules in one list at most.
const MAX_RULES: i64 = 500;
/// Folders followed up from a server at most: a loop in a broken tree ends.
const MAX_DEPTH: usize = 64;

/// What makes a command more than one simple command, to a POSIX shell (and
/// to cmd.exe and PowerShell): lists, pipes, background, substitution,
/// expansion, redirection, subshells and groups.
const OPERATORS: &[char] = &[';', '&', '|', '`', '$', '>', '<', '(', ')', '{', '}'];

/// The characters the request's card shows as ⟨U+…⟩ (AiPanel's showPlainly:
/// HIDDEN_CONTROLS in src/util/fileName.ts, and the control characters):
/// format characters such as the right-to-left override and zero-width
/// spaces, line and paragraph separators, the invisible letters, variation
/// selectors and tags, and every C0 and C1 control (tab, newline and CR
/// among them).
static HIDDEN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(concat!(
        r"[\p{Cf}\p{Zl}\p{Zp}\u{AD}\u{34F}\u{115F}\u{1160}\u{17B4}\u{17B5}\u{180E}\u{3164}\u{FFA0}",
        r"\u{FE00}-\u{FE0F}\u{E0000}-\u{E007F}\u{E0100}-\u{E01EF}\x00-\x1F\x7F-\x9F]"
    ))
    .unwrap()
});

fn hidden(c: char) -> bool {
    // Printable ASCII never is: most of every command, without the regex.
    if (' '..='~').contains(&c) {
        return false;
    }
    let mut buf = [0u8; 4];
    HIDDEN.is_match(c.encode_utf8(&mut buf))
}

/// The command as rules see it: spaces and tabs at either end taken off.
fn trimmed(command: &str) -> &str {
    command.trim_matches(|c| c == ' ' || c == '\t')
}

/// What cmd.exe also treats specially: `%VAR%` expansion and `^`, its
/// escape (and line continuation).
const WINDOWS_OPERATORS: &[char] = &['%', '^'];

/// The half-width and full-width forms (U+FF00 to U+FFEF): look-alikes of
/// the operators and letters, which no allowed command needs.
fn full_width(c: char) -> bool {
    ('\u{FF00}'..='\u{FFEF}').contains(&c)
}

/// One simple command, which an allow rule may match: no operator, no
/// newline or CR, no backslash at the end (which would join a next line),
/// none of the hidden characters, and no full-width forms. Conservative on
/// purpose: `echo "a;b"` isn't one, though the shell would run it as one.
pub(crate) fn is_simple(command: &str) -> bool {
    is_simple_on(command, false)
}

/// The same, for a server whose saved OS is Windows (`windows`): `%` and
/// `^` count as operators there too.
pub(crate) fn is_simple_on(command: &str, windows: bool) -> bool {
    let c = trimmed(command);
    !c.is_empty()
        && !c.ends_with('\\')
        && !c.chars().any(|ch| OPERATORS.contains(&ch) || hidden(ch) || full_width(ch) || (windows && WINDOWS_OPERATORS.contains(&ch)))
}

/// Whether the allow rule `rule` matches `command`: `uptime` exactly that,
/// `systemctl status *` that followed by a space and anything, or alone,
/// `cat /var/log/*` that followed by the rest of the same word, with no
/// space and no `..`. Only a simple command
/// (is_simple) matches, and a rule with `*` anywhere but at its end matches
/// nothing.
pub(crate) fn allow_matches(rule: &str, command: &str) -> bool {
    allow_matches_on(rule, command, false)
}

/// The same, on a Windows server (`windows`, is_simple_on).
pub(crate) fn allow_matches_on(rule: &str, command: &str, windows: bool) -> bool {
    let cmd = trimmed(command);
    if !is_simple_on(cmd, windows) {
        return false;
    }
    let rule = rule.trim();
    let (body, wild) = match rule.strip_suffix('*') {
        Some(b) => (b, true),
        None => (rule, false),
    };
    if body.contains('*') || body.trim().is_empty() {
        return false;
    }
    if !wild {
        return cmd == body;
    }
    match body.strip_suffix(' ') {
        // A word boundary: `ls *` is `ls` and `ls -la`, never `lsof`.
        Some(prefix) => {
            let prefix = prefix.trim_end();
            cmd == prefix || cmd.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(' '))
        }
        // The rest of that one word only: no space, and no `..` to climb
        // out of the folder written (`cat /var/log/*` never reaches
        // /etc/shadow).
        None => cmd.strip_prefix(body).is_some_and(|rest| {
            let last_word = cmd.rsplit(' ').next().unwrap_or(cmd);
            // No glob either: `.?` is `..` to bash before 5.2.
            !rest.chars().any(|c| c.is_whitespace() || matches!(c, '?' | '*' | '['))
                && !last_word.split(['/', '\\']).any(|seg| seg == "..")
        }),
    }
}

/// What the card refuses to save as a rule: the AI's own command when it
/// ends with `*`. As a rule, its `*` would allow much more than it does as
/// a glob in this one command.
pub(crate) const GLOB_AS_RULE: &str = "This command ends with *, which as a rule would allow more than this command. Edit it first.";

pub(crate) fn glob_as_rule(pattern: &str, command: &str) -> bool {
    let p = pattern.trim();
    p.ends_with('*') && p == trimmed(command)
}

/// A word as an always-ask rule compares it: without quotes, backslashes or
/// hidden characters (`r''eboot`, `\reboot` and `re<U+200B>boot` are all
/// `reboot`), and in lower case (cmd.exe doesn't mind case).
fn plain_word(w: &str) -> String {
    w.chars().filter(|c| !matches!(c, '\'' | '"' | '\\') && !hidden(*c)).collect::<String>().to_lowercase()
}

/// The words of a command or a rule: split at the operators and at any
/// whitespace or control character.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c.is_control() || OPERATORS.contains(&c))
        .map(plain_word)
        .filter(|w| !w.is_empty())
        .collect()
}

/// What Windows runs a name with, dropped from it: `shutdown.exe` is
/// `shutdown`.
const RUNNABLE_EXTENSIONS: &[&str] = &[".exe", ".com", ".bat", ".cmd"];

/// The ways a command word may name a program: as written, the name after
/// its last `/` or `\`, and that without a Windows extension.
fn forms(word: &str) -> Vec<&str> {
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let mut out = vec![word, base];
    if let Some(stem) = RUNNABLE_EXTENSIONS.iter().find_map(|e| base.strip_suffix(e)) {
        out.push(stem);
    }
    out
}

fn is_glob(w: &str) -> bool {
    w.contains(['?', '*', '['])
}

/// A glob longer than this isn't matched: it's taken as one that could
/// name anything, which only ever makes a request ask.
const MAX_GLOB: usize = 256;

/// One part of a glob.
#[derive(Debug, Clone, PartialEq)]
enum Glob {
    Char(char),
    /// `?`
    One,
    /// `*`, a run of them as one.
    Any,
    /// `[…]`: ranges and characters, and whether it's negated (`[!…]`).
    Set(Vec<(char, char)>, bool),
}

fn parse_glob(pat: &[char]) -> Vec<Glob> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < pat.len() {
        match pat[i] {
            '*' => {
                if out.last() != Some(&Glob::Any) {
                    out.push(Glob::Any);
                }
                i += 1;
            }
            '?' => {
                out.push(Glob::One);
                i += 1;
            }
            '[' => match pat.iter().skip(i + 2).position(|c| *c == ']').map(|p| p + i + 2) {
                Some(close) => {
                    let mut set = &pat[i + 1..close];
                    let negate = matches!(set.first(), Some('!' | '^'));
                    if negate {
                        set = &set[1..];
                    }
                    let mut ranges = Vec::new();
                    let mut j = 0;
                    while j < set.len() {
                        if j + 2 < set.len() && set[j + 1] == '-' {
                            ranges.push((set[j], set[j + 2]));
                            j += 3;
                        } else {
                            ranges.push((set[j], set[j]));
                            j += 1;
                        }
                    }
                    out.push(Glob::Set(ranges, negate));
                    i = close + 1;
                }
                None => {
                    out.push(Glob::Char('['));
                    i += 1;
                }
            },
            c => {
                out.push(Glob::Char(c));
                i += 1;
            }
        }
    }
    out
}

/// Whether one part takes the character `c` (not `Any`).
fn takes(g: &Glob, c: char) -> bool {
    match g {
        Glob::Char(x) => *x == c,
        Glob::One => true,
        Glob::Set(ranges, negate) => ranges.iter().any(|(a, b)| (*a..=*b).contains(&c)) != *negate,
        Glob::Any => true,
    }
}

/// The positions in `glob` reachable after reading `text`, as a set:
/// every position at once, so the work is the text's length times the
/// glob's, never more (no backtracking).
fn glob_states(glob: &[Glob], text: &[char]) -> Vec<bool> {
    // Where a `*` may also be skipped.
    let close = |states: &mut Vec<bool>| {
        for i in 0..glob.len() {
            if states[i] && glob[i] == Glob::Any {
                states[i + 1] = true;
            }
        }
    };
    let mut states = vec![false; glob.len() + 1];
    states[0] = true;
    close(&mut states);
    for c in text {
        let mut next = vec![false; glob.len() + 1];
        for i in 0..glob.len() {
            if !states[i] {
                continue;
            }
            match &glob[i] {
                Glob::Any => next[i] = true,
                g if takes(g, *c) => next[i + 1] = true,
                _ => {}
            }
        }
        close(&mut next);
        if !next.iter().any(|s| *s) {
            return next;
        }
        states = next;
    }
    states
}

/// Whether the shell glob `pat` (`?`, `*`, `[…]`) matches all of `text`.
#[cfg(test)]
fn glob_match(pat: &[char], text: &[char]) -> bool {
    pat.len() > MAX_GLOB || glob_states(&parse_glob(pat), text)[parse_glob(pat).len()]
}

/// Whether the glob `pat` could expand to `name`, or (`prefix`) to some
/// word starting with `name`: once all of `name` is read, any part of the
/// glob still reachable can be finished by some ending. A glob longer than
/// MAX_GLOB counts as one that could.
fn glob_could_be(pat: &str, name: &str, prefix: bool) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    if pat.len() > MAX_GLOB {
        return true;
    }
    let glob = parse_glob(&pat);
    let name: Vec<char> = name.chars().collect();
    let states = glob_states(&glob, &name);
    if prefix {
        states.iter().any(|s| *s)
    } else {
        states[glob.len()]
    }
}

/// A command word that names `name` (or, `prefix`, something starting with
/// it): as itself, its name after the last `/` or `\`, without `.exe` and
/// the like, or as a glob that could expand to it (`sud?`, `/sbin/reb?ot`,
/// `mk?s.ext4` for `mkfs*`). A glob of wildcards alone (`*`) counts for
/// nothing: it would name everything.
#[cfg(test)]
fn word_is(word: &str, name: &str, prefix: bool) -> bool {
    word_is_at(word, name, prefix, true)
}

/// The same, where a glob is read as one only when `glob_ok`: a word in
/// command position (`is_command`). Elsewhere it's an argument, such as
/// `*.txt`, compared as it's written.
fn word_is_at(word: &str, name: &str, prefix: bool, glob_ok: bool) -> bool {
    forms(word).into_iter().any(|f| {
        if is_glob(f) && glob_ok {
            f.chars().any(|c| !matches!(c, '?' | '*' | '[' | ']' | '!' | '^' | '-')) && glob_could_be(f, name, prefix)
        } else if prefix {
            f.starts_with(name)
        } else {
            f == name
        }
    })
}

/// An always-ask rule's words, and whether its last is a prefix (`mkfs*`).
/// A trailing ` *` adds nothing: the words may be followed by anything.
fn ask_words(rule: &str) -> Option<(Vec<String>, bool)> {
    let rule = rule.trim();
    let (body, prefix) = match rule.strip_suffix('*') {
        Some(b) => (b, !b.ends_with(|c: char| c.is_whitespace())),
        None => (rule, false),
    };
    if body.contains('*') {
        return None;
    }
    let w = words(body);
    (!w.is_empty()).then_some((w, prefix))
}

/// Most brace alternatives tried, and brace groups read, in one command.
const MAX_BRACE_ALTS: usize = 8;

/// Expansions that may expand to nothing, for the collapsed reading: `$@`,
/// `$*`, `$1`, `$NAME`, `${…}`, an empty `$()`, an empty pair of
/// backticks, and cmd.exe's `%…%`.
static EMPTYABLE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\$(?:[@*#?!$0-9-]|[A-Za-z_][A-Za-z0-9_]*|\{[^}]*\}|\(\s*\))|``|%[^%\s]*%").unwrap()
});
/// A brace group with alternatives: `{b,}`, `{,}`.
static BRACES: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\{([^{}]*,[^{}]*)\}").unwrap());
/// Bash's `$'…'`, which reads escapes such as `\x72`.
static ANSI_C: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\$'((?:[^'\\]|\\.)*)'").unwrap());

/// What bash makes of the inside of a `$'…'`.
fn ansi_c(body: &str) -> String {
    let mut out = String::new();
    let mut it = body.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = it.next() else { break };
        let mut number = |radix: u32, max: usize, first: Option<char>| {
            let mut digits: String = first.into_iter().collect();
            while digits.len() < max && it.peek().is_some_and(|d| d.is_digit(radix)) {
                digits.push(it.next().unwrap_or_default());
            }
            u32::from_str_radix(&digits, radix).ok().and_then(char::from_u32)
        };
        let decoded = match e {
            'x' => number(16, 2, None),
            'u' => number(16, 4, None),
            'U' => number(16, 8, None),
            '0'..='7' => number(8, 3, Some(e)),
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            other => Some(other),
        };
        out.extend(decoded);
    }
    out
}

/// The collapsed readings of a command: what's left once the ways to hide
/// a word in it are taken out. Line continuations (`\` or cmd.exe's `^`
/// before a new line) go; `$'…'` is read as bash reads it; expansions that
/// may be empty go; each brace group is taken out, and in turn read as its
/// first, second, … alternative; then quotes, `^` and PowerShell's
/// backticks go. Backslashes stay, as Windows paths need them (the plain
/// reading takes them out).
fn collapsed(command: &str) -> Vec<String> {
    let mut text = command.replace("\\\r\n", "").replace("\\\n", "").replace("^\r\n", "").replace("^\n", "");
    text = ANSI_C.replace_all(&text, |c: &regex::Captures| ansi_c(&c[1])).into_owned();
    text = text.replace("$\"", "\"");
    text = EMPTYABLE.replace_all(&text, "").into_owned();
    let groups: Vec<Vec<String>> = BRACES
        .captures_iter(&text)
        .take(MAX_BRACE_ALTS)
        .map(|c| c[1].split(',').map(str::to_string).collect())
        .collect();
    let most = groups.iter().map(Vec::len).max().unwrap_or(0).min(MAX_BRACE_ALTS);
    // None: every group taken out; Some(k): each read as its k-th
    // alternative (its last, where it has fewer).
    let picks = std::iter::once(None).chain((0..most).map(Some));
    let strip = |t: String| t.chars().filter(|c| !matches!(c, '\'' | '"' | '^' | '`')).collect::<String>();
    picks
        .map(|pick| {
            let mut n = 0;
            let t = BRACES.replace_all(&text, |_: &regex::Captures| {
                let g = groups.get(n);
                n += 1;
                match (pick, g) {
                    (Some(k), Some(alts)) => alts.get(k).or(alts.last()).cloned().unwrap_or_default(),
                    _ => String::new(),
                }
            });
            strip(t.into_owned())
        })
        .collect()
}

/// Words split as the collapsed reading has them: lower case, hidden
/// characters out, backslashes kept.
fn collapsed_words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c.is_control() || OPERATORS.contains(&c))
        .map(|w| w.chars().filter(|c| !hidden(*c)).collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// A word of a simple command, and whether it's in command position: the
/// first word (after any `VAR=value`), or any word after a command that
/// runs others (RUNNERS) or gives root (ROOT_WORDS). Only there is a glob
/// read as one that could name a program.
type Word = (String, bool);

/// `NAME=value`, as a word before the command sets it.
fn is_assignment(w: &str) -> bool {
    let Some((name, _)) = w.split_once('=') else { return false };
    let mut c = name.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

fn runs_others(w: &str, glob_ok: bool) -> bool {
    RUNNERS.iter().any(|r| word_is_at(w, r, *r == "python", glob_ok))
}

fn gives_root(w: &str, glob_ok: bool) -> bool {
    ROOT_WORDS.iter().any(|r| word_is_at(w, r, false, glob_ok))
}

/// Which words of a simple command are in command position.
fn positions(words: Vec<String>) -> Vec<Word> {
    let first = words.iter().position(|w| !is_assignment(w));
    let mut after = false;
    let mut out = Vec::with_capacity(words.len());
    for (i, w) in words.into_iter().enumerate() {
        let at = after || Some(i) == first;
        if runs_others(&w, at) || gives_root(&w, at) {
            after = true;
        }
        out.push((w, at));
    }
    out
}

/// The ways a command is read, for the always-ask list and the root check,
/// each tried in turn: plainly (quotes and backslashes out), and
/// collapsed. Each reading is cut into the simple commands it holds (at
/// `;`, `&`, `|`, new lines, parentheses, braces and backticks), and each
/// of those into its words.
///
/// A command is read once (`Parsed::new`), and every rule matched against
/// that.
pub(crate) struct Parsed {
    command: String,
    /// Each reading's simple commands, and their words.
    commands: Vec<Vec<Vec<Word>>>,
    /// It asks for root (asks_for_root).
    pub root: bool,
}

/// The tools that give root, as the old check had them: kept beside the
/// words, as it also sees them inside a word (`(su -)`).
static ROOT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(?:^|[\s;|&(){}`'"/!])(?:sudo|dzdo|su|doas|pkexec)(?:$|[\s;|&(){}`'"])"#).unwrap());

impl Parsed {
    pub(crate) fn new(command: &str) -> Parsed {
        let cut = |t: &str, read: &dyn Fn(&str) -> Vec<String>| -> Vec<Vec<Word>> {
            t.split([';', '&', '|', '\n', '\r', '(', ')', '{', '}', '`']).map(read).filter(|w| !w.is_empty()).map(positions).collect()
        };
        let mut commands = vec![cut(command, &words)];
        commands.extend(collapsed(command).iter().map(|t| cut(t, &collapsed_words)));
        let root = ROOT_RE.is_match(command)
            || commands.iter().flatten().any(|cmd| {
                cmd.iter().any(|(w, at)| gives_root(w, *at))
                    // `machinectl shell` opens a shell as root by default.
                    || cmd.windows(2).any(|p| word_is_at(&p[0].0, "machinectl", false, p[0].1) && p[1].0 == "shell")
            });
        Parsed { command: command.to_string(), commands, root }
    }

    /// It only opens a shell, which a request as root (#120) can't use: it
    /// runs one command, with no terminal and nothing on stdin, so a shell
    /// would end at once. `sudo -i`, `sudo -s`, `dzdo su -`, `su` without
    /// `-c`, `machinectl shell`, or a shell with no command or script
    /// (`bash`, `sudo bash -l`), as any of the simple commands in it; or one
    /// that starts with an option (`-i`, `-s`), which sudo or dzdo would read
    /// as its own. Read plainly. Not a safety check: it only tells the AI to
    /// send the command itself.
    pub(crate) fn opens_shell(&self) -> bool {
        self.commands.first().is_some_and(|cmds| cmds.iter().any(|c| opens_shell(c)))
    }
}

/// The shells that start interactive with no command or script.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ash", "ksh", "mksh", "csh", "tcsh", "fish"];

/// sudo's, dzdo's and doas's options that take the next word.
const ROOT_OPTIONS_WITH_A_VALUE: &[&str] = &["-u", "-g", "-p", "-c", "-h", "-r", "-t", "-d", "-a"];

/// Whether one simple command (its words, read plainly) only opens a shell.
fn opens_shell(words: &[Word]) -> bool {
    let name = |w: &str| forms(w).last().map(|f| f.to_string()).unwrap_or_default();
    let mut i = words.iter().position(|(w, _)| !is_assignment(w)).unwrap_or(words.len());
    if words.get(i).is_some_and(|(w, _)| w.starts_with('-')) {
        return true;
    }
    while let Some((w, _)) = words.get(i) {
        let n = name(w);
        match n.as_str() {
            "sudo" | "dzdo" | "doas" | "run0" | "pkexec" => {
                // Its options, then what it runs: nothing at all, after -i or
                // -s, is a shell.
                let mut shell = false;
                i += 1;
                while let Some((o, _)) = words.get(i) {
                    if o == "--" {
                        i += 1;
                        break;
                    }
                    if !o.starts_with('-') {
                        break;
                    }
                    if o == "--login" || o == "--shell" || (!o.starts_with("--") && o.contains(['i', 's'])) {
                        shell = true;
                    }
                    i += if ROOT_OPTIONS_WITH_A_VALUE.contains(&o.as_str()) { 2 } else { 1 };
                }
                if i >= words.len() {
                    return shell || n == "run0" || n == "pkexec";
                }
            }
            // A shell unless it's given a command.
            "su" | "runuser" => {
                return !words[i + 1..].iter().any(|(o, _)| {
                    o == "-c" || o.starts_with("--command") || o == "--session-command" || (o.starts_with('-') && !o.starts_with("--") && o.ends_with('c'))
                });
            }
            "machinectl" => return words.get(i + 1).is_some_and(|(o, _)| o == "shell"),
            s if SHELLS.contains(&s) => {
                let rest = &words[i + 1..];
                let command = rest.iter().any(|(o, _)| o.starts_with('-') && !o.starts_with("--") && o.contains('c'));
                let script = rest.iter().any(|(o, _)| !o.starts_with('-') && !o.starts_with('+'));
                return !command && !script;
            }
            _ => return false,
        }
    }
    false
}

/// A command's words, when it's one simple command made only of plain
/// words: printable ASCII, split at spaces, with nothing a shell
/// would read otherwise (quotes, backslashes, globs, `~`, `#`, `!`, or a
/// `NAME=value` first). Such a command runs as root as its words, with no
/// shell between (`sudo docker ps -a`), so a sudoers rule for that one
/// program allows it. Any other runs as `sh -c <command>`, and so does one
/// whose first word starts with `-`, which the tool would take for an
/// option of its own: the tool's first word is then `sh`.
pub(crate) fn plain_words(command: &str) -> Option<Vec<String>> {
    if !is_simple(command) || !command.chars().all(|c| c == ' ' || c.is_ascii_graphic()) {
        return None;
    }
    if command.contains(['\'', '"', '\\', '*', '?', '[', ']', '~', '#', '!']) {
        return None;
    }
    let words: Vec<String> = command.split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
    let first = words.first()?;
    (!first.contains('=') && !first.starts_with('-') && !BUILTINS.contains(&first.as_str())).then_some(words)
}

/// What a shell does itself, with no program of that name to run.
const BUILTINS: &[&str] = &[
    ".", ":", "alias", "bg", "break", "cd", "command", "continue", "eval", "exec", "exit", "export", "fg", "getopts", "hash",
    "jobs", "read", "readonly", "return", "set", "shift", "source", "times", "trap", "type", "ulimit", "umask", "unalias",
    "unset", "wait",
];

/// The options a word gives: `-rf` is r and f, `--recursive` is recursive
/// and r. None for a word that isn't an option (or `--`, after which none
/// are).
fn flags_of(word: &str) -> Option<Vec<String>> {
    if let Some(long) = word.strip_prefix("--") {
        let long = long.split('=').next().unwrap_or(long);
        let first = long.chars().next()?;
        return Some(vec![long.to_string(), first.to_string()]);
    }
    let short = word.strip_prefix('-')?;
    (!short.is_empty()).then(|| short.chars().map(|c| c.to_string()).collect())
}

/// A rule of a command and its options alone (`rm -rf *`) matches the
/// command with those options in any order, split or long: `rm -fr`,
/// `rm -r -f`, `rm --recursive --force`.
fn flags_match(want: &[String], command: &Parsed) -> bool {
    let Some((name, opts)) = want.split_first() else { return false };
    let mut need: Vec<String> = Vec::new();
    for o in opts {
        match flags_of(o) {
            Some(f) if !o.starts_with("--") => need.extend(f),
            Some(f) => need.push(f[0].clone()),
            None => return false,
        }
    }
    if need.is_empty() {
        return false;
    }
    command.commands.iter().flatten().any(|cmd| {
        cmd.iter().enumerate().any(|(i, (w, at))| {
            if !word_is_at(w, name, false, *at) {
                return false;
            }
            let mut have: Vec<String> = Vec::new();
            for (a, _) in &cmd[i + 1..] {
                if a == "--" {
                    break;
                }
                have.extend(flags_of(a).unwrap_or_default());
            }
            need.iter().all(|n| have.contains(n))
        })
    })
}

/// Whether the always-ask rule `rule` is in `command`: its words, in order
/// and side by side, as words anywhere in it, whatever comes before or
/// after. So `reboot` is in `uptime; reboot`, `sudo reboot`,
/// `/sbin/reboot now` and `shutdown.exe /r`, and `rm -rf *` in
/// `cd /; rm -rf /tmp`. A command word counts as itself, as the name after
/// its last `/` or `\` (without `.exe` and the like), or, in command
/// position (`positions`), as a glob that could expand to the rule's word:
/// `/sbin/reb?ot` asks, `ls *.txt` doesn't.
///
/// It's read twice, and either reading counts: plainly, and collapsed
/// (`collapsed`: `re$@boot`, `re{,}boot`, `shut^down`, `Restart-Comp`uter`
/// are all caught). A rule of a command and options (`rm -rf *`) also
/// catches those options in any order, split or long (`rm -r -f`,
/// `rm --recursive --force`).
///
/// It doesn't catch a command that builds the name as it runs
/// (`$(echo re)boot`, base64): the always-ask list is a safety net, not a
/// wall.
#[cfg(test)]
pub(crate) fn ask_matches(rule: &str, command: &str) -> bool {
    ask_matches_parsed(rule, &Parsed::new(command))
}

fn ask_matches_parsed(rule: &str, command: &Parsed) -> bool {
    let Some((want, prefix)) = ask_words(rule) else { return false };
    command.commands.iter().flatten().any(|cmd| {
        cmd.windows(want.len())
            .any(|win| win.iter().zip(&want).enumerate().all(|(i, ((w, at), r))| word_is_at(w, r, prefix && i + 1 == want.len(), *at)))
    }) || (!prefix && flags_match(&want, command))
}

/// The programs that give root.
const ROOT_WORDS: &[&str] = &["sudo", "sudoedit", "dzdo", "su", "doas", "pkexec", "run0", "runuser"];

/// Whether a word of `command`, read as the always-ask list reads them,
/// names a program that gives root, or is a glob that could (`sud?`).
#[cfg(test)]
pub(crate) fn mentions_root(command: &str) -> bool {
    Parsed::new(command).commands.iter().flatten().flatten().any(|(w, at)| gives_root(w, *at))
}

/// A command that asks for root: one of the tools that give it (sudo,
/// sudoedit, dzdo, su, doas, pkexec, run0, runuser, `machinectl shell`),
/// read as the always-ask list reads words, anywhere in it, or a glob that
/// could name one.
#[cfg(test)]
pub(crate) fn asks_for_root(command: &str) -> bool {
    Parsed::new(command).root
}

/// The programs that run other commands they're given: an allow rule with a
/// wildcard that starts with one of them allows any command at all.
/// `python` stands for every name that starts with it (`python3.12`).
const RUNNERS: &[&str] = &[
    "env", "xargs", "timeout", "nice", "ionice", "nohup", "exec", "command", "builtin", "busybox", "stdbuf", "setsid",
    "chroot", "watch", "sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh", "fish", "python", "perl", "ruby", "node", "php",
    "awk", "gawk", "mawk", "find", "git", "ssh", "rsync", "tar", "docker", "kubectl", "cmd", "powershell", "pwsh",
    "sed", "time", "strace", "ltrace", "gdb", "nsenter", "unshare", "systemd-run", "flock", "parallel", "script", "sshpass",
    "expect", "screen", "tmux", "at", "batch", "iex", "invoke-expression", "start-process", "wsl",
];

/// The warning for an allow rule with a wildcard whose command runs other
/// commands (`timeout *`); None for any other rule. Saving one takes an
/// extra click.
pub(crate) fn runner_warning(pattern: &str) -> Option<String> {
    let p = pattern.trim();
    if !p.ends_with('*') {
        return None;
    }
    // Its first word, quotes out but backslashes kept, for a Windows path.
    let first = p
        .trim_end_matches('*')
        .split(|c: char| c.is_whitespace() || OPERATORS.contains(&c))
        .find(|w| !w.is_empty())?
        .replace(['\'', '"'], "")
        .to_lowercase();
    let runs = forms(&first).into_iter().any(|f| RUNNERS.iter().any(|r| f == *r || (*r == "python" && f.starts_with(r))));
    runs.then(|| format!("`{p}` lets the AI run any command, as root too where sudo needs no password."))
}

/// What happens to a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// It waits for you. `rule`: the always-ask rule that made it, if one did.
    Ask { rule: Option<String> },
    /// It runs now: it matches your allow rule.
    Rule(String),
    /// It runs now: the server is at "Run anything".
    Anything,
    /// It runs now: you allowed everything on this server until the AI tab
    /// closes.
    Session,
}

impl Decision {
    /// How it was approved, for the audit log and the panel: None when it
    /// waits for you.
    pub(crate) fn approved_by(&self) -> Option<String> {
        match self {
            Decision::Ask { .. } => None,
            Decision::Rule(r) => Some(format!("rule: {r}")),
            Decision::Anything => Some("level: run anything".into()),
            Decision::Session => Some("session: allow everything".into()),
        }
    }
}

/// The decision for `command` on a server whose settings are `policy`.
///
/// - An always-ask rule that matches makes it ask, whatever the level.
/// - "Allow everything here until this AI tab closes" (`session_allow`)
///   runs the rest, as level 3 does.
/// - A policy that couldn't be read, or is on hold, asks.
/// - At level 2, a command that asks for root (asks_for_root: sudo, dzdo,
///   su, doas, pkexec and the like) asks; otherwise an allow rule that
///   matches runs it.
/// - Anything else asks.
///
/// The app decides through decide_parsed, with the command read once.
#[cfg(test)]
pub(crate) fn decide(policy: &Policy, session_allow: bool, command: &str) -> Decision {
    decide_parsed(policy, session_allow, &Parsed::new(command), false)
}

/// The same for a request to run `command` as root (`as_root`, #120): it
/// counts as asking for root, so at levels 1 and 2 it always asks, and no
/// allow rule runs it.
#[cfg(test)]
pub(crate) fn decide_as(policy: &Policy, session_allow: bool, command: &str, as_root: bool) -> Decision {
    decide_parsed(policy, session_allow, &Parsed::new(command), as_root)
}

/// The same, for a command read already; `as_root` for a request to run it
/// as root. A policy that couldn't be read, or is on hold (`force_ask`),
/// asks, whatever the level and the session.
///
/// At level 3 (or with the session's "allow everything") a request as root
/// runs, but only with what this AI tab already has for root on the server:
/// a password you typed for it here, or none where the server asks for
/// none. Where the server wants a password this AI tab doesn't have, the
/// card asks you for it (mod.rs): nothing here ever supplies one.
pub(crate) fn decide_parsed(policy: &Policy, session_allow: bool, command: &Parsed, as_root: bool) -> Decision {
    if let Some(rule) = policy.ask.iter().find(|r| ask_matches_parsed(r, command)) {
        return Decision::Ask { rule: Some(rule.clone()) };
    }
    if policy.force_ask.is_some() {
        return Decision::Ask { rule: None };
    }
    if session_allow || policy.level == RUN_ANYTHING {
        return if session_allow { Decision::Session } else { Decision::Anything };
    }
    if policy.level == ALLOWED_COMMANDS && !command.root && !as_root {
        if let Some(rule) = policy.allow.iter().find(|r| allow_matches_on(r, &command.command, policy.windows)) {
            return Decision::Rule(rule.clone());
        }
    }
    Decision::Ask { rule: None }
}

/// A rule as you typed it, checked: trimmed, not empty, not too long, `*`
/// only at its end and not alone. An allow rule must be one simple command
/// itself, or it could never match.
pub(crate) fn check_rule(kind: &str, pattern: &str) -> Result<String, String> {
    let p = pattern.trim();
    if p.is_empty() {
        return Err("Write a command.".into());
    }
    if p.len() > MAX_RULE {
        return Err(format!("A rule can be at most {MAX_RULE} characters long."));
    }
    let body = p.strip_suffix('*').unwrap_or(p);
    if body.contains('*') {
        return Err("A * can only go at the end of a rule.".into());
    }
    if body.trim().is_empty() {
        return Err("Write the command before the *.".into());
    }
    match kind {
        "allow" if !is_simple(p) => Err(
            "An allowed command must be one simple command: no ; & | ` $ > < ( ) { }, new lines or hidden characters.".into(),
        ),
        "allow" => Ok(p.to_string()),
        "ask" if ask_words(p).is_none() => Err("Write the command's words.".into()),
        "ask" => Ok(p.to_string()),
        _ => Err("That isn't a kind of rule.".into()),
    }
}

// ---------------------------------------------------------------------------
// Kept in the vault, on this device
// ---------------------------------------------------------------------------

/// Device-local, like `walls`: not in SYNCED_TABLES, no sync columns, no
/// sync triggers. `scope` is `server`, `folder` or `default` (with
/// `scope_id` 0).
pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS ai_levels (
    scope TEXT NOT NULL CHECK (scope IN ('server', 'folder', 'default')),
    scope_id INTEGER NOT NULL,
    level INTEGER NOT NULL CHECK (level BETWEEN 1 AND 3),
    PRIMARY KEY (scope, scope_id)
);
CREATE TABLE IF NOT EXISTS ai_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT NOT NULL CHECK (scope IN ('server', 'folder', 'default')),
    scope_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('allow', 'ask')),
    pattern TEXT NOT NULL,
    UNIQUE (scope, scope_id, kind, pattern)
);
CREATE TRIGGER IF NOT EXISTS ai_trust_server_gone AFTER DELETE ON servers BEGIN
    DELETE FROM ai_levels WHERE scope = 'server' AND scope_id = OLD.id;
    DELETE FROM ai_rules WHERE scope = 'server' AND scope_id = OLD.id;
END;
CREATE TRIGGER IF NOT EXISTS ai_trust_folder_gone AFTER DELETE ON folders BEGIN
    DELETE FROM ai_levels WHERE scope = 'folder' AND scope_id = OLD.id;
    DELETE FROM ai_rules WHERE scope = 'folder' AND scope_id = OLD.id;
END;";

/// The always-ask list a vault starts with, for every server. You can
/// remove any of them in Settings › AI tools.
/// Only a vault that has no list yet gets them, so a rule you removed never
/// comes back.
pub(crate) const DEFAULT_ASK: &[&str] = &[
    "reboot", "shutdown", "poweroff", "halt", "init 0", "init 6", "telinit", "rm -rf *", "rm -fr *", "mkfs*", "dd *", ":(){",
    "Restart-Computer", "Stop-Computer", "format", "diskpart",
];

fn db(code: &'static str) -> impl Fn(rusqlite::Error) -> String {
    move |e| format!("[DATABASE] {code}: {e}")
}

/// This install's id, outside every vault (beside ai_tools.json): made
/// the first time. The levels and allow rules a vault holds count only on
/// the install that last opened it (create_schema).
pub(crate) fn install_id(app: &tauri::AppHandle) -> String {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Install {
        id: String,
    }
    const FILE: &str = "ai_install.json";
    if let Some(i) = crate::local_open::read_setting::<Install>(app, FILE).filter(|i| i.id.len() >= 16) {
        return i.id;
    }
    let id = super::channel::random_hex(16);
    // Not kept, the next open gets another, and the vault's levels and
    // allow rules are reset: the safe way to fail.
    if let Err(e) = crate::local_open::write_setting(app, FILE, &Install { id: id.clone() }, "this install's id") {
        crate::applog::write_line("ai", &format!("{e}: AI approval levels will be reset at the next open."));
    }
    id
}

/// The vault's key holding the install that last opened it. In sync_meta,
/// which never syncs.
const DEVICE_KEY: &str = "ai_trust_device";

fn seed_defaults(conn: &Connection) -> Result<(), String> {
    for p in DEFAULT_ASK {
        conn.execute("INSERT OR IGNORE INTO ai_rules (scope, scope_id, kind, pattern) VALUES ('default', 0, 'ask', ?1)", [p])
            .map_err(db("AI_TRUST_DEFAULTS"))?;
    }
    Ok(())
}

/// Make the tables, and the first time, the default always-ask list. Every
/// vault open runs it, on `servers` and `folders` as they are by then.
///
/// The levels and allow rules belong to the install that set them
/// (`install`): a vault last opened by another one (exported and imported,
/// or copied by hand) loses them, and keeps its always-ask list (the
/// defaults again, if it has none). So does one with the tables but no
/// install named: only a vault whose tables are made now takes this
/// install with nothing reset. Returns whether it changed anything, so the
/// vault is saved.
pub(crate) fn create_schema(conn: &Connection, install: &str) -> Result<bool, String> {
    let had: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ai_rules'", [], |_| Ok(()))
        .optional()
        .map_err(db("AI_TRUST_SCHEMA"))?
        .is_some();
    conn.execute_batch(SCHEMA).map_err(db("AI_TRUST_SCHEMA"))?;
    let mut changed = false;
    if !had {
        seed_defaults(conn)?;
        changed = true;
    }
    conn.execute("CREATE TABLE IF NOT EXISTS sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)", [])
        .map_err(db("AI_TRUST_DEVICE"))?;
    let last: Option<String> = conn
        .query_row("SELECT value FROM sync_meta WHERE key = ?1", [DEVICE_KEY], |r| r.get(0))
        .optional()
        .map_err(db("AI_TRUST_DEVICE"))?;
    if last.as_deref() == Some(install) {
        return Ok(changed);
    }
    // Tables there already, but another install's, or none named: reset.
    // Only tables made just now take this install with nothing to reset.
    if had {
        conn.execute_batch("DELETE FROM ai_levels; DELETE FROM ai_rules WHERE kind = 'allow';").map_err(db("AI_TRUST_RESET"))?;
        let asks: i64 = conn.query_row("SELECT COUNT(*) FROM ai_rules WHERE kind = 'ask'", [], |r| r.get(0)).map_err(db("AI_TRUST_RESET"))?;
        if asks == 0 {
            seed_defaults(conn)?;
        }
    }
    conn.execute(
        "INSERT INTO sync_meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![DEVICE_KEY, install],
    )
    .map_err(db("AI_TRUST_DEVICE"))?;
    Ok(true)
}

/// What holds for one server: its level and every rule on the way up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    pub level: u8,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    /// The server's saved OS is Windows (server_os.rs): `%` and `^` are
    /// operators to its allow rules.
    pub windows: bool,
    /// Every command asks, whatever the level and the session's allow: the
    /// policy couldn't be read, or a change to it waits for your OK. Why,
    /// in words for the request's card.
    pub force_ask: Option<String>,
}

/// What the card says when the settings can't be read.
pub(crate) const UNREADABLE: &str = "Submarine couldn't read this server's AI approval, so it asks.";

impl Policy {
    /// What holds when the settings can't be read: every command asks.
    pub(crate) fn unreadable() -> Policy {
        Policy { force_ask: Some(UNREADABLE.into()), ..Policy::default() }
    }
}

impl Default for Policy {
    /// Ask every time: what holds when nothing can be read.
    fn default() -> Self {
        Policy { level: ASK_EVERY_TIME, allow: Vec::new(), ask: Vec::new(), windows: false, force_ask: None }
    }
}

/// The scopes that count for a server, nearest first: the server, its
/// folder and each one above, the default. Without a saved server (a quick
/// connect), the default only.
fn chain(conn: &Connection, server_id: Option<i64>) -> rusqlite::Result<Vec<(&'static str, i64)>> {
    let mut out = Vec::new();
    if let Some(id) = server_id {
        out.push(("server", id));
        let mut folder: Option<i64> = conn.query_row("SELECT folder_id FROM servers WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        while let Some(f) = folder {
            if out.iter().any(|(s, i)| *s == "folder" && *i == f) || out.len() > MAX_DEPTH {
                break;
            }
            out.push(("folder", f));
            folder = conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [f], |r| r.get(0)).optional()?.flatten();
        }
    }
    out.push(("default", 0));
    Ok(out)
}

fn level_of(conn: &Connection, scope: &str, id: i64) -> rusqlite::Result<Option<u8>> {
    conn.query_row("SELECT level FROM ai_levels WHERE scope = ?1 AND scope_id = ?2", params![scope, id], |r| r.get(0)).optional()
}

fn try_policy(conn: &Connection, server_id: Option<i64>) -> rusqlite::Result<Policy> {
    let mut p = Policy::default();
    let mut level = None;
    let mut stmt = conn.prepare("SELECT kind, pattern FROM ai_rules WHERE scope = ?1 AND scope_id = ?2 ORDER BY id")?;
    for (scope, id) in chain(conn, server_id)? {
        if level.is_none() {
            level = level_of(conn, scope, id)?;
        }
        for row in stmt.query_map(params![scope, id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (kind, pattern) = row?;
            if kind == "allow" { &mut p.allow } else { &mut p.ask }.push(pattern);
        }
    }
    p.level = level.filter(|l| (1..=3).contains(l)).unwrap_or(ASK_EVERY_TIME);
    // Read leniently: a vault without the column has no OS saved.
    p.windows = server_id.is_some_and(|id| {
        conn.query_row("SELECT os FROM servers WHERE id = ?1", [id], |r| r.get::<_, Option<String>>(0))
            .ok()
            .flatten()
            .is_some_and(|os| os == "windows")
    });
    Ok(p)
}

/// What holds for `server_id`. Anything that can't be read asks every time,
/// even where you allowed everything (Policy::unreadable). A server whose
/// policy a teammate's change loosened, held for your OK, asks every time
/// too, with no allow rules, and keeps the always-ask rules it lost.
pub(crate) fn policy_for(conn: &Connection, server_id: Option<i64>) -> Policy {
    let held = server_id.map(|id| held_policy(conn, id));
    match (try_policy(conn, server_id), held) {
        (Err(_), _) | (_, Some(Err(_))) => Policy::unreadable(),
        (Ok(mut p), Some(Ok(Some(h)))) => {
            p.level = ASK_EVERY_TIME;
            p.allow.clear();
            for lost in h.lost {
                if !p.ask.contains(&lost) {
                    p.ask.push(lost);
                }
            }
            p.force_ask = Some(HELD_NOTE.into());
            p
        }
        (Ok(p), _) => p,
    }
}

// ---------------------------------------------------------------------------
// A teammate's move that loosens a server's policy, held (approvals.rs)
// ---------------------------------------------------------------------------

/// The item a loosened server is held by in `held_approvals`, followed by
/// HeldPolicy as JSON.
pub(crate) const HELD_ITEM: &str = "ai_policy:";

/// What the card says while a server is held.
pub(crate) const HELD_NOTE: &str =
    "Someone you share this profile with moved this server, added it, or changed where it connects to, and the AI may now do more here. Until you look at it in Waiting for your OK, every command asks.";

/// What a move did to a server's policy, for "Waiting for your OK".
#[derive(Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeldPolicy {
    /// The level it has now.
    pub level: u8,
    /// Allow rules it got.
    pub allow: usize,
    /// Always-ask rules it lost: kept while it's held.
    pub lost: Vec<String>,
    /// What was moved (the server, or a folder it's in), and into which
    /// folder (empty: the top level).
    pub moved: String,
    pub moved_folder: bool,
    pub into: String,
    /// What the change was: `moved` (the server or a folder above it),
    /// `added` (a server new here, where it may do more than by default),
    /// or `pointed` (where it connects to, while the AI may run commands
    /// there without asking).
    #[serde(default = "moved")]
    pub change: String,
}

fn moved() -> String {
    "moved".into()
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(())).optional()?.is_some())
}

fn uuid_of(conn: &Connection, server_id: i64) -> rusqlite::Result<Option<String>> {
    Ok(conn.query_row("SELECT uuid FROM servers WHERE id = ?1", [server_id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten())
}

/// The server's held move, if one is held.
fn held_policy(conn: &Connection, server_id: i64) -> rusqlite::Result<Option<HeldPolicy>> {
    if !has_table(conn, "held_approvals")? {
        return Ok(None);
    }
    let Some(uuid) = uuid_of(conn, server_id)? else { return Ok(None) };
    let item: Option<String> = conn
        .query_row("SELECT item FROM held_approvals WHERE uuid = ?1 AND item LIKE 'ai_policy:%' LIMIT 1", [&uuid], |r| r.get(0))
        .optional()?;
    // One that can't be read is held all the same.
    Ok(item.map(|i| {
        serde_json::from_str(&i[HELD_ITEM.len()..]).unwrap_or(HeldPolicy {
            level: RUN_ANYTHING,
            allow: 0,
            lost: Vec::new(),
            moved: String::new(),
            moved_folder: false,
            into: String::new(),
            change: moved(),
        })
    }))
}

/// Let a server's held move go: you allowed it, or set the server's level
/// yourself.
fn release_held(conn: &Connection, server_id: i64) -> rusqlite::Result<()> {
    if has_table(conn, "held_approvals")? {
        if let Some(uuid) = uuid_of(conn, server_id)? {
            conn.execute("DELETE FROM held_approvals WHERE uuid = ?1 AND item LIKE 'ai_policy:%'", [&uuid])?;
        }
    }
    Ok(())
}

/// "Keep as it was": the move's hold goes, and the server stays at "Ask
/// every time" (its own level, on this device) until you change it.
pub(crate) fn keep_as_it_was(conn: &Connection, server_id: i64) -> Result<(), String> {
    set_level(conn, "server", server_id, Some(ASK_EVERY_TIME))
}

/// Where a server connects to: its host, port, user, saved login, jump
/// host and proxy.
type Target = Vec<rusqlite::types::Value>;
const TARGET: &str = "host, port, username, credential_id, jump_host_id, proxy_type, proxy_host, proxy_port";
const TARGET_COLUMNS: usize = 8;

/// The tree as it was before a sync merged: each server's policy, its
/// folder and where it connects to, and each folder's parent.
pub(crate) struct TreeBefore {
    servers: std::collections::HashMap<i64, (Policy, Option<i64>, Option<Target>)>,
    parents: std::collections::HashMap<i64, Option<i64>>,
}

fn id_pairs(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<(i64, Option<i64>)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Where each server connects to; None where that can't be read.
fn targets(conn: &Connection) -> std::collections::HashMap<i64, Target> {
    let read = || -> rusqlite::Result<std::collections::HashMap<i64, Target>> {
        let mut stmt = conn.prepare(&format!("SELECT id, {TARGET} FROM servers"))?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, (1..=TARGET_COLUMNS).map(|i| r.get(i)).collect::<rusqlite::Result<Target>>()?)))?;
        rows.collect()
    };
    read().unwrap_or_default()
}

/// Just before a shared profile's sync merges: what each server's policy
/// is, and where it connects to. None when no sync could let the AI do
/// more: no level or rule is set on a folder (a server's own go with it,
/// and the default is the same everywhere) and no server runs anything
/// without asking, so a profile that doesn't use levels pays nothing.
pub(crate) fn before_sync(conn: &Connection) -> Option<TreeBefore> {
    let used: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM ai_levels WHERE scope = 'folder' OR level > 1) OR EXISTS (SELECT 1 FROM ai_rules WHERE scope = 'folder')",
            [],
            |r| r.get(0),
        )
        .ok()?;
    if !used {
        return None;
    }
    let mut targets = targets(conn);
    let mut servers = std::collections::HashMap::new();
    for (id, folder) in id_pairs(conn, "SELECT id, folder_id FROM servers").ok()? {
        servers.insert(id, (try_policy(conn, Some(id)).ok()?, folder, targets.remove(&id)));
    }
    let parents = id_pairs(conn, "SELECT id, parent_id FROM folders").ok()?.into_iter().collect();
    Some(TreeBefore { servers, parents })
}

/// Whether going from `old` to `new` lets the AI do more: a higher level,
/// allow rules that count (at level 2 or above) it didn't have, or
/// always-ask rules it lost. The lost ones, and how many allow rules it got.
fn loosened(old: &Policy, new: &Policy) -> Option<(usize, Vec<String>)> {
    let gained = if new.level >= ALLOWED_COMMANDS { new.allow.iter().filter(|r| !old.allow.contains(r)).count() } else { 0 };
    let lost: Vec<String> = old.ask.iter().filter(|r| !new.ask.contains(r)).cloned().collect();
    (new.level > old.level || gained > 0 || !lost.is_empty()).then_some((gained, lost))
}

fn name_of(conn: &Connection, table: &str, id: Option<i64>) -> String {
    id.and_then(|id| conn.query_row(&format!("SELECT name FROM {table} WHERE id = ?1"), [id], |r| r.get::<_, Option<String>>(0)).ok().flatten())
        .unwrap_or_default()
}

/// Just after a shared profile's sync merged (every record, deletion and
/// link in place), each server the sync let the AI do more on is held
/// (approvals.rs's `held_approvals`, item HELD_ITEM), until you allow it or
/// keep it as it was:
///
/// - one whose policy it loosened, by moving it or a folder above it;
/// - one new here (made, or deleted and made again) that the AI may run
///   commands on without asking (its level above "Ask every time");
/// - one the AI may run commands on without asking whose host, port,
///   user, saved login, jump host or proxy it changed.
///
/// One held already is held again with what it is now.
pub(crate) fn hold_loosened(conn: &Connection, before: TreeBefore) -> Result<(), String> {
    let fail = db("AI_POLICY_HOLD");
    if !has_table(conn, "held_approvals").map_err(&fail)? {
        return Ok(());
    }
    let mut now_targets = targets(conn);
    for (id, folder) in id_pairs(conn, "SELECT id, folder_id FROM servers").map_err(&fail)? {
        let Some(uuid) = uuid_of(conn, id).map_err(&fail)? else { continue };
        let new = try_policy(conn, Some(id)).unwrap_or_else(|_| Policy::unreadable());
        let target = now_targets.remove(&id);
        let server = name_of(conn, "servers", Some(id));
        let held = match before.servers.get(&id) {
            // New here: held wherever the AI may run commands on it without
            // asking, so a careful server deleted and made again under a
            // loose default isn't let through.
            None => {
                if new.level <= ASK_EVERY_TIME {
                    continue;
                }
                HeldPolicy {
                    level: new.level,
                    allow: new.allow.len(),
                    lost: Vec::new(),
                    moved: server,
                    moved_folder: false,
                    into: name_of(conn, "folders", folder),
                    change: "added".into(),
                }
            }
            Some((old, old_folder, old_target)) => match loosened(old, &new) {
                Some((allow, lost)) => {
                    // What moved: the server, or the nearest folder above
                    // it whose parent changed.
                    let (moved, moved_folder, into) = if folder != *old_folder {
                        (server, false, name_of(conn, "folders", folder))
                    } else {
                        let mut at = folder;
                        let mut found = None;
                        for _ in 0..MAX_DEPTH {
                            let Some(f) = at else { break };
                            let parent: Option<i64> =
                                conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [f], |r| r.get(0)).optional().map_err(&fail)?.flatten();
                            if before.parents.get(&f) != Some(&parent) {
                                found = Some((name_of(conn, "folders", Some(f)), true, name_of(conn, "folders", parent)));
                                break;
                            }
                            at = parent;
                        }
                        found.unwrap_or_else(|| (server, false, name_of(conn, "folders", folder)))
                    };
                    HeldPolicy { level: new.level, allow, lost, moved, moved_folder, into, change: "moved".into() }
                }
                None if new.level > ASK_EVERY_TIME && old_target.is_some() && *old_target != target => HeldPolicy {
                    level: new.level,
                    allow: 0,
                    lost: Vec::new(),
                    moved: server,
                    moved_folder: false,
                    into: String::new(),
                    change: "pointed".into(),
                },
                None => continue,
            },
        };
        let item = format!("{HELD_ITEM}{}", serde_json::to_string(&held).map_err(|e| e.to_string())?);
        conn.execute("DELETE FROM held_approvals WHERE uuid = ?1 AND item LIKE 'ai_policy:%'", [&uuid]).map_err(&fail)?;
        conn.execute("INSERT OR IGNORE INTO held_approvals (uuid, item, since) VALUES (?1, ?2, strftime('%s','now'))", params![uuid, item])
            .map_err(&fail)?;
    }
    Ok(())
}

/// A level set on a server or folder (or the default), for Settings.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub(crate) struct LevelRow {
    pub scope: String,
    pub scope_id: i64,
    /// The server's or folder's name; empty for the default.
    pub name: String,
    pub level: u8,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub(crate) struct RuleRow {
    pub id: i64,
    pub scope: String,
    pub scope_id: i64,
    pub name: String,
    pub kind: String,
    pub pattern: String,
}

/// Everything set, for Settings › AI tools.
#[derive(Serialize, Debug)]
pub(crate) struct TrustView {
    pub default_level: u8,
    /// The servers and folders set above "Ask every time".
    pub levels: Vec<LevelRow>,
    /// Every rule, the default's first.
    pub rules: Vec<RuleRow>,
}

const NAME: &str = "COALESCE(CASE scope WHEN 'server' THEN (SELECT name FROM servers WHERE id = scope_id) \
     WHEN 'folder' THEN (SELECT name FROM folders WHERE id = scope_id) END, '')";

fn rules_where(conn: &Connection, filter: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<RuleRow>, String> {
    let sql = format!(
        "SELECT id, scope, scope_id, {NAME}, kind, pattern FROM ai_rules {filter} \
         ORDER BY scope = 'default' DESC, scope, lower({NAME}), kind, id"
    );
    let mut stmt = conn.prepare(&sql).map_err(db("AI_RULES_LIST"))?;
    let rows = stmt
        .query_map(args, |r| {
            Ok(RuleRow { id: r.get(0)?, scope: r.get(1)?, scope_id: r.get(2)?, name: r.get(3)?, kind: r.get(4)?, pattern: r.get(5)? })
        })
        .map_err(db("AI_RULES_LIST"))?;
    rows.collect::<Result<_, _>>().map_err(db("AI_RULES_LIST"))
}

pub(crate) fn view(conn: &Connection) -> Result<TrustView, String> {
    let default_level = level_of(conn, "default", 0).map_err(db("AI_LEVELS_LIST"))?.unwrap_or(ASK_EVERY_TIME);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT scope, scope_id, {NAME}, level FROM ai_levels WHERE scope != 'default' AND level > 1 ORDER BY scope DESC, lower({NAME})"
        ))
        .map_err(db("AI_LEVELS_LIST"))?;
    let levels = stmt
        .query_map([], |r| Ok(LevelRow { scope: r.get(0)?, scope_id: r.get(1)?, name: r.get(2)?, level: r.get(3)? }))
        .map_err(db("AI_LEVELS_LIST"))?
        .collect::<Result<_, _>>()
        .map_err(db("AI_LEVELS_LIST"))?;
    Ok(TrustView { default_level, levels, rules: rules_where(conn, "", &[])? })
}

/// One server's or folder's own settings, and what it gets from above, for
/// its editor.
#[derive(Serialize, Debug)]
pub(crate) struct ScopeView {
    /// Set on it; None: it follows its folder or the default.
    pub own_level: Option<u8>,
    /// What it follows when it has none of its own.
    pub inherited_level: u8,
    /// Where that comes from: a folder's name, or empty for the default.
    pub inherited_from: String,
    /// Its own rules.
    pub rules: Vec<RuleRow>,
    /// How many rules it gets from above, of each kind.
    pub inherited_allow: usize,
    pub inherited_ask: usize,
}

pub(crate) fn scope_view(conn: &Connection, scope: &str, id: i64) -> Result<ScopeView, String> {
    check_scope(scope)?;
    let id = if scope == "default" { 0 } else { id };
    let full = match scope {
        "server" => chain(conn, Some(id)),
        "folder" => chain(conn, None).map(|d| {
            // The folder and those above it, then the default.
            let mut out = vec![("folder", id)];
            let mut f: Option<i64> = conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| r.get(0)).ok().flatten();
            while let Some(x) = f {
                if out.iter().any(|(_, i)| *i == x) || out.len() > MAX_DEPTH {
                    break;
                }
                out.push(("folder", x));
                f = conn.query_row("SELECT parent_id FROM folders WHERE id = ?1", [x], |r| r.get(0)).ok().flatten();
            }
            out.extend(d);
            out
        }),
        _ => Ok(vec![("default", 0)]),
    }
    .map_err(db("AI_SCOPE"))?;
    let own_level = level_of(conn, scope, id).map_err(db("AI_SCOPE"))?;
    let (mut inherited_level, mut inherited_from) = (ASK_EVERY_TIME, String::new());
    for (s, i) in full.iter().skip(1) {
        if let Some(l) = level_of(conn, s, *i).map_err(db("AI_SCOPE"))? {
            inherited_level = l;
            if *s == "folder" {
                inherited_from = conn.query_row("SELECT name FROM folders WHERE id = ?1", [i], |r| r.get(0)).unwrap_or_default();
            }
            break;
        }
    }
    let (mut inherited_allow, mut inherited_ask) = (0, 0);
    for (s, i) in full.iter().skip(1) {
        let count = |kind: &str| -> Result<usize, String> {
            conn.query_row("SELECT COUNT(*) FROM ai_rules WHERE scope = ?1 AND scope_id = ?2 AND kind = ?3", params![s, i, kind], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
                .map_err(db("AI_SCOPE"))
        };
        inherited_allow += count("allow")?;
        inherited_ask += count("ask")?;
    }
    let rules = rules_where(conn, "WHERE scope = ?1 AND scope_id = ?2", &[&scope, &id])?;
    Ok(ScopeView { own_level, inherited_level, inherited_from, rules, inherited_allow, inherited_ask })
}

fn check_scope(scope: &str) -> Result<(), String> {
    if matches!(scope, "server" | "folder" | "default") {
        Ok(())
    } else {
        Err("That isn't a server, a folder or the default.".into())
    }
}

/// The server or folder must exist.
fn check_target(conn: &Connection, scope: &str, id: i64) -> Result<(), String> {
    check_scope(scope)?;
    let table = match scope {
        "server" => "servers",
        "folder" => "folders",
        _ => return Ok(()),
    };
    let found = conn
        .query_row(&format!("SELECT 1 FROM {table} WHERE id = ?1"), [id], |_| Ok(()))
        .optional()
        .map_err(db("AI_TRUST_TARGET"))?
        .is_some();
    if found {
        Ok(())
    } else {
        Err(format!("That {scope} isn't in this profile any more."))
    }
}

/// Set a level, or (None) take it off, so it follows its folder or the
/// default. The default taken off is "Ask every time".
pub(crate) fn set_level(conn: &Connection, scope: &str, id: i64, level: Option<u8>) -> Result<(), String> {
    check_target(conn, scope, id)?;
    // A server's level you set is yours: a move held for it goes.
    if scope == "server" {
        release_held(conn, id).map_err(db("AI_LEVEL_SET"))?;
    }
    let id = if scope == "default" { 0 } else { id };
    match level {
        Some(l) if (1..=3).contains(&l) => conn
            .execute(
                "INSERT INTO ai_levels (scope, scope_id, level) VALUES (?1, ?2, ?3) ON CONFLICT (scope, scope_id) DO UPDATE SET level = excluded.level",
                params![scope, id, l],
            )
            .map(|_| ()),
        Some(_) => return Err("A level is 1, 2 or 3.".into()),
        None => conn.execute("DELETE FROM ai_levels WHERE scope = ?1 AND scope_id = ?2", params![scope, id]).map(|_| ()),
    }
    .map_err(db("AI_LEVEL_SET"))
}

/// Add a rule; one already there is fine. Returns the rule as kept.
pub(crate) fn add_rule(conn: &Connection, scope: &str, id: i64, kind: &str, pattern: &str) -> Result<String, String> {
    check_target(conn, scope, id)?;
    let id = if scope == "default" { 0 } else { id };
    let pattern = check_rule(kind, pattern)?;
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_rules WHERE scope = ?1 AND scope_id = ?2 AND kind = ?3", params![scope, id, kind], |r| r.get(0))
        .map_err(db("AI_RULE_ADD"))?;
    if n >= MAX_RULES {
        return Err(format!("A list holds up to {MAX_RULES} rules."));
    }
    conn.execute("INSERT OR IGNORE INTO ai_rules (scope, scope_id, kind, pattern) VALUES (?1, ?2, ?3, ?4)", params![scope, id, kind, pattern])
        .map_err(db("AI_RULE_ADD"))?;
    Ok(pattern)
}

/// Remove a rule; returns what it was, for the audit log.
pub(crate) fn remove_rule(conn: &Connection, rule_id: i64) -> Result<Option<RuleRow>, String> {
    let row = rules_where(conn, "WHERE id = ?1", &[&rule_id])?.into_iter().next();
    conn.execute("DELETE FROM ai_rules WHERE id = ?1", [rule_id]).map_err(db("AI_RULE_REMOVE"))?;
    Ok(row)
}

/// Whether a level set on `scope` `id` reaches `server_id`: its own, one of
/// its folders', or the default.
pub(crate) fn reaches(conn: &Connection, server_id: Option<i64>, scope: &str, id: i64) -> bool {
    match scope {
        "default" => true,
        "server" => server_id == Some(id),
        _ => chain(conn, server_id).map(|c| c.contains(&("folder", id))).unwrap_or(true),
    }
}

/// What a scope is called, for the audit log.
pub(crate) fn describe(conn: &Connection, scope: &str, id: i64) -> String {
    let name = |table: &str| conn.query_row(&format!("SELECT name FROM {table} WHERE id = ?1"), [id], |r| r.get::<_, String>(0)).unwrap_or_default();
    match scope {
        "server" => format!("server {} (id {id})", name("servers")),
        "folder" => format!("folder {} (id {id})", name("folders")),
        _ => "every server".into(),
    }
}

#[cfg(test)]
mod tests;
