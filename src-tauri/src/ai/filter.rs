//! What an AI tab is given of a command's output: the output with the secrets
//! it's known to carry replaced by `[hidden by Submarine]`. You see the whole
//! output in the AI panel, and how many things were hidden.
//!
//! The rules are fixed (#32, "Secrets reaching the AI service"): private key
//! blocks (also in base64), the value after `password=`, `secret:`,
//! `token=`, `DB_PASS=` and their kin, passwords on command lines
//! (`--password X`, `mysql -pX`, `sshpass -p X`), an Authorization header's
//! credentials, JSON Web Tokens, password hashes, cloud and service access
//! keys, and the password in a URL; and in config files that write it after
//! a space: Cisco's `password 7 …` and `secret 5 …`, Redis's `requirepass`,
//! `.netrc`, Juniper's `$9$…` and `encrypted-password`, Docker's `"auth"`,
//! `.pgpass` lines, Wi-Fi `psk=` and Debian's `wpa-psk`. They catch the common shapes, not
//! everything, and the app says so where it shows them.

use regex::Regex;
use std::sync::LazyLock;

pub(crate) const HIDDEN: &str = "[hidden by Submarine]";

/// The output as the AI gets it, and how many things were hidden.
#[derive(Debug, PartialEq)]
pub(crate) struct Filtered {
    pub text: String,
    pub hidden: usize,
}

struct Rule {
    re: Regex,
    /// The capture group that's the secret; 0 = the whole match.
    group: usize,
    /// For a match that isn't one after all: true leaves it as it is.
    not_secret: Option<fn(&regex::Captures) -> bool>,
}

fn rule(pattern: &str, group: usize) -> Rule {
    Rule { re: Regex::new(pattern).expect("filter rule"), group, not_secret: None }
}

fn rule_unless(pattern: &str, group: usize, not_secret: fn(&regex::Captures) -> bool) -> Rule {
    Rule { not_secret: Some(not_secret), ..rule(pattern, group) }
}

/// A value after a short key (`pass`, `pwd`, `credentials`) that's no
/// secret: the working folder `env` and `set` print (`PWD=/home/you`), git's
/// `credential.helper`, and a bare `pass:`/`pwd:` (a test run's `pass: 12`),
/// which only counts as a key quoted (`"pass": …`) or with a prefix
/// (`db_pass: …`).
fn short_key_not_secret(c: &regex::Captures) -> bool {
    let key = c.name("key").map_or("", |m| m.as_str()).to_ascii_lowercase();
    let value = c.name("value").map_or("", |m| m.as_str());
    let quoted = c.name("quote").is_some_and(|m| !m.as_str().is_empty());
    let colon = c.name("sep").is_some_and(|m| m.as_str() == ":");
    let bare = matches!(key.as_str(), "pass" | "pwd");
    (matches!(key.as_str(), "pwd" | "oldpwd") && value.starts_with(['/', '~']))
        || key.starts_with("credential.")
        || (bare && colon && !quoted)
}

/// The longest base64 blob that's decoded to look inside; a longer one is
/// hidden unread.
const MAX_PEM_BLOB: usize = 64 * 1024;

/// A base64 blob that starts like a PEM block but holds no private key (a
/// certificate, a public key, a chain of them). All of it is looked at: a
/// bundle (HAProxy's .pem) may start with its certificate and carry the key
/// after it. One too long to look at, or that doesn't decode, is hidden.
fn base64_pem_not_secret(c: &regex::Captures) -> bool {
    use base64::Engine;
    let blob: String = c[0].chars().filter(|ch| !ch.is_whitespace()).collect();
    if blob.len() > MAX_PEM_BLOB {
        return false;
    }
    let data = blob.trim_end_matches('=');
    // Whole groups only: output cut short may end in a partial one.
    let usable = data.len() / 4 * 4;
    match base64::engine::general_purpose::STANDARD_NO_PAD.decode(&data.as_bytes()[..usable]) {
        Ok(decoded) => !String::from_utf8_lossy(&decoded).contains("PRIVATE KEY"),
        Err(_) => false,
    }
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // A private key, from its BEGIN line to its END line (or to the end of
        // the output, when it was cut): PEM, OpenSSH, PGP.
        rule(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?s:.*?)(?:-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\z)", 0),
        // The same in base64 (`LS0tLS1CRUdJTi…` is "-----BEGIN"), as
        // Kubernetes secrets and CI variables hold them, on one line or
        // wrapped.
        rule_unless(r"(?m)LS0tLS1CRUdJTi[A-Za-z0-9+/]*={0,2}(?:\r?\n[A-Za-z0-9+/]{16,}={0,2}\r?$)*", 0, base64_pem_not_secret),
        // A key file's contents as a YAML value: `tls.key: …`, `client.key: …`.
        rule(r"(?m)^[ \t]*[A-Za-z0-9_.-]*\.key[ \t]*:[ \t]*([A-Za-z0-9+/]{20,}={0,2})[ \t]*\r?$", 1),
        // A PuTTY key's private part.
        rule(r"(?m)^Private-Lines: *\d+\r?\n((?:[A-Za-z0-9+/=]+(?:\r?\n|\z))+)", 1),
        // Juniper: `encrypted-password "…"` (before the hashes, so the quotes
        // go with it), and its reversible `$9$…`, anywhere; Cisco's type 9
        // (scrypt) `$9$salt$hash` too, all of it.
        rule(r#"\bencrypted-password[ \t]+("[^"\r\n]*"|[^\s;]+)"#, 1),
        rule(r"\$9\$[A-Za-z0-9./$-]{8,}", 0),
        // Password hashes as crypt(3) writes them, in /etc/shadow and
        // elsewhere: MD5 (and Apache's), bcrypt, SHA-256/512, yescrypt,
        // scrypt, argon2; and Cisco's type 8 (PBKDF2).
        rule(r"\$(?:1|2[abxy]?|5|6|7|8|y|gy|md5|apr1|sha1|argon2(?:id|i|d))\$[^\s:]{8,}", 0),
        // `password=…`, `secret: …`, `"api_key": "…"`: the value.
        rule(
            r#"(?i)[A-Za-z0-9_.-]*(?:password|passwd|passphrase|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|account[_-]?key)(?:[_.-][A-Za-z0-9_.-]*)?["']?[ \t]*[:=][ \t]*("[^"\r\n]+"|'[^'\r\n]+'|[^\s"',;]+)"#,
            1,
        ),
        // The same for the short keys, as a word of their own or a part of
        // one: `pass=…`, `DB_PASS=…`, `MYSQL_PWD=…`, `"pwd": …`,
        // `credentials: …`. Not `bypass` or `compass`.
        rule_unless(
            r#"(?i)(?:^|[^A-Za-z0-9_.-])(?P<quote>["']?)(?P<key>(?:[A-Za-z0-9]+[_.-])*(?:pass|pwd|credentials?)(?:[_.-][A-Za-z0-9]+)*)["']?[ \t]*(?P<sep>[:=])[ \t]*(?P<value>"[^"\r\n]+"|'[^'\r\n]+'|[^\s"',;]+)"#,
            4,
            short_key_not_secret,
        ),
        // A password on a command line: `--password X`, `--pass=X`, and
        // their kin.
        rule(
            r#"(?i)(?:^|[\s'"])--(?:password|passwd|pass|pwd|passphrase|secret|token|api-key)(?:=|[ \t]+)("[^"\r\n]*"|'[^'\r\n]*'|[^\s"']+)"#,
            1,
        ),
        // MySQL's and MariaDB's `-pPASSWORD` (no space), and `sshpass -p X`.
        rule(
            r#"\b(?:mysql|mysqldump|mysqladmin|mysqlimport|mysqlcheck|mysqlshow|mariadb|mariadb-dump|mariadb-admin)\b[^\r\n]*?[ \t]-p("[^"\r\n]*"|'[^'\r\n]*'|[^\s"'-][^\s"']*)"#,
            1,
        ),
        rule(r#"\bsshpass\b[^\r\n]*?[ \t]-p[ \t]*("[^"\r\n]*"|'[^'\r\n]*'|[^\s"']+)"#, 1),
        // A JSON Web Token on its own: header and payload, both `{"…`.
        rule(r"\beyJ[A-Za-z0-9_-]{5,}\.eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*", 0),
        // An HTTP Authorization header's credentials.
        rule(r"(?i)\b(?:proxy-)?authorization[ \t]*:[ \t]*(?:(?:bearer|basic|token|digest)[ \t]+)?([^\s]+)", 1),
        // Access keys with a known shape: AWS key ids, GitHub, GitLab, Slack,
        // Google, Stripe, Anthropic and OpenAI keys.
        rule(r"\b(?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b", 0),
        rule(r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})\b", 0),
        rule(r"\bglpat-[A-Za-z0-9_-]{20,}", 0),
        rule(r"\bxox[abprs]-[A-Za-z0-9-]{10,}", 0),
        rule(r"\bAIza[0-9A-Za-z_-]{35}", 0),
        rule(r"\b[sr]k_live_[0-9A-Za-z]{24,}", 0),
        rule(r"\bsk-(?:ant-)?[A-Za-z0-9_-]{20,}", 0),
        // The password in `scheme://user:password@host`.
        rule(r"\b[A-Za-z][A-Za-z0-9+.-]*://[^\s/:@]+:([^\s/@]+)@", 1),
        // Config lines that give the value after a space, not `=` or `:`:
        // Cisco's `enable secret 5 …` and `password 7 …` (the digit is the
        // kind, the value follows; `enable` may name a `level N` first), Redis's `requirepass` and `masterauth`,
        // a `.netrc` password on a line of its own. Only a line that starts
        // with the word, in lower case, and ends with the value: not prose
        // ("password reset link sent"), nor PAM's `password required pam_unix.so`.
        rule(
            r#"(?m)^[ \t]*(?:enable[ \t]+(?:password|secret)(?:[ \t]+level[ \t]+[0-9]+)?|password|secret|requirepass|masterauth)[ \t]+(?:[0-9][ \t]+)?("[^"\r\n]*"|'[^'\r\n]*'|\S+)[ \t]*\r?$"#,
            1,
        ),
        // Cisco's `username NAME [privilege N] [algorithm-type T] password|secret [N] VALUE`.
        rule(
            r#"(?m)^[ \t]*username[ \t]+\S+(?:[ \t]+privilege[ \t]+[0-9]+)?(?:[ \t]+algorithm-type[ \t]+\S+)?[ \t]+(?:password|secret)[ \t]+(?:[0-9][ \t]+)?("[^"\r\n]*"|\S+)[ \t]*\r?$"#,
            1,
        ),
        // `.netrc`: `machine HOST [login U] [account A] [port P] password PW`,
        // or `default login U password PW`, the line made of these only.
        rule(
            r"(?m)^[ \t]*(?:machine[ \t]+\S+(?:[ \t]+(?:login|account|port)[ \t]+\S+)*|default(?:[ \t]+(?:login|account|port)[ \t]+\S+)+)[ \t]+password[ \t]+(\S+)(?:[ \t]+(?:login|account|port)[ \t]+\S+)*[ \t]*\r?$",
            1,
        ),
        // Docker's config.json: `"auth": "<base64 of user:password>"`.
        rule(r#""auth"[ \t]*:[ \t]*("[^"\r\n]+")"#, 1),
        // A `.pgpass` line, `host:port:database:user:password`: the last
        // field, which is the rest of the line, spaces and all. Five fields
        // exactly (a colon in the password is escaped), the port a number
        // or `*`, so /etc/passwd and /etc/shadow lines aren't taken for one.
        rule(
            r"(?m)^(?:[^\s:\\]|\\.)+:(?:[0-9]+|\*):(?:[^\s:\\]|\\.)+:(?:[^\s:\\]|\\.)+:((?:[^\s:\\]|\\.)(?:[^\r\n:\\]|\\.)*?)[ \t]*\r?$",
            1,
        ),
        // Wi-Fi: wpa_supplicant's and NetworkManager's `psk=…`, hostapd's
        // `wpa_psk=…`.
        rule(r#"(?im)(?:^|[\s{;,-])(?:wpa_)?psk[ \t]*=[ \t]*("[^"\r\n]*"|[^\s"]+)"#, 1),
        // Debian's /etc/network/interfaces: `wpa-psk VALUE` (and
        // `wpa-password` for 802.1X), the value the rest of the line.
        rule(r"(?m)^[ \t]*wpa-(?:psk|password)[ \t]+(\S(?:[^\r\n]*\S)?)[ \t]*\r?$", 1),
    ]
});

/// `text` with every secret the rules find replaced, rule by rule.
///
/// `text` is the output decoded first (non-UTF-8 bytes become U+FFFD, which
/// the value rules take as part of a value), then filtered. NUL characters
/// are dropped first, so text written as UTF-16 (`p\0a\0s\0s…`) is read as
/// its letters.
pub(crate) fn filter(text: &str) -> Filtered {
    let mut out = text.replace('\0', "");
    let mut hidden = 0;
    for r in RULES.iter() {
        // What earlier rules hid: a match touching one is left as it is.
        // Both come in order and don't overlap among themselves, so one walk
        // along `done` checks every match (linear, not matches × done).
        let done: Vec<(usize, usize)> = out.match_indices(HIDDEN).map(|(i, s)| (i, i + s.len())).collect();
        let mut d = 0;
        let mut next = String::with_capacity(out.len());
        let mut last = 0;
        for caps in r.re.captures_iter(&out) {
            let Some(m) = caps.get(r.group) else { continue };
            while d < done.len() && done[d].1 <= m.start() {
                d += 1;
            }
            if d < done.len() && done[d].0 < m.end() {
                continue;
            }
            if r.not_secret.is_some_and(|f| f(&caps)) {
                continue;
            }
            next.push_str(&out[last..m.start()]);
            next.push_str(HIDDEN);
            last = m.end();
            hidden += 1;
        }
        if last > 0 {
            next.push_str(&out[last..]);
            out = next;
        }
    }
    Filtered { text: out, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hides(input: &str, expect: &str, n: usize) {
        let f = filter(input);
        assert_eq!(f.text, expect, "for {input:?}");
        assert_eq!(f.hidden, n, "for {input:?}");
    }

    #[test]
    fn private_key_blocks() {
        hides(
            "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\nAAAA\n-----END OPENSSH PRIVATE KEY-----\nafter",
            &format!("before\n{HIDDEN}\nafter"),
            1,
        );
        hides("-----BEGIN RSA PRIVATE KEY-----\r\nMIIE\r\n-----END RSA PRIVATE KEY-----", HIDDEN, 1);
        hides("-----BEGIN PGP PRIVATE KEY BLOCK-----\nlQ\n-----END PGP PRIVATE KEY BLOCK-----", HIDDEN, 1);
        // Cut off before its end: hidden to the end.
        hides("x\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg", &format!("x\n{HIDDEN}"), 1);
        // A public key or a certificate isn't a secret.
        let public = "-----BEGIN PUBLIC KEY-----\nMIIB\n-----END PUBLIC KEY-----";
        hides(public, public, 0);
    }

    #[test]
    fn putty_private_lines() {
        hides(
            "PuTTY-User-Key-File-3: ssh-ed25519\nPrivate-Lines: 1\nAAAAIFR4cGF5bG9hZA==\nPrivate-MAC: ab12",
            &format!("PuTTY-User-Key-File-3: ssh-ed25519\nPrivate-Lines: 1\n{HIDDEN}Private-MAC: ab12"),
            1,
        );
    }

    #[test]
    fn password_hashes() {
        hides(
            "root:$6$rounds=5000$saltsalt$Zx9aQ1bC2dE3fG4hI5jK6lM7nO8pQ9rS0tU1vW2xY3zA4bC5dE6fG7hI8jK9lM0nO1pQ2rS3tU4vW5xY6z:19000:0:99999:7:::",
            &format!("root:{HIDDEN}:19000:0:99999:7:::"),
            1,
        );
        hides("paul:$y$j9T$abcdEFGH1234$abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG:19500::::::", &format!("paul:{HIDDEN}:19500::::::"), 1);
        hides("$2b$12$KIXQ1Yx6u8Vb3n2Q4u5e1eQwZ3yX0aB9cD8eF7gH6iJ5kL4mN3oP2", HIDDEN, 1);
        hides("$argon2id$v=19$m=65536,t=3,p=4$c2FsdHNhbHQ$aGFzaGhhc2hoYXNo", HIDDEN, 1);
        // A locked account has no hash; a price isn't one.
        hides("daemon:*:19000:0:99999:7:::\nbin:!:19000::::::", "daemon:*:19000:0:99999:7:::\nbin:!:19000::::::", 0);
        hides("it costs $6 today", "it costs $6 today", 0);
    }

    #[test]
    fn assignments_hide_the_value() {
        hides("DB_PASSWORD=hunter2 next", &format!("DB_PASSWORD={HIDDEN} next"), 1);
        hides("password: \"correct horse\"", &format!("password: {HIDDEN}"), 1);
        hides(r#"{"api_key": "abc123", "user": "paul"}"#, &format!(r#"{{"api_key": {HIDDEN}, "user": "paul"}}"#), 1);
        hides("client_secret='s3cr3t'", &format!("client_secret={HIDDEN}"), 1);
        hides("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG", &format!("aws_secret_access_key = {HIDDEN}"), 1);
        hides("GITHUB_TOKEN=abc", &format!("GITHUB_TOKEN={HIDDEN}"), 1);
        // A key word without a value, or prose, is left alone.
        hides("Enter your password:", "Enter your password:", 0);
        hides("tokens: 5 used", "tokens: 5 used", 0);
    }

    #[test]
    fn authorization_headers() {
        hides("Authorization: Bearer eyJhbGciOi.abc", &format!("Authorization: Bearer {HIDDEN}"), 1);
        hides("proxy-authorization: Basic dXNlcjpwYXNz", &format!("proxy-authorization: Basic {HIDDEN}"), 1);
    }

    // Split with concat! so the samples aren't taken for real keys by
    // secret scanners.
    #[test]
    fn access_keys_with_known_shapes() {
        hides(concat!("key AKIA", "IOSFODNN7EXAMPLE here"), &format!("key {HIDDEN} here"), 1);
        hides(concat!("ghp", "_abcdefghijklmnopqrstuvwxyz0123456789"), HIDDEN, 1);
        hides(concat!("github", "_pat_11ABCDEFG0123456789_abcdefghij"), HIDDEN, 1);
        hides(concat!("glpat", "-abcdefghij0123456789"), HIDDEN, 1);
        hides(concat!("xoxb", "-1234567890-abcdef"), HIDDEN, 1);
        hides(concat!("AIza", "SyA1234567890abcdefghijklmnopqrstuv"), HIDDEN, 1);
        hides(concat!("sk", "_live_", "0123456789abcdefghijklmn"), HIDDEN, 1);
        hides(concat!("sk-ant", "-api03-abcdefghijklmnopqrstuvwxyz"), HIDDEN, 1);
        // Too short to be one.
        hides("AKIA123", "AKIA123", 0);
    }

    #[test]
    fn passwords_in_urls() {
        hides("postgres://app:s3cret@db.internal:5432/x", &format!("postgres://app:{HIDDEN}@db.internal:5432/x"), 1);
        hides("https://example.com/path", "https://example.com/path", 0);
        hides("ssh://git@example.com/repo", "ssh://git@example.com/repo", 0);
    }

    #[test]
    fn passwords_on_command_lines() {
        hides("mysqldump --password hunter2 db", &format!("mysqldump --password {HIDDEN} db"), 1);
        hides("app --pass=hunter2 --verbose", &format!("app --pass={HIDDEN} --verbose"), 1);
        hides("x --password 'two words' y", &format!("x --password {HIDDEN} y"), 1);
        hides("mysql -u root -phunter2 -h db", &format!("mysql -u root -p{HIDDEN} -h db"), 1);
        hides("  1234 mariadb -h db -p's3cr3t'", &format!("  1234 mariadb -h db -p{HIDDEN}"), 1);
        hides("sshpass -p hunter2 ssh host", &format!("sshpass -p {HIDDEN} ssh host"), 1);
        hides("sshpass -phunter2 ssh host", &format!("sshpass -p{HIDDEN} ssh host"), 1);
        // Not passwords: a prompt, a file, other programs' -p.
        for plain in ["mysql -u root -p", "mysql -u root --password", "mkdir -p /tmp/x", "ssh -p2222 host", "ps -p1234", "tar -pxf a.tar", "mysql -P3306"] {
            hides(plain, plain, 0);
        }
    }

    #[test]
    fn short_keys() {
        hides("DB_PASS=hunter2", &format!("DB_PASS={HIDDEN}"), 1);
        hides("export MYSQL_PWD=hunter2", &format!("export MYSQL_PWD={HIDDEN}"), 1);
        hides("pass=hunter2", &format!("pass={HIDDEN}"), 1);
        hides("pwd=hunter2", &format!("pwd={HIDDEN}"), 1);
        hides(r#"{"pwd": "hunter2"}"#, &format!(r#"{{"pwd": {HIDDEN}}}"#), 1);
        hides("db_pass: hunter2", &format!("db_pass: {HIDDEN}"), 1);
        hides("credentials: abc123", &format!("credentials: {HIDDEN}"), 1);
        hides("aws.credential = abc123", &format!("aws.credential = {HIDDEN}"), 1);
        // Not secrets: the working folder, the pwd command, a test run's
        // tally, words that end in "pass", git's credential helper.
        for plain in [
            "PWD=/home/paul\nOLDPWD=/tmp",
            "$ pwd\n/home/paul",
            "pass: 12 fail: 0",
            "bypass=1 compass=north",
            "credential.helper=store",
            "passed: 3",
        ] {
            hides(plain, plain, 0);
        }
    }

    #[test]
    fn cisco_and_redis_config_lines() {
        hides("enable secret 5 $1$mERr$hx5rVt7rPNoS4wqbXKX7m0", &format!("enable secret 5 {HIDDEN}"), 1);
        hides("enable password cisco123", &format!("enable password {HIDDEN}"), 1);
        hides("line vty 0 4\n password 7 0822455D0A16\n login", &format!("line vty 0 4\n password 7 {HIDDEN}\n login"), 1);
        hides(" password 0 plaintext\r\n", &format!(" password 0 {HIDDEN}\r\n"), 1);
        hides("username admin password 7 0822455D0A16", &format!("username admin password 7 {HIDDEN}"), 1);
        hides("username paul privilege 15 secret 8 $8$dsYGNam3K1SIJO$7nv/35M/qr6t.dVc7UY9zrJDWRVqncHub1PE9UlMQFs", &format!("username paul privilege 15 secret 8 {HIDDEN}"), 1);
        hides(
            "username paul privilege 15 algorithm-type scrypt secret 9 $9$nhEmQVczB7dqsO$X.HsgL6x1il0RxkOSSvyQYwucySCt7qFm4v7pqCxkKM",
            &format!("username paul privilege 15 algorithm-type scrypt secret 9 {HIDDEN}"),
            1,
        );
        hides("requirepass foobared\nmasterauth \"two words\"", &format!("requirepass {HIDDEN}\nmasterauth {HIDDEN}"), 2);
        hides("enable password level 15 cisco", &format!("enable password level 15 {HIDDEN}"), 1);
        hides("enable secret level 15 5 $1$mERr$hx5rVt7rPNoS4wqbXKX7m0", &format!("enable secret level 15 5 {HIDDEN}"), 1);
    }

    #[test]
    fn netrc_juniper_docker_pgpass_and_wifi() {
        hides("machine example.com login paul password hunter2", &format!("machine example.com login paul password {HIDDEN}"), 1);
        hides("machine h login u password pw account a", &format!("machine h login u password {HIDDEN} account a"), 1);
        hides("default login anonymous password me@x", &format!("default login anonymous password {HIDDEN}"), 1);
        hides("machine h\n  login u\n  password hunter2\n", &format!("machine h\n  login u\n  password {HIDDEN}\n"), 1);
        hides(r#"encrypted-password "$6$abcdefgh$0123456789abcdefghijklmnop";"#, &format!("encrypted-password {HIDDEN};"), 1);
        hides(r#"pre-shared-key ascii-text "$9$Hk5FCtOIEhrlMWx7-ws2gUjH"; ## SECRET-DATA"#, &format!(r#"pre-shared-key ascii-text "{HIDDEN}"; ## SECRET-DATA"#), 1);
        hides(r#"{"auths": {"registry.example.com": {"auth": "cGF1bDpodW50ZXIy"}}}"#, &format!(r#"{{"auths": {{"registry.example.com": {{"auth": {HIDDEN}}}}}}}"#), 1);
        hides("db.internal:5432:app:paul:hunter2\n*:*:*:paul:pa\\:ss", &format!("db.internal:5432:app:paul:{HIDDEN}\n*:*:*:paul:{HIDDEN}"), 2);
        hides("network={\n\tssid=\"home\"\n\tpsk=\"correct horse\"\n}", &format!("network={{\n\tssid=\"home\"\n\tpsk={HIDDEN}\n}}"), 1);
        hides("wpa_psk=0123456789abcdef", &format!("wpa_psk={HIDDEN}"), 1);
        hides("db:5432:app:paul:correct horse battery\r\n", &format!("db:5432:app:paul:{HIDDEN}\r\n"), 1);
        hides("*:*:*:paul:two words \\: one colon  \n", &format!("*:*:*:paul:{HIDDEN}  \n"), 1);
        hides(
            "iface wlan0 inet dhcp\n    wpa-ssid home\n    wpa-psk correct horse battery\n    wpa-password hunter2\n",
            &format!("iface wlan0 inet dhcp\n    wpa-ssid home\n    wpa-psk {HIDDEN}\n    wpa-password {HIDDEN}\n"),
            2,
        );
    }

    /// The space-separated rules leave prose and look-alikes alone.
    #[test]
    fn space_separated_rules_leave_prose_alone() {
        for plain in [
            "Your password reset link was sent.",
            "Password reset required",
            "password reset link sent",
            "the default password is admin",
            "username and password are required",
            "secret sauce recipe inside",
            "password    required    pam_unix.so sha512 shadow",
            "PasswordAuthentication no",
            "root:x:0:0:root:/root:/bin/bash",
            "daemon:*:19000:0:99999:7:::",
            "12:30:45 start",
            "# hostname:port:database:username:password",
            "machine learning password manager review",
            "\"auth\": {\"type\": \"oauth\"}",
            "upsk=1 pskreport",
            "enable password level 15",
            "wpa-psk",
            "daemon:*:19000:0:99999:7:::",
            "nobody:*:0:0:Unprivileged User:/var/empty:/usr/bin/false",
            "00:11:22:33:44:55 permanent",
        ] {
            hides(plain, plain, 0);
        }
    }

    #[test]
    fn apache_md5_hashes() {
        hides("paul:$apr1$r31.....$HqJZimcKQFAMYayBlzkrA/", &format!("paul:{HIDDEN}"), 1);
    }

    #[test]
    fn bare_json_web_tokens() {
        let jwt = concat!("eyJhbGciOiJIUzI1NiJ9", ".", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", ".", "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U");
        hides(&format!("token is {jwt} ok"), &format!("token is {HIDDEN} ok"), 1);
        hides(&format!("cookie {jwt}"), &format!("cookie {HIDDEN}"), 1);
        // One part alone isn't one.
        hides("eyJhbGciOiJIUzI1NiJ9", "eyJhbGciOiJIUzI1NiJ9", 0);
    }

    #[test]
    fn private_keys_in_base64() {
        use base64::Engine;
        let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
        let key = b64("-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\n-----END RSA PRIVATE KEY-----\n");
        assert!(key.starts_with("LS0tLS1CRUdJTi"));
        hides(&format!("data:\n  ssh-key: {key}\nkind: Secret"), &format!("data:\n  ssh-key: {HIDDEN}\nkind: Secret"), 1);
        // Wrapped, as `base64` writes it.
        let wrapped: Vec<String> = key.as_bytes().chunks(40).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        hides(&format!("{}\ndone", wrapped.join("\n")), &format!("{HIDDEN}\ndone"), 1);
        // A key file as a YAML value.
        hides(&format!("  tls.key: {key}"), &format!("  tls.key: {HIDDEN}"), 1);
        hides("  tls.key: MIIEvQIBADANBgkqhkiG9w0BAQEFAASC", &format!("  tls.key: {HIDDEN}"), 1);
        // A certificate isn't a secret, nor a path.
        let cert = format!("  tls.crt: {}", b64("-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n"));
        hides(&cert, &cert, 0);
        hides("  client.key: /etc/ssl/private/client.key", "  client.key: /etc/ssl/private/client.key", 0);
    }

    /// A bundle that starts with its certificate and has the key after it,
    /// on one line or wrapped: hidden whole. Too long to look at: hidden.
    #[test]
    fn a_certificate_and_key_bundle_in_base64() {
        use base64::Engine;
        let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
        let cert = format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n", "MIIB".repeat(200));
        let bundle = b64(&format!("{cert}-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg\n-----END PRIVATE KEY-----\n"));
        hides(&format!("  haproxy.pem: {bundle}"), &format!("  haproxy.pem: {HIDDEN}"), 1);
        let wrapped: Vec<String> = bundle.as_bytes().chunks(76).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        let f = filter(&format!("{}\nend", wrapped.join("\n")));
        assert_eq!(f.hidden, 1);
        assert!(!f.text.contains("LS0t") && f.text.ends_with("\nend"), "{}", f.text);
        // A chain of certificates only is left; one past the cap is hidden.
        let chain = format!("  ca.crt: {}", b64(&cert.repeat(3)));
        hides(&chain, &chain, 0);
        let huge = b64(&cert.repeat(80));
        assert!(huge.len() > MAX_PEM_BLOB);
        hides(&huge, HIDDEN, 1);
    }

    /// Many matches of later rules beside many hidden by earlier ones: the
    /// time grows with the output, not with its square. (Checked as a ratio,
    /// so a slow machine or a debug build doesn't make it fail.)
    #[test]
    fn many_secrets_take_linear_time() {
        let line = "pass=a password=b\n";
        let time = |kb: usize| {
            let text = line.repeat(kb * 1024 / line.len());
            (0..2)
                .map(|_| {
                    let t = std::time::Instant::now();
                    let f = filter(&text);
                    assert_eq!(f.hidden, 2 * (kb * 1024 / line.len()));
                    t.elapsed()
                })
                .min()
                .unwrap()
        };
        let (small, large) = (time(128), time(512));
        // Four times the output: about four times the time if linear,
        // sixteen if quadratic.
        let ratio = large.as_secs_f64() / small.as_secs_f64();
        assert!(ratio < 6.0, "128 KB: {small:?}, 512 KB: {large:?}");
    }

    #[test]
    fn output_that_isnt_utf8() {
        // Decoded first (as run_command does), then filtered.
        let raw = b"\xff\xfeuser=x password=hun\xffter2 end";
        let f = filter(&String::from_utf8_lossy(raw));
        assert!(f.text.contains(&format!("password={HIDDEN} end")), "{}", f.text);
        // UTF-16 text: read as its letters.
        let utf16: Vec<u8> = "DB_PASSWORD=hunter2\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let f = filter(&String::from_utf8_lossy(&utf16));
        assert_eq!(f.text, format!("DB_PASSWORD={HIDDEN}\n"));
    }

    #[test]
    fn counts_each_and_never_twice() {
        let f = filter("password=a\npassword=b\nhttp://u:p@h");
        assert_eq!(f.hidden, 3);
        // A value that's already hidden isn't counted again by a later rule.
        let f = filter(concat!("token=ghp", "_abcdefghijklmnopqrstuvwxyz0123456789"));
        assert_eq!(f.text, format!("token={HIDDEN}"));
        assert_eq!(f.hidden, 1);
        let f = filter("password=$6$saltsalt$abcdefghijklmnop");
        assert_eq!(f.text, format!("password={HIDDEN}"));
        assert_eq!(f.hidden, 1);
        // Plain output passes through as it is.
        hides("Filesystem Size Used\n/dev/sda1 40G 12G\n", "Filesystem Size Used\n/dev/sda1 40G 12G\n", 0);
    }
}
