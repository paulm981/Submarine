//! What an AI tab is given of a command's output: the output with the secrets
//! it's known to carry replaced by `[hidden by Submarine]`. You see the whole
//! output in the AI panel, and how many things were hidden.
//!
//! The rules are fixed (#32, "Secrets reaching the AI service"): private key
//! blocks, the value after `password=`, `secret:`, `token=` and their kin, an
//! Authorization header's credentials, password hashes, cloud and service
//! access keys, and the password in a URL. They catch the common shapes, not
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
}

fn rule(pattern: &str, group: usize) -> Rule {
    Rule { re: Regex::new(pattern).expect("filter rule"), group }
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // A private key, from its BEGIN line to its END line (or to the end of
        // the output, when it was cut): PEM, OpenSSH, PGP.
        rule(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?s:.*?)(?:-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\z)", 0),
        // A PuTTY key's private part.
        rule(r"(?m)^Private-Lines: *\d+\r?\n((?:[A-Za-z0-9+/=]+(?:\r?\n|\z))+)", 1),
        // Password hashes as crypt(3) writes them, in /etc/shadow and
        // elsewhere: MD5, bcrypt, SHA-256/512, yescrypt, scrypt, argon2.
        rule(r"\$(?:1|2[abxy]?|5|6|7|y|gy|md5|sha1|argon2(?:id|i|d))\$[^\s:]{8,}", 0),
        // `password=…`, `secret: …`, `"api_key": "…"`: the value.
        rule(
            r#"(?i)[A-Za-z0-9_.-]*(?:password|passwd|passphrase|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|account[_-]?key)(?:[_.-][A-Za-z0-9_.-]*)?["']?[ \t]*[:=][ \t]*("[^"\r\n]+"|'[^'\r\n]+'|[^\s"',;]+)"#,
            1,
        ),
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
    ]
});

/// `text` with every secret the rules find replaced, rule by rule.
pub(crate) fn filter(text: &str) -> Filtered {
    let mut out = text.to_string();
    let mut hidden = 0;
    for r in RULES.iter() {
        // What earlier rules hid: a match touching one is left as it is.
        let done: Vec<(usize, usize)> = out.match_indices(HIDDEN).map(|(i, s)| (i, i + s.len())).collect();
        let mut next = String::with_capacity(out.len());
        let mut last = 0;
        for caps in r.re.captures_iter(&out) {
            let Some(m) = caps.get(r.group) else { continue };
            if done.iter().any(|&(a, b)| m.start() < b && a < m.end()) {
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
