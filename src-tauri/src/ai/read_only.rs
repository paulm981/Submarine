//! The "Run commands that only read" level (#190): whether a command only
//! reads, decided by Submarine before it runs, never by the AI.
//!
//! - **A list, not a guess.** A command runs at this level only when every
//!   program in it is on the list below, with only the words that keep it
//!   reading. Anything not on the list asks: a program that's missing costs
//!   a click, one that's wrongly on it could change a server.
//! - **One plain pipeline.** Plain words, single or double quotes, and `|`
//!   between programs. Anything else a shell would read as more (`;`, `&`,
//!   `>`, `<`, `$`, backticks, brackets, braces, `\`, `#`, `!`, `^`, a tab
//!   or a newline) makes it ask, and so does `NAME=value` first or a program
//!   named by its path (`./ls`). Inside single quotes anything printable but
//!   `\` (fish reads it there); inside double quotes nothing a shell expands.
//! - **Globs only where they can't turn into an option.** A word with an
//!   unquoted `*`, `?` or `[` must not start with one of them or with `-`:
//!   `/var/log/*.log` runs, `*.log` asks, as a file named `-delete` would
//!   turn into an option of the program.
//! - **GNU long options can be shortened** (`--vac` for `--vacuum-size`), so
//!   a word that could be the start of a long option that changes things
//!   counts as that option (`long_is`).
//! - **Left off on purpose,** as they can run a program or write while they
//!   look as if they read: `sed` (its `e` and `w`), `awk` (`system()`), `git`
//!   (a repository's config can run a program on `git status`), `less` and
//!   `man` (a shell), `jq` (`$ENV`), `env`, `xargs`, `watch`, `timeout`,
//!   `tee`. And everything that reaches out: `curl`, `wget`, `nc`, `ssh`,
//!   `ping`, `dig`, `nslookup` (a hostname can carry data out).
//! - **Devices ask** (`reaches_a_device`): opening one can do something,
//!   reading or not. `/dev/watchdog` reboots the server unless it's closed
//!   just so, and reading `/proc/kmsg` takes the kernel's messages away.
//! - **Files that usually hold secrets ask** (`names_secret`): a word naming
//!   `.env`, `.ssh/`, a private key and the like. A best effort, not a wall:
//!   a secret in a file with another name, or found by `grep -r`, is read.
//!
//! What runs as root never runs at this level (trust::decide_parsed), and
//! neither does anything on a Windows server: PowerShell and cmd read a
//! command otherwise.

/// The longest command looked at, in bytes: a longer one asks.
static MAX_LEN: usize = 2048;
/// The most programs in one pipeline.
static MAX_STAGES: usize = 8;

/// One word, as the shell will pass it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Word {
    /// With its quotes taken off.
    pub text: String,
    /// It has an unquoted `*`, `?` or `[`.
    pub glob: bool,
}

/// Whether `command` only reads, by the list in this file: false for
/// anything not on it.
pub(crate) fn only_reads(command: &str) -> bool {
    match pipeline(command) {
        Some(stages) => stages.iter().all(|s| stage_reads(s)) && !stages.iter().flatten().any(|w| names_secret(&w.text) || reaches_a_device(w)),
        None => false,
    }
}

/// The pipeline's programs and their words; None when it's more than one
/// plain pipeline, or isn't plain enough to read for sure.
pub(crate) fn pipeline(command: &str) -> Option<Vec<Vec<Word>>> {
    if command.len() > MAX_LEN || !command.chars().all(|c| c == ' ' || c.is_ascii_graphic()) {
        return None;
    }
    let mut stages: Vec<Vec<Word>> = Vec::new();
    let mut stage: Vec<Word> = Vec::new();
    let mut word: Option<Word> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' => stage.extend(word.take()),
            '|' => {
                stage.extend(word.take());
                if stage.is_empty() {
                    return None;
                }
                stages.push(std::mem::take(&mut stage));
            }
            '\'' => {
                let w = word.get_or_insert_with(empty);
                loop {
                    match chars.next()? {
                        '\'' => break,
                        '\\' => return None,
                        q => w.text.push(q),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_with(empty);
                loop {
                    match chars.next()? {
                        '"' => break,
                        '$' | '`' | '\\' | '!' => return None,
                        q => w.text.push(q),
                    }
                }
            }
            '*' | '?' | '[' | ']' => {
                let w = word.get_or_insert_with(empty);
                // `]` alone expands nothing.
                w.glob |= c != ']';
                w.text.push(c);
            }
            c if c.is_ascii_alphanumeric() || "-_./:=,+@%~".contains(c) => word.get_or_insert_with(empty).text.push(c),
            _ => return None,
        }
    }
    stage.extend(word.take());
    if stage.is_empty() {
        return None;
    }
    stages.push(stage);
    let globs_safe = stages.iter().flatten().all(|w| !w.glob || !w.text.starts_with(['*', '?', '[', '-']));
    (stages.len() <= MAX_STAGES && globs_safe).then_some(stages)
}

fn empty() -> Word {
    Word { text: String::new(), glob: false }
}

/// Whether one program and its words only read.
fn stage_reads(words: &[Word]) -> bool {
    let Some((program, args)) = words.split_first() else { return false };
    if program.glob {
        return false;
    }
    match program.text.as_str() {
        // Nothing they're given makes them write or run anything.
        "cat" | "tac" | "head" | "tail" | "wc" | "nl" | "cut" | "tr" | "paste" | "join" | "comm" | "fold" | "fmt" | "expand" | "column"
        | "zcat" | "ls" | "stat" | "du" | "df" | "free" | "uptime" | "w" | "who" | "whoami" | "id" | "groups" | "uname" | "nproc" | "arch"
        | "ps" | "pgrep" | "pidof" | "lsblk" | "lscpu" | "lsmem" | "lspci" | "lsusb" | "lsof" | "lsmod" | "modinfo" | "findmnt" | "netstat" | "vmstat"
        | "iostat" | "mpstat" | "getent" | "last" | "which" | "readlink" | "realpath" | "basename" | "dirname" | "pwd" | "md5sum" | "sha1sum"
        | "sha256sum" | "sha512sum" | "b2sum" | "cksum" | "strings" | "od" | "hexdump" | "diff" | "cmp" | "dpkg-query" | "getconf" | "locale" => true,
        // `-D read` reads the devices `-r` comes across.
        "grep" | "egrep" | "fgrep" => !short_has(args, "D") && !long_is(args, &["--devices"]),
        // `uniq IN OUT` writes OUT.
        "uniq" => positional(args).count() <= 1,
        "sort" => !short_has(args, "o") && !long_is(args, &["--output", "--compress-program"]),
        "ss" => !short_has(args, "KD") && !long_is(args, &["--kill", "--diag"]),
        "file" => !short_has(args, "C") && !long_is(args, &["--compile"]),
        "find" => !args.iter().any(|w| FIND_ACTIONS.contains(&w.text.as_str())),
        "dmesg" => !short_has(args, "cCnDE") && !long_is(args, &["--clear", "--read-clear", "--console-level", "--console-off", "--console-on"]),
        "date" => date_shows(args),
        "hostname" => args.iter().all(|w| HOSTNAME_SHOWS.contains(&w.text.as_str())),
        "hostnamectl" | "timedatectl" => args.iter().all(|w| matches!(w.text.as_str(), "status" | "show" | "--no-pager")),
        "ifconfig" => positional(args).count() <= 1 && args.iter().filter(|w| w.text.starts_with('-')).all(|w| matches!(w.text.as_str(), "-a" | "-s" | "-v")),
        "sysctl" => args.iter().all(|w| matches!(w.text.as_str(), "-a" | "-n" | "-e" | "-N" | "-A" | "-X") || (!w.text.starts_with('-') && !w.text.contains('='))),
        "crontab" => args.len() == 1 && args[0].text == "-l",
        "systemctl" => first_is(args, SYSTEMCTL_SHOWS) && !short_has(args, "HM") && !long_is(args, &["--host", "--machine"]),
        "journalctl" => !long_is(args, JOURNALCTL_CHANGES),
        "ip" => ip_shows(args),
        "docker" | "podman" => docker_shows(args),
        "dpkg" => {
            args.first().is_some_and(|w| DPKG_SHOWS.contains(&w.text.as_str())) && args[1..].iter().all(|w| !w.text.starts_with('-'))
        }
        "rpm" => rpm_queries(args),
        "apt" => first_is(args, &["list", "show", "policy", "search", "depends", "rdepends"]) && !short_has(args, "oc") && !long_is(args, &["--option", "--config-file"]),
        "apt-cache" => {
            first_is(args, &["policy", "show", "search", "depends", "rdepends", "madison", "showpkg", "pkgnames"])
                && !short_has(args, "oc")
                && !long_is(args, &["--option", "--config-file"])
        }
        _ => false,
    }
}

/// find's actions that delete, run a program or write a file.
static FIND_ACTIONS: &[&str] = &["-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf", "-fls"];
static HOSTNAME_SHOWS: &[&str] =
    &["-f", "-s", "-i", "-I", "-d", "-A", "-a", "--fqdn", "--long", "--short", "--ip-address", "--all-ip-addresses", "--domain", "--all-fqdns"];
static SYSTEMCTL_SHOWS: &[&str] = &[
    "status",
    "show",
    "cat",
    "list-units",
    "list-unit-files",
    "list-timers",
    "list-sockets",
    "list-dependencies",
    "list-jobs",
    "is-active",
    "is-enabled",
    "is-failed",
    "is-system-running",
    "get-default",
];
static JOURNALCTL_CHANGES: &[&str] = &[
    "--vacuum-size",
    "--vacuum-files",
    "--vacuum-time",
    "--rotate",
    "--flush",
    "--sync",
    "--relinquish-var",
    "--smart-relinquish-var",
    "--setup-keys",
    "--update-catalog",
    "--cursor-file",
];
static DPKG_SHOWS: &[&str] = &["-l", "-L", "-s", "-S", "--list", "--listfiles", "--status", "--search", "--get-selections", "--print-architecture"];

/// The words that aren't options.
fn positional(args: &[Word]) -> impl Iterator<Item = &Word> {
    args.iter().filter(|w| !w.text.starts_with('-'))
}

/// The first word is one of `subcommands`: no option before it, whose
/// value could be read as one (`docker -H ps rm x`).
fn first_is(args: &[Word], subcommands: &[&str]) -> bool {
    args.first().is_some_and(|w| subcommands.contains(&w.text.as_str()))
}

/// A short option (`-x`, or one of a cluster `-lx`) is one of `letters`.
/// A cluster with a value in it (`-t,`) counts every character in it, so
/// this may say yes where the program wouldn't: then it asks.
fn short_has(args: &[Word], letters: &str) -> bool {
    args.iter().any(|w| w.text.starts_with('-') && !w.text.starts_with("--") && w.text[1..].contains(|c| letters.contains(c)))
}

/// A long option is one of `options`, or could be one shortened, as GNU's
/// getopt takes any start of a long option that's unique (`--vac`).
fn long_is(args: &[Word], options: &[&str]) -> bool {
    args.iter().any(|w| {
        let name = w.text.split('=').next().unwrap_or("");
        name.len() > 2 && name.starts_with("--") && options.iter().any(|o| o.starts_with(name))
    })
}

/// `date` showing a date: `-d`, `--date` and `-r` (a file's time) take the
/// next word; any other word that isn't an option is a format (`+%F`).
/// `-s`, `--set` and a bare date (`date 0101…`, which sets it) ask.
fn date_shows(args: &[Word]) -> bool {
    if short_has(args, "s") || long_is(args, &["--set"]) {
        return false;
    }
    let mut words = args.iter();
    while let Some(w) = words.next() {
        match w.text.as_str() {
            "-d" | "--date" | "-r" | "--reference" => {
                if words.next().is_none() {
                    return false;
                }
            }
            t if t.starts_with('-') || t.starts_with('+') => {}
            _ => return false,
        }
    }
    true
}

/// `ip` showing something: options with no value, then an object, then
/// nothing or a word that shows (`ip -br a`, `ip route get 1.1.1.1`).
/// Options with a value (`-b FILE` runs the commands in it, `-n NS`) and
/// objects that do more (`netns exec`) ask.
fn ip_shows(args: &[Word]) -> bool {
    static NO_VALUE: &[&str] = &[
        "-4", "-6", "-0", "-s", "-stats", "-statistics", "-d", "-details", "-o", "-oneline", "-r", "-resolve", "-c", "-color", "-br", "-brief", "-j", "-json", "-p",
        "-pretty", "-h", "-human",
    ];
    static OBJECTS: &[&str] = &["address", "addr", "a", "link", "l", "route", "r", "neigh", "neighbor", "neighbour", "n", "rule", "maddress", "maddr", "netconf"];
    static SHOWS: &[&str] = &["show", "list", "ls", "lst", "sh", "get"];
    let start = args.iter().position(|w| !NO_VALUE.contains(&w.text.as_str())).unwrap_or(args.len());
    let rest = &args[start..];
    match rest {
        [] => false,
        [object, more @ ..] => {
            OBJECTS.contains(&object.text.as_str())
                && match more.first() {
                    None => true,
                    Some(verb) => SHOWS.contains(&verb.text.as_str()),
                }
        }
    }
}

/// `docker` or `podman` showing something: the subcommand first, then any
/// of its words (a format is a template, which can't run anything).
fn docker_shows(args: &[Word]) -> bool {
    static SHOWS: &[&str] = &["ps", "logs", "inspect", "images", "top", "port", "version", "info", "stats", "history", "diff"];
    let second = |list: &[&str]| args.get(1).is_some_and(|w| list.contains(&w.text.as_str()));
    match args.first().map(|w| w.text.as_str()) {
        Some(s) if SHOWS.contains(&s) => true,
        Some("container") => second(&["ls", "list", "ps", "logs", "inspect", "top", "port", "stats", "diff"]),
        Some("image") => second(&["ls", "list", "inspect", "history"]),
        Some("network" | "volume") => second(&["ls", "list", "inspect"]),
        Some("system") => second(&["df"]),
        _ => false,
    }
}

/// `rpm -q…` with query letters only (`-qa`, `-qi`, `-qf`, `-qlp`), or
/// `--query`, and nothing else that starts with `-`.
fn rpm_queries(args: &[Word]) -> bool {
    let Some((first, rest)) = args.split_first() else { return false };
    let query = first.text == "--query" || first.text.strip_prefix("-q").is_some_and(|l| l.chars().all(|c| "ailfcdRsp".contains(c)));
    query && rest.iter().all(|w| !w.text.starts_with('-'))
}

/// Whether a word could name a device: a path into `/dev` or to
/// `/proc/kmsg` (each part split at `=`, as `--file=/dev/x`), however it's
/// spelled (`//dev`, `/./dev`), also from the home folder where that's `/`
/// (`dev/x`, but not a bare `dev`, as in `ip addr show dev eth0`); a path with `..` in it; or a glob in its first folder, which
/// could be `/dev` (`/d*/watchdog`).
fn reaches_a_device(w: &Word) -> bool {
    w.text.split('=').any(|path| {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
        let globbed = w.glob && parts.first().is_some_and(|p| p.contains(['*', '?', '['])) && (path.starts_with('/') || parts.len() > 1);
        parts.contains(&"..")
            || (matches!(parts.first(), Some(&"dev")) && (path.starts_with('/') || parts.len() > 1))
            || (matches!(parts.first(), Some(&"proc")) && parts.get(1).is_some_and(|p| p.starts_with("kmsg")))
            || globbed
    })
}

/// Whether a word names a file or folder that usually holds secrets: one
/// of its parts, split at `/` and `=` (`--file=~/.env`), in any case.
pub(crate) fn names_secret(word: &str) -> bool {
    static NAMES: &[&str] = &[
        ".ssh",
        ".aws",
        ".gnupg",
        ".kube",
        ".docker",
        ".azure",
        "gcloud",
        ".password-store",
        ".env",
        ".netrc",
        ".pgpass",
        ".git-credentials",
        ".my.cnf",
        ".htpasswd",
        ".npmrc",
        ".pypirc",
        ".vault-token",
        "shadow",
        "gshadow",
        "sudoers",
        "credentials",
        "secrets",
    ];
    static ENDS: &[&str] = &[".pem", ".key", "_key", ".p12", ".pfx", ".kdbx", ".jks", ".keystore"];
    let word = word.to_ascii_lowercase();
    word.split(['/', '=']).any(|part| {
        NAMES.contains(&part) || part.starts_with(".env.") || part.starts_with("id_") || ENDS.iter().any(|e| part.ends_with(e)) || part.starts_with("shadow")
    })
}

#[cfg(test)]
mod tests;
