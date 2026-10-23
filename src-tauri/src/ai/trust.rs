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
            !rest.chars().any(char::is_whitespace) && !last_word.split(['/', '\\']).any(|seg| seg == "..")
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

/// Whether the shell glob `pat` (`?`, `*`, `[…]`) matches all of `text`.
fn glob_match(pat: &[char], text: &[char]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some('*') => (0..=text.len()).any(|i| glob_match(&pat[1..], &text[i..])),
        Some('?') => !text.is_empty() && glob_match(&pat[1..], &text[1..]),
        Some('[') => {
            let Some(close) = pat.iter().skip(2).position(|c| *c == ']').map(|p| p + 2) else {
                return text.first() == Some(&'[') && glob_match(&pat[1..], &text[1..]);
            };
            let Some(c) = text.first() else { return false };
            let set = &pat[1..close];
            let (negate, set) = match set.first() {
                Some('!' | '^') => (true, &set[1..]),
                _ => (false, set),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == '-' {
                    hit |= (set[i]..=set[i + 2]).contains(c);
                    i += 3;
                } else {
                    hit |= set[i] == *c;
                    i += 1;
                }
            }
            hit != negate && glob_match(&pat[close + 1..], &text[1..])
        }
        Some(p) => text.first() == Some(p) && glob_match(&pat[1..], &text[1..]),
    }
}

/// A command word that names `name` (or, `prefix`, something starting with
/// it): as itself, its name after the last `/` or `\`, without `.exe` and
/// the like, or as a glob that could expand to it (`sud?`, `/sbin/reb?ot`).
/// A glob of wildcards alone (`*`) counts for nothing: it would name
/// everything.
fn word_is(word: &str, name: &str, prefix: bool) -> bool {
    forms(word).into_iter().any(|f| {
        if is_glob(f) {
            if !f.chars().any(|c| !matches!(c, '?' | '*' | '[' | ']' | '!' | '^' | '-')) {
                return false;
            }
            let pat: Vec<char> = f.chars().collect();
            let name: Vec<char> = name.chars().collect();
            glob_match(&pat, &name) || (prefix && (name.len()..name.len() + 24).any(|n| {
                // Any longer name starting with it: tried with the glob's
                // own letters as filler, which is enough for `?` and `*`.
                let mut longer = name.clone();
                longer.resize(n, 'x');
                glob_match(&pat, &longer)
            }))
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

/// The ways a command is read into words for the always-ask list and the
/// root check, each tried in turn: plainly (quotes and backslashes out),
/// and collapsed.
///
/// The same readings are also cut into the simple commands they hold (at
/// `;`, `&`, `|`, new lines, parentheses, braces and backticks), for
/// options.
///
/// A command is read once (`Parsed::new`), and every rule matched against
/// that.
pub(crate) struct Parsed {
    command: String,
    /// Each reading's words.
    words: Vec<Vec<String>>,
    /// Each reading's simple commands, and their words.
    commands: Vec<Vec<Vec<String>>>,
    /// It asks for root (asks_for_root).
    pub root: bool,
}

/// The tools that give root, as the old check had them: kept beside the
/// words, as it also sees them inside a word (`(su -)`).
static ROOT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(?:^|[\s;|&(){}`'"/!])(?:sudo|dzdo|su|doas|pkexec)(?:$|[\s;|&(){}`'"])"#).unwrap());

impl Parsed {
    pub(crate) fn new(command: &str) -> Parsed {
        let cut = |t: &str, read: &dyn Fn(&str) -> Vec<String>| -> Vec<Vec<String>> {
            t.split(|c: char| matches!(c, ';' | '&' | '|' | '\n' | '\r' | '(' | ')' | '{' | '}' | '`')).map(read).filter(|w| !w.is_empty()).collect()
        };
        let readings = collapsed(command);
        let mut words_ = vec![words(command)];
        words_.extend(readings.iter().map(|t| collapsed_words(t)));
        let mut commands = vec![cut(command, &words)];
        commands.extend(readings.iter().map(|t| cut(t, &collapsed_words)));
        let root = ROOT_RE.is_match(command) || words_.iter().flatten().any(|w| ROOT_WORDS.iter().any(|r| word_is(w, r, false)));
        Parsed { command: command.to_string(), words: words_, commands, root }
    }
}

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
        cmd.iter().enumerate().any(|(i, w)| {
            if !word_is(w, name, false) {
                return false;
            }
            let mut have: Vec<String> = Vec::new();
            for a in &cmd[i + 1..] {
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
/// its last `/` or `\` (without `.exe` and the like), or as a glob that
/// could expand to the rule's word.
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
    command.words.iter().any(|have| {
        have.windows(want.len())
            .any(|win| win.iter().zip(&want).enumerate().all(|(i, (w, r))| word_is(w, r, prefix && i + 1 == want.len())))
    }) || (!prefix && flags_match(&want, command))
}

/// The programs that give root.
const ROOT_WORDS: &[&str] = &["sudo", "sudoedit", "dzdo", "su", "doas", "pkexec", "run0"];

/// Whether a word of `command`, read as the always-ask list reads them,
/// names a program that gives root, or is a glob that could (`sud?`).
#[cfg(test)]
pub(crate) fn mentions_root(command: &str) -> bool {
    Parsed::new(command).words.iter().flatten().any(|w| ROOT_WORDS.iter().any(|r| word_is(w, r, false)))
}

/// A command that asks for root: one of the tools that give it (sudo,
/// sudoedit, dzdo, su, doas, pkexec, run0), read as the always-ask list
/// reads words, anywhere in it, or a glob that could name one.
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
pub(crate) fn decide(policy: &Policy, session_allow: bool, command: &str) -> Decision {
    decide_parsed(policy, session_allow, &Parsed::new(command))
}

/// The same, for a command read already. A policy that couldn't be read,
/// or is on hold (`force_ask`), asks, whatever the level and the session.
pub(crate) fn decide_parsed(policy: &Policy, session_allow: bool, command: &Parsed) -> Decision {
    if let Some(rule) = policy.ask.iter().find(|r| ask_matches_parsed(r, command)) {
        return Decision::Ask { rule: Some(rule.clone()) };
    }
    if policy.force_ask.is_some() {
        return Decision::Ask { rule: None };
    }
    if session_allow || policy.level == RUN_ANYTHING {
        return if session_allow { Decision::Session } else { Decision::Anything };
    }
    if policy.level == ALLOWED_COMMANDS && !command.root {
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

/// Make the tables, and the first time, the default always-ask list. Every
/// vault open runs it, on `servers` and `folders` as they are by then.
pub(crate) fn create_schema(conn: &Connection) -> Result<(), String> {
    let had: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ai_rules'", [], |_| Ok(()))
        .optional()
        .map_err(db("AI_TRUST_SCHEMA"))?
        .is_some();
    conn.execute_batch(SCHEMA).map_err(db("AI_TRUST_SCHEMA"))?;
    if !had {
        for p in DEFAULT_ASK {
            conn.execute("INSERT OR IGNORE INTO ai_rules (scope, scope_id, kind, pattern) VALUES ('default', 0, 'ask', ?1)", [p])
                .map_err(db("AI_TRUST_DEFAULTS"))?;
        }
    }
    Ok(())
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
/// even where you allowed everything (Policy::unreadable).
pub(crate) fn policy_for(conn: &Connection, server_id: Option<i64>) -> Policy {
    try_policy(conn, server_id).unwrap_or_else(|_| Policy::unreadable())
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
