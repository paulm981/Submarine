//! Deterministic adversarial & property-based fuzz tests for the import parsers (#61):
//! MobaXterm (.ini), PuTTY (.reg), OpenSSH (~/.ssh/config), and FileZilla (.xml).
//! Runs under normal `cargo test` with zero GitHub Actions cron cost.

use super::*;
use crate::{parse_filezilla_sites, parse_mobaxterm_sessions, parse_putty_reg};

/// Fast deterministic PRNG (SplitMix64) so failures are 100% reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() as usize) % items.len()]
    }

    fn usize_below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

const FRAGMENTS: &[&str] = &[
    "", " ", "\t", "\r\n", "\n", "\0", "\u{feff}",
    "%", "%%", "%20", "%ZZ", "%E2%82", "%h", "%r", "%u", "%%h",
    "__PTVIRG__", "__PIPE__", "__DBLQUO__", "__DIEZE__", "__PERCENT__",
    "Host ", "HostName ", "User ", "Port ", "ProxyJump ", "IdentityFile ", "Match host ",
    "!*", "*", "?", "=", "\"", "'", "\\", "[", "]", "<", ">", "&amp;", "&quot;",
    "0", "22", "65535", "65536", "-1", "999999999999", "dword:ffffffff", "dword:zzzz",
    "[Bookmarks]\r\n", "SubRep=", "ImgNum=42\r\n",
    "[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\",
    "\"HostName\"=", "\"PortNumber\"=", "\"Protocol\"=", "\"UserName\"=",
    "Windows Registry Editor Version 5.00\r\n",
    "# comment\n", "none", "auto", "bastion", "a,b,c",
    "109", "91", "128", "ssh", "telnet", "serial", "raw",
    "_ProfileDir_\\", "_CurrentDrive_:", "_MyDocuments_\\",
    "äöü", "€", "🔑", "../..", "\x1b[31m",
];

fn synth_input(rng: &mut Rng, header: &str, max_parts: usize) -> String {
    let mut s = String::from(header);
    let count = 1 + rng.usize_below(max_parts);
    for _ in 0..count {
        s.push_str(rng.pick(FRAGMENTS));
    }
    s
}

#[test]
fn mobaxterm_parser_never_panics_on_fuzzed_or_adversarial_ini() {
    let mut rng = Rng(0x61_4d_6f_62_61_u64);
    let fixed = [
        "",
        "\0\0\r\n\u{feff}",
        "[Bookmarks]\nSubRep=ä\\ö\\€\nImgNum=42\nbad=not%enough",
        "[Bookmarks]\r\nSubRep=\r\nImgNum=41\r\nweird=0%h%%%-1%-1%__PERCENT____PTVIRG____PIPE__%%%",
        &export(&[session("unicode-ä€🔑", 109, "0%ä.example%99999%[ops]%%-1%-1%echo € && __PIPE__%jump%badport%%%_ProfileDir_\\k")]),
        &export(&[session("bad-port-neg", 109, "0%host%-5%u%%-1%-1%%%%%0%0%0%%%-1")]),
        &export(&[session("many-percents", 109, &"%".repeat(500))]),
        &export(&[session("rdp-short", 91, "4%rdp.example")]),
    ];
    for case in fixed {
        let _ = parse_mobaxterm_sessions(case);
        let _ = parse_mobaxterm_sessions_in(case, Some(r"C:\Users\ä"), Some(r"C:\Users\ä\Docs"));
        let _ = mobaxterm_skipped(case);
    }
    for _ in 0..400 {
        let fuzzed = synth_input(&mut rng, "[Bookmarks]\r\nImgNum=42\r\n", 40);
        let _ = parse_mobaxterm_sessions(&fuzzed);
        let _ = parse_mobaxterm_sessions_in(&fuzzed, Some(r"C:\Home"), None);
        let _ = mobaxterm_skipped(&fuzzed);

        let line = synth_input(&mut rng, "fuzz=", 28);
        let wrapped = export(&[line]);
        let _ = parse_mobaxterm_sessions(&wrapped);
    }
}

#[test]
fn putty_reg_parser_never_panics_on_fuzzed_or_adversarial_reg() {
    let mut rng = Rng(0x61_50_75_54_54_79_u64);
    let fixed = [
        "",
        "\u{feff}Windows Registry Editor Version 5.00\r\n",
        "REGEDIT4\r\n[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\%ZZ%E2%82%ä]\r\n\"HostName\"=\"h\"\r\n\"PortNumber\"=dword:ffffffff\r\n",
        "[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\unclosed\"quote\r\n\"HostName\"=\"bad\\\"\\\\\"\r\n\"Protocol\"=\"ssh\"\r\n\"PortNumber\"=dword:not_hex\r\n",
        "[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\]\r\n\"HostName\"=\"\"\r\n",
    ];
    for case in fixed {
        let _ = parse_putty_reg(case);
        let _ = parse_client_hosts(case.to_string());
        let _ = parse_client_import(case.to_string());
    }
    for _ in 0..400 {
        let fuzzed = synth_input(
            &mut rng,
            "Windows Registry Editor Version 5.00\r\n[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\f%20z]\r\n",
            36,
        );
        let _ = parse_putty_reg(&fuzzed);
        let _ = parse_client_hosts(fuzzed);
    }
}

#[test]
fn ssh_config_parser_and_jump_resolver_never_panic_on_fuzzed_inputs() {
    let mut rng = Rng(0x61_53_53_48_63_u64);
    let fixed = [
        "",
        "\u{feff}\0\r\n",
        "Host a\n\tHostName %h.%h.example\n\tIdentityFile ~/.ssh/%%h-%r-%u-%p\n\tProxyJump a,b,self\n",
        "Host !* *\n\tHostName = \"unterminated\n\tPort 99999999999999999999\n\tPort not_a_num\n",
        "Match all host *\n\tHostName bad\nHost good\n\tHostName äöü.example\n\tUser €\n",
        "Host ",
        &"Host h\n\tProxyJump h\n".repeat(20),
    ];
    for case in fixed {
        let mut hosts = crate::parse_ssh_config_text(case);
        crate::resolve_proxy_jump_aliases(&mut hosts);
    }
    for _ in 0..400 {
        let fuzzed = synth_input(&mut rng, "", 48);
        let mut hosts = crate::parse_ssh_config_text(&fuzzed);
        for h in &hosts {
            assert!(!h.host_alias.is_empty());
        }
        crate::resolve_proxy_jump_aliases(&mut hosts);
    }
}

#[test]
fn filezilla_xml_parser_never_panics_on_fuzzed_or_malformed_xml() {
    let mut rng = Rng(0x61_46_5a_78_6d_u64);
    let fixed = [
        "",
        "<FileZilla3><Servers><Server><Host>h</Host><Port>abc</Port><Pass encoding=\"base64\">!!!not-b64!!!</Pass></Server></Servers></FileZilla3>",
        "<FileZilla3><Servers><Folder>ä<Server><Host>h</Host><Protocol>1</Protocol><Name><![CDATA[x<y>]]></Name></Server>",
        "<FileZilla3><Servers><Server><Host>&amp;&#99999999;</Host></Server></Servers></FileZilla3>",
    ];
    for case in fixed {
        let _ = parse_filezilla_sites(case);
    }
    for _ in 0..250 {
        let fuzzed = synth_input(&mut rng, "<FileZilla3><Servers>", 36);
        let _ = parse_filezilla_sites(&fuzzed);
    }
}
