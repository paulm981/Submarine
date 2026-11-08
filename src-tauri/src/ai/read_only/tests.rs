//! What runs at "Run commands that only read", and every way around it we
//! know of.

use super::*;

fn runs(list: &[&str]) {
    for c in list {
        assert!(only_reads(c), "should run: {c}");
    }
}

fn asks(list: &[&str]) {
    for c in list {
        assert!(!only_reads(c), "should ask: {c}");
    }
}

#[test]
fn everyday_looking_runs() {
    runs(&[
        "uptime",
        "df -h",
        "free -m",
        "ps aux",
        "ps aux | grep nginx",
        "ps -ef | grep -v grep | grep 'java -jar' | head -n 5",
        "ls -la /var/log",
        "ls -la /var/log/*.log",
        "cat /etc/os-release",
        "tail -n 100 /var/log/syslog",
        "tail -f /var/log/nginx/access.log",
        "grep -i 'error' /var/log/syslog | tail -n 20",
        "grep -E '^[0-9]+$' /tmp/numbers.txt",
        "du -sh /var/lib/docker",
        "wc -l /var/log/auth.log",
        "sort /tmp/a.txt | uniq -c | sort -rn | head",
        "find /var/log -name '*.gz' -mtime +7",
        "find /etc -maxdepth 1 -type f",
        "ss -tlnp",
        "lsof -i :443",
        "uname -a",
        "id",
        "date +%F",
        "date -d yesterday +%F",
        "date -u",
        "hostname -f",
        "hostname",
        "dmesg -T | tail",
        "sysctl net.ipv4.ip_forward",
        "sysctl -a",
        "crontab -l",
        "ifconfig -a",
        "ifconfig eth0",
        "md5sum /usr/bin/ls",
        "getent passwd paul",
        "which nginx",
        "\"cat\" /etc/hosts",
        "cat app*.log",
        "cat ./*.conf",
    ]);
}

#[test]
fn services_and_containers_run_when_they_only_show() {
    runs(&[
        "systemctl status nginx",
        "systemctl status nginx --no-pager -l",
        "systemctl list-units --type=service --state=failed",
        "systemctl is-active sshd",
        "systemctl cat nginx",
        "journalctl -u nginx -n 50 --no-pager",
        "journalctl -u nginx -f",
        "journalctl --since=today -p err",
        "journalctl --disk-usage",
        "docker ps -a",
        "docker logs --tail 100 web",
        "docker logs -f web",
        "docker inspect --format '{{.State.Status}}' web",
        "docker container ls",
        "docker image ls",
        "docker system df",
        "podman ps",
        "ip a",
        "ip -br addr",
        "ip -4 addr show dev eth0",
        "ip route",
        "ip route get 1.1.1.1",
        "ip link show",
        "dpkg -l",
        "dpkg -l nginx",
        "dpkg-query -W -f '${Version}' nginx",
        "rpm -qa",
        "rpm -qi openssl",
        "apt list --installed",
        "apt policy nginx",
        "apt-cache policy nginx",
        "timedatectl",
        "hostnamectl status",
    ]);
}

#[test]
fn anything_that_changes_things_asks() {
    asks(&[
        "rm -rf /tmp/x",
        "touch /tmp/x",
        "mv a b",
        "cp a b",
        "chmod 777 /etc/passwd",
        "kill 1234",
        "pkill nginx",
        "reboot",
        "systemctl restart nginx",
        "systemctl stop nginx",
        "systemctl enable nginx",
        "systemctl daemon-reload",
        "journalctl --vacuum-size=100M",
        "journalctl --vacuum-time=1d",
        "journalctl --vac=1",
        "journalctl --rotate",
        "journalctl --rot",
        "journalctl --flush",
        "journalctl --cursor-file=/tmp/c",
        "docker rm web",
        "docker stop web",
        "docker exec web sh",
        "docker run alpine",
        "docker -H ps rm web",
        "docker container rm web",
        "docker image prune",
        "docker system prune",
        "ip link set eth0 down",
        "ip addr add 10.0.0.1/24 dev eth0",
        "ip route flush all",
        "ip -b /tmp/cmds",
        "ip -batch /tmp/cmds",
        "ip netns exec x sh",
        "ip -n ns a",
        "sort -o /etc/passwd /tmp/x",
        "sort -uo /etc/passwd /tmp/x",
        "sort --output=/etc/passwd /tmp/x",
        "sort --out=/etc/passwd /tmp/x",
        "sort --compress-program=/tmp/evil x",
        "sort --comp=/tmp/evil x",
        "uniq /tmp/in /tmp/out",
        "find / -delete",
        "find /tmp -name x -exec rm {} ;",
        "find /tmp -execdir sh",
        "find /tmp -exec rm x",
        "find /tmp -ok rm x",
        "find / -fprint /tmp/list",
        "find / '-delete'",
        "dmesg -c",
        "dmesg -C",
        "dmesg --clear",
        "dmesg -n 1",
        "dmesg -Tc",
        "date -s 2020-01-01",
        "date --set=2020-01-01",
        "date 010100002020",
        "hostname evil",
        "hostname -F /tmp/name",
        "hostnamectl set-hostname evil",
        "timedatectl set-time 2020-01-01",
        "sysctl -w net.ipv4.ip_forward=1",
        "sysctl net.ipv4.ip_forward=1",
        "sysctl -p",
        "ifconfig eth0 down",
        "ifconfig eth0 10.0.0.1",
        "crontab -r",
        "crontab /tmp/evil",
        "ss -K dst 10.0.0.1",
        "ss -D /tmp/out",
        "file -C -m /tmp/magic",
        "dpkg -i x.deb",
        "dpkg -l -i x.deb",
        "dpkg --purge nginx",
        "rpm -i x.rpm",
        "rpm -e openssl",
        "rpm -qe openssl",
        "rpm -qa --setperms",
        "apt install nginx",
        "apt remove nginx",
        "apt -o APT::Update::Pre-Invoke::=x list",
        "apt list -o Dir::Cache=/tmp",
        "apt-cache gencaches",
        "systemctl -H evil status",
        "systemctl status -H evil",
        "systemctl --host=evil status",
    ]);
}

#[test]
fn programs_that_can_run_or_write_while_looking_ask() {
    asks(&[
        "sed -n 1,10p /etc/hosts",
        "sed 's/a/b/e' /etc/hosts",
        "awk '{print $1}' /etc/hosts",
        "git status",
        "git log -1",
        "less /etc/hosts",
        "more /etc/hosts",
        "man ls",
        "jq . /tmp/x.json",
        "env",
        "printenv",
        "xargs rm",
        "ls | xargs rm",
        "watch ls",
        "timeout 5 ls",
        "nice ls",
        "tee /tmp/x",
        "ls | tee /tmp/x",
        "cat x | sh",
        "cat x | bash",
        "tree -o /tmp/x",
        "xxd /tmp/a /tmp/b",
        "vi /etc/hosts",
        "python3 -c 'print(1)'",
        "perl -e 'print 1'",
        "sudo cat /var/log/syslog",
        "su -c 'cat /x'",
    ]);
}

#[test]
fn reaching_out_asks() {
    asks(&[
        "curl https://example.com",
        "wget https://example.com",
        "nc example.com 80",
        "ssh other",
        "scp a other:",
        "rsync a other:",
        "ping -c 1 example.com",
        "dig example.com",
        "nslookup example.com",
        "host example.com",
        "telnet example.com 25",
    ]);
}

#[test]
fn anything_a_shell_reads_as_more_asks() {
    asks(&[
        "ls; rm -rf /",
        "ls && rm x",
        "ls || rm x",
        "ls & rm x",
        "ls > /tmp/x",
        "ls >> /tmp/x",
        "cat < /etc/hosts",
        "cat $(which rm)",
        "cat `which rm`",
        "cat ${HOME}/x",
        "cat $HOME/x",
        "(rm x)",
        "{ rm x; }",
        "cat x\\;rm",
        "ls #comment",
        "ls !",
        "ls ^x",
        "ls\trm",
        "ls\nrm x",
        "ls\rrm x",
        "ls |",
        "| ls",
        "ls | | cat",
        "",
        "   ",
        "FOO=bar ls",
        "LD_PRELOAD=/tmp/x.so ls",
        "PAGER=sh systemctl status",
        "./ls",
        "/tmp/evil/cat x",
        "/bin/cat x",
        "cat \"$HOME\"",
        "cat \"`rm x`\"",
        "cat \"a\\\"b\"",
        "cat 'unterminated",
        "cat \"unterminated",
        "grep 'a\\' x",
        "cat x\u{202e}",
        "cat x\u{200b}y",
        "cat ｘ",
    ]);
}

#[test]
fn a_glob_that_could_turn_into_an_option_asks() {
    // A file named `-delete` or `-o/etc/passwd` would become an option.
    asks(&["ls *.log", "sort *", "find . -name x *", "cat ?x", "cat [ab]x", "sort -f* x", "cat -*"]);
    // Quoted, it's the program's to read, not the shell's.
    runs(&["find /var/log -name '*.log'", "grep -r --include='*.conf' listen /etc/nginx"]);
}

#[test]
fn files_that_usually_hold_secrets_ask() {
    asks(&[
        "cat .env",
        "cat /srv/app/.env",
        "cat /srv/app/.env.production",
        "cat ~/.ssh/id_rsa",
        "cat /home/paul/.ssh/authorized_keys",
        "ls ~/.ssh",
        "cat /root/.aws/credentials",
        "cat ~/.netrc",
        "cat ~/.pgpass",
        "cat ~/.git-credentials",
        "cat /etc/shadow",
        "cat /etc/gshadow",
        "cat /etc/sudoers",
        "getent shadow root",
        "cat /etc/ssl/private/server.key",
        "cat /etc/ssh/ssh_host_ed25519_key",
        "cat /etc/letsencrypt/live/x/privkey.pem",
        "cat /srv/vault.kdbx",
        "grep -f /srv/.ENV x",
        "journalctl --file=/home/u/.ssh/x",
        "cat ~/.kube/config",
        "cat ~/.docker/config.json",
        "cat /run/secrets/db",
        "cat ~/.npmrc",
        "find / -name id_ed25519",
    ]);
    assert!(names_secret("/a/b/.env"));
    assert!(!names_secret("/etc/environment"));
    assert!(!names_secret("/var/log/syslog"));
    assert!(!names_secret("identity"));
}

#[test]
fn devices_ask_however_they_re_spelled() {
    asks(&[
        "cat /dev/watchdog",
        "head -c 10 /dev/sda",
        "cat //dev/watchdog",
        "cat /./dev/watchdog",
        "cat ../../dev/watchdog",
        "cat /var/../dev/watchdog",
        "cat dev/watchdog",
        "cat /d*/watchdog",
        "cat /?ev/watchdog",
        "tail -f /proc/kmsg",
        "journalctl --file=/dev/x",
        "grep -D read -r x /",
        "grep --devices=read -r x /",
        "grep -rD read x /",
    ]);
    runs(&["cat /proc/meminfo", "cat /sys/class/net/eth0/address", "grep -r listen /etc/nginx", "cat /var/log/*/error.log", "cat app*.log"]);
}

#[test]
fn too_much_asks() {
    asks(&[&format!("cat {}", "a".repeat(MAX_LEN))]);
    runs(&[&vec!["cat x"; MAX_STAGES].join(" | ")]);
    asks(&[&vec!["cat x"; MAX_STAGES + 1].join(" | ")]);
}

#[test]
fn the_pipeline_is_read_as_the_shell_reads_it() {
    let p = pipeline("grep -i 'a b'\"c\" /x | wc -l").unwrap();
    let texts: Vec<Vec<&str>> = p.iter().map(|s| s.iter().map(|w| w.text.as_str()).collect()).collect();
    assert_eq!(texts, vec![vec!["grep", "-i", "a bc", "/x"], vec!["wc", "-l"]]);
    // An empty quoted word is a word.
    assert_eq!(pipeline("grep '' /x").unwrap()[0].len(), 3);
    assert!(pipeline("ls /a/*").unwrap()[0][1].glob);
    assert!(!pipeline("ls '/a/*'").unwrap()[0][1].glob);
}
