//! "Why did the connect fail?" (#35): a short check under the error that says
//! what got through. Does the name resolve, and to what; does the port
//! answer, and how fast. Plain TCP, no login, so it works whatever the
//! failure was.
//!
//! Where the connection goes through something else, that is what gets
//! checked: a SOCKS / HTTP proxy's own address, or a jump host's. The target
//! behind a jump host would need a login to reach, so it is left alone and
//! the line says so. A ProxyCommand is a program, not an address; there is
//! nothing to dial.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

/// How long the name lookup and the TCP connect each get. Short, because the
/// connect's own retry runs alongside and the answer is only a hint.
const STEP_TIMEOUT: Duration = Duration::from_secs(3);

/// How many resolved addresses a line lists before it says "and N more".
const SHOWN_ADDRESSES: usize = 3;

/// How the connection reaches its target, as far as this check cares.
pub enum Route {
    /// Straight to the target.
    Direct,
    /// Through a SOCKS5 / HTTP proxy: the proxy is what the socket opens to.
    Proxy { kind: &'static str, host: String, port: u16 },
    /// Through a jump host (named for the line).
    Jump { name: String, host: String, port: u16 },
    /// Through a ProxyCommand.
    Command,
}

/// What the TCP connect to one address came to.
#[derive(Debug, PartialEq)]
enum Reach {
    Answers(u128),
    Refused,
    TimedOut,
    Unreachable,
    Failed(String),
}

/// The outcome for one `host:port`.
#[derive(Debug, PartialEq)]
struct Probe {
    host: String,
    port: u16,
    /// Empty when the host is already an address.
    addresses: Vec<IpAddr>,
    /// `Err` is why the name didn't resolve.
    outcome: Result<Reach, String>,
}

/// Look `host` up (unless it is an address) and try a TCP connect to what
/// comes back, in order, within one overall `STEP_TIMEOUT`. The first
/// address that answers wins; otherwise the last failure is reported.
async fn probe(host: &str, port: u16) -> Probe {
    let literal = host.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>().ok();
    let addresses: Vec<IpAddr> = match literal {
        Some(ip) => vec![ip],
        None => match tokio::time::timeout(STEP_TIMEOUT, tokio::net::lookup_host((host, port))).await {
            Ok(Ok(found)) => {
                let mut list: Vec<IpAddr> = Vec::new();
                for a in found {
                    if !list.contains(&a.ip()) {
                        list.push(a.ip());
                    }
                }
                list
            }
            Ok(Err(_)) => return unresolved(host, port, "name not found"),
            Err(_) => return unresolved(host, port, "the lookup timed out"),
        },
    };
    if addresses.is_empty() {
        return unresolved(host, port, "no address came back");
    }

    let started = Instant::now();
    let mut last = Reach::TimedOut;
    for ip in &addresses {
        let Some(left) = STEP_TIMEOUT.checked_sub(started.elapsed()) else { break };
        let tried = Instant::now();
        last = match tokio::time::timeout(left, tokio::net::TcpStream::connect(SocketAddr::new(*ip, port))).await {
            Ok(Ok(_)) => Reach::Answers(tried.elapsed().as_millis()),
            Ok(Err(e)) => classify(&e),
            Err(_) => Reach::TimedOut,
        };
        if matches!(last, Reach::Answers(_)) {
            break;
        }
    }
    Probe {
        host: host.to_string(),
        port,
        addresses: if literal.is_some() { Vec::new() } else { addresses },
        outcome: Ok(last),
    }
}

fn unresolved(host: &str, port: u16, why: &str) -> Probe {
    Probe { host: host.to_string(), port, addresses: Vec::new(), outcome: Err(why.to_string()) }
}

/// The kind of failure a socket error stands for. Unreachable is matched on
/// the message: the dedicated `io::ErrorKind`s differ by platform.
fn classify(e: &std::io::Error) -> Reach {
    let text = e.to_string().to_lowercase();
    if e.kind() == std::io::ErrorKind::ConnectionRefused {
        Reach::Refused
    } else if e.kind() == std::io::ErrorKind::TimedOut {
        Reach::TimedOut
    } else if text.contains("unreachable") || text.contains("no route") {
        Reach::Unreachable
    } else {
        Reach::Failed(e.to_string())
    }
}

/// One plain line: `web-01 → 10.0.0.5 · port 22 answers in 14 ms`.
fn describe(p: &Probe) -> String {
    let mut line = p.host.clone();
    let port = p.port;
    let reach = match &p.outcome {
        Ok(reach) => reach,
        Err(why) => return format!("{line} doesn't resolve ({why})"),
    };
    if !p.addresses.is_empty() {
        let shown: Vec<String> = p.addresses.iter().take(SHOWN_ADDRESSES).map(|a| a.to_string()).collect();
        line.push_str(&format!(" → {}", shown.join(", ")));
        if p.addresses.len() > SHOWN_ADDRESSES {
            line.push_str(&format!(" and {} more", p.addresses.len() - SHOWN_ADDRESSES));
        }
    }
    match reach {
        Reach::Answers(ms) => format!("{line} · port {port} answers in {ms} ms"),
        Reach::Refused => format!("{line} · port {port}: connection refused"),
        Reach::TimedOut => format!("{line} · port {port}: no answer within {} s", STEP_TIMEOUT.as_secs()),
        Reach::Unreachable => format!("{line} · port {port}: network unreachable"),
        Reach::Failed(e) => format!("{line} · port {port}: {e}"),
    }
}

/// The lines to show under a failed connect to `host:port` over `route`.
pub async fn diagnose(host: &str, port: u16, route: Route) -> Vec<String> {
    match route {
        Route::Direct => vec![describe(&probe(host, port).await)],
        Route::Proxy { kind, host: proxy_host, port: proxy_port } => {
            let p = probe(&proxy_host, proxy_port).await;
            vec![format!("{kind} proxy: {}", describe(&p))]
        }
        Route::Jump { name, host: jump_host, port: jump_port } => {
            let p = probe(&jump_host, jump_port).await;
            vec![
                format!("Jump host {name}: {}", describe(&p)),
                format!("{host} is behind {name}: checked the jump host only."),
            ]
        }
        Route::Command => vec!["This connection goes through a ProxyCommand, so it can't be checked directly.".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free_port() -> u16 {
        // Bind to find a free port, then close it again.
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn listening_port_answers() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let p = probe("127.0.0.1", port).await;
        assert!(matches!(p.outcome, Ok(Reach::Answers(_))), "{p:?}");
        assert!(describe(&p).contains(&format!("port {port} answers in")));
    }

    #[tokio::test]
    async fn closed_port_is_refused() {
        let p = probe("127.0.0.1", free_port()).await;
        assert_eq!(p.outcome, Ok(Reach::Refused));
        assert!(describe(&p).ends_with("connection refused"));
    }

    #[tokio::test]
    async fn unresolvable_name_doesnt_resolve() {
        // `.invalid` is reserved: it never resolves.
        let p = probe("nothing-here.invalid", 22).await;
        assert!(p.outcome.is_err(), "{p:?}");
        assert!(describe(&p).starts_with("nothing-here.invalid doesn't resolve"));
    }

    #[tokio::test]
    async fn localhost_lists_its_address() {
        let lines = diagnose("localhost", free_port(), Route::Direct).await;
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("localhost → "), "{lines:?}");
    }

    #[tokio::test]
    async fn jump_host_is_checked_and_target_is_not() {
        let route = Route::Jump { name: "bastion".into(), host: "127.0.0.1".into(), port: free_port() };
        let lines = diagnose("web-01", 22, route).await;
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("Jump host bastion: 127.0.0.1"), "{lines:?}");
        assert!(lines[1].contains("web-01 is behind bastion: checked the jump host only"));
    }

    #[tokio::test]
    async fn proxy_command_is_skipped() {
        let lines = diagnose("web-01", 22, Route::Command).await;
        assert!(lines[0].contains("ProxyCommand"));
    }

    #[test]
    fn long_address_lists_are_shortened() {
        let p = Probe {
            host: "multi".into(),
            port: 22,
            addresses: (1..=5).map(|i| IpAddr::from([10, 0, 0, i])).collect(),
            outcome: Ok(Reach::Answers(3)),
        };
        assert_eq!(describe(&p), "multi → 10.0.0.1, 10.0.0.2, 10.0.0.3 and 2 more · port 22 answers in 3 ms");
    }
}
