//! Submarine: who is at the other end of a connection to this computer's
//! loopback (#135).
//!
//! An RDP server opened in this computer's RDP app through an SSH server is
//! reached by a forward on a loopback address (rdp.rs, `forward_through`).
//! Any user of this computer can connect to a loopback address, and so would
//! reach the RDP server through your SSH session. So each connection there is
//! checked first: the program that made it must run as you, or the
//! connection is closed. The system knows each connection's owner:
//!
//! - Linux: `/proc/net/tcp` and `tcp6` give each connection's user.
//! - Windows: the TCP table gives each connection's process
//!   (`GetExtendedTcpTable`), and that process's token its user.
//! - macOS: the connection is looked for among the sockets of your own
//!   processes (`proc_listpids`, `proc_pidfdinfo`); another user's aren't
//!   open to you, so a connection found there is yours.
//!
//! When the owner can't be told, the connection is refused.

use std::net::SocketAddr;

/// Whether the program that made a connection from `peer` to `listening`,
/// accepted here, runs as this user. Logs a refusal.
pub async fn checked(peer: SocketAddr, listening: SocketAddr) -> bool {
    let ours = tokio::task::spawn_blocking(move || from_this_user(peer, listening)).await.unwrap_or(false);
    if !ours {
        println!("[local] refused a connection from {peer} to {listening}: not a program of this user");
    }
    ours
}

/// The same, as it happens: blocking, without the log.
pub fn from_this_user(peer: SocketAddr, listening: SocketAddr) -> bool {
    os::from_this_user(peer, listening)
}

/// An address as the system may give it: an IPv4 one may come as IPv6
/// (`::ffff:a.b.c.d`) from a program using an IPv6 socket.
fn plain(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(v4.into(), v6.port()),
            None => addr,
        },
        v4 => v4,
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod os {
    use super::plain;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    pub fn from_this_user(peer: SocketAddr, listening: SocketAddr) -> bool {
        // SAFETY: geteuid can't fail.
        let me = unsafe { libc::geteuid() };
        ["/proc/net/tcp", "/proc/net/tcp6"]
            .iter()
            .filter_map(|table| std::fs::read_to_string(table).ok())
            .find_map(|table| owner_in(&table, peer, listening))
            == Some(me)
    }

    /// The user of the connection from `from` to `to` in a `/proc/net/tcp`
    /// table: its `local_address`, `rem_address` and `uid` columns.
    pub(super) fn owner_in(table: &str, from: SocketAddr, to: SocketAddr) -> Option<u32> {
        let (from, to) = (plain(from), plain(to));
        table.lines().skip(1).find_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let (local, remote, uid) = (cols.get(1)?, cols.get(2)?, cols.get(7)?);
            (address(local)? == from && address(remote)? == to).then(|| uid.parse().ok())?
        })
    }

    /// `0100007F:0D3D`: the address as the kernel keeps it, in hex words of
    /// this machine's byte order, then the port.
    fn address(text: &str) -> Option<SocketAddr> {
        let (ip, port) = text.split_once(':')?;
        let port = u16::from_str_radix(port, 16).ok()?;
        let word = |i: usize| ip.get(i * 8..i * 8 + 8).and_then(|w| u32::from_str_radix(w, 16).ok()).map(u32::to_ne_bytes);
        let ip: IpAddr = match ip.len() {
            8 => Ipv4Addr::from(word(0)?).into(),
            32 => {
                let mut bytes = [0u8; 16];
                for i in 0..4 {
                    bytes[i * 4..i * 4 + 4].copy_from_slice(&word(i)?);
                }
                Ipv6Addr::from(bytes).into()
            }
            _ => return None,
        };
        Some(plain(SocketAddr::new(ip, port)))
    }
}

#[cfg(windows)]
mod os {
    use super::plain;
    use std::ffi::c_void;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    pub fn from_this_user(peer: SocketAddr, listening: SocketAddr) -> bool {
        let Some(pid) = owner_pid(plain(peer), plain(listening)) else { return false };
        if pid == std::process::id() {
            return true;
        }
        let Ok(me) = crate::ai::channel::pipe::my_sid() else { return false };
        // SAFETY: the handle is checked, used for its token only and closed once.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let theirs = crate::ai::channel::pipe::user_sid(process);
            CloseHandle(process);
            theirs.is_ok_and(|theirs| theirs == me)
        }
    }

    /// The process that has the connection from `from` to `to`.
    fn owner_pid(from: SocketAddr, to: SocketAddr) -> Option<u32> {
        let v4 = table(u32::from(AF_INET))?;
        let rows4 = rows::<MIB_TCPROW_OWNER_PID>(&v4);
        let port = |p: u32| u16::from_be(p as u16);
        let found = rows4.iter().find(|r| {
            let at = |a: u32, p: u32| SocketAddr::new(Ipv4Addr::from(a.to_ne_bytes()).into(), port(p));
            at(r.dwLocalAddr, r.dwLocalPort) == from && at(r.dwRemoteAddr, r.dwRemotePort) == to
        });
        if let Some(r) = found {
            return Some(r.dwOwningPid);
        }
        let v6 = table(u32::from(AF_INET6))?;
        rows::<MIB_TCP6ROW_OWNER_PID>(&v6)
            .iter()
            .find(|r| {
                let at = |a: [u8; 16], p: u32| plain(SocketAddr::new(Ipv6Addr::from(a).into(), port(p)));
                at(r.ucLocalAddr, r.dwLocalPort) == from && at(r.ucRemoteAddr, r.dwRemotePort) == to
            })
            .map(|r| r.dwOwningPid)
    }

    /// The system's table of connections for one address family: a count,
    /// then the rows. In u64s, so the rows are aligned.
    fn table(family: u32) -> Option<Vec<u64>> {
        let mut size = 8u32;
        for _ in 0..4 {
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let mut room = u32::try_from(buf.len() * 8).ok()?;
            // SAFETY: `buf` holds `room` bytes; Windows writes at most that.
            let r = unsafe { GetExtendedTcpTable(buf.as_mut_ptr() as *mut c_void, &mut room, 0, family, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) };
            match r {
                NO_ERROR => return Some(buf),
                // Too small (the first time, or the table grew in between):
                // again, at the size it says now and some more.
                ERROR_INSUFFICIENT_BUFFER => size = room.saturating_add(4096),
                _ => return None,
            }
        }
        None
    }

    /// The rows of a table from `table`.
    fn rows<Row>(buf: &[u64]) -> &[Row] {
        let count = (buf[0] & 0xffff_ffff) as usize;
        // The rows follow the count, at the rows' own alignment (4).
        let start = std::mem::size_of::<u32>();
        let room = (buf.len() * 8).saturating_sub(start) / std::mem::size_of::<Row>();
        // SAFETY: the rows are within `buf` (count capped by its size), and
        // u32-aligned, all the alignment these rows need.
        unsafe { std::slice::from_raw_parts((buf.as_ptr() as *const u8).add(start) as *const Row, count.min(room)) }
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::plain;
    use std::ffi::{c_int, c_void};
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    // sys/proc_info.h; not in the libc crate.
    const PROC_UID_ONLY: u32 = 4;
    const PROC_PIDFDSOCKETINFO: c_int = 3;
    const SOCKINFO_TCP: c_int = 2;
    const INI_IPV4: u8 = 1;
    const INI_IPV6: u8 = 2;

    /// The start of `struct socket_fdinfo`, as far as a TCP socket's
    /// addresses (sys/proc_info.h): `proc_fileinfo`, then `socket_info` up to
    /// its `soi_proto`, whose `pri_tcp` starts with `in_sockinfo`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SocketFdinfo {
        pfi: [u64; 3],
        // `soi_stat` (struct vinfo_stat, 136 bytes): its owner at 16.
        stat_head: [u32; 4],
        stat_uid: u32,
        stat_rest: [u32; 29],
        so: u64,
        pcb: u64,
        kind_family: [c_int; 3],
        shorts: [i16; 8],
        oobmark: u32,
        buffers: [u32; 12],
        kind: c_int,
        rfu: u32,
        // `in_sockinfo`
        fport: c_int,
        lport: c_int,
        gencnt: u64,
        flags: u32,
        flow: u32,
        vflag: u8,
        ttl: u8,
        rfu_1: u32,
        faddr: [u8; 16],
        laddr: [u8; 16],
        v4_v6: [u32; 4],
    }

    pub fn from_this_user(peer: SocketAddr, listening: SocketAddr) -> bool {
        // SAFETY: geteuid can't fail.
        let me = unsafe { libc::geteuid() };
        let (from, to) = (plain(peer), plain(listening));
        pids(me).into_iter().any(|pid| sockets(pid).into_iter().any(|fd| socket_of(pid, fd).is_some_and(|s| s.stat_uid == me && ends(&s) == Some((from, to)))))
    }

    /// The processes of user `uid`.
    fn pids(uid: u32) -> Vec<c_int> {
        // SAFETY: a null buffer asks for the size only; then `buf` has room
        // for what was asked, plus some for processes started in between.
        unsafe {
            let size = libc::proc_listpids(PROC_UID_ONLY, uid, std::ptr::null_mut(), 0);
            if size <= 0 {
                return Vec::new();
            }
            let mut buf = vec![0 as c_int; size as usize / std::mem::size_of::<c_int>() + 64];
            let got = libc::proc_listpids(PROC_UID_ONLY, uid, buf.as_mut_ptr() as *mut c_void, (buf.len() * std::mem::size_of::<c_int>()) as c_int);
            buf.truncate(got.max(0) as usize / std::mem::size_of::<c_int>());
            buf.retain(|&pid| pid > 0);
            buf
        }
    }

    /// The socket descriptors of `pid`.
    fn sockets(pid: c_int) -> Vec<c_int> {
        let one = std::mem::size_of::<libc::proc_fdinfo>();
        // SAFETY: as in `pids`.
        unsafe {
            let size = libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0);
            if size <= 0 {
                return Vec::new();
            }
            let mut buf = vec![libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 }; size as usize / one + 16];
            let got = libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, buf.as_mut_ptr() as *mut c_void, (buf.len() * one) as c_int);
            buf.truncate(got.max(0) as usize / one);
            buf.into_iter().filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32).map(|f| f.proc_fd).collect()
        }
    }

    /// What the system says of socket `fd` of `pid`, if it is TCP.
    fn socket_of(pid: c_int, fd: c_int) -> Option<SocketFdinfo> {
        // Room for all of `struct socket_fdinfo`, which is longer than the
        // part read here; the system asks for at least its own size.
        let mut buf = [0u64; 256];
        // SAFETY: the system writes at most the size given; what it wrote is
        // read only when it covers `SocketFdinfo`.
        unsafe {
            let got = libc::proc_pidfdinfo(pid, fd, PROC_PIDFDSOCKETINFO, buf.as_mut_ptr() as *mut c_void, std::mem::size_of_val(&buf) as c_int);
            if got < std::mem::size_of::<SocketFdinfo>() as c_int {
                return None;
            }
            let info = std::ptr::read(buf.as_ptr() as *const SocketFdinfo);
            (info.kind == SOCKINFO_TCP).then_some(info)
        }
    }

    /// A TCP socket's own address and the one it is connected to.
    fn ends(s: &SocketFdinfo) -> Option<(SocketAddr, SocketAddr)> {
        let ip = |a: [u8; 16]| -> Option<std::net::IpAddr> {
            if s.vflag & INI_IPV4 != 0 {
                Some(Ipv4Addr::new(a[12], a[13], a[14], a[15]).into())
            } else if s.vflag & INI_IPV6 != 0 {
                Some(Ipv6Addr::from(a).into())
            } else {
                None
            }
        };
        let port = |p: c_int| u16::from_be(p as u16);
        Some((plain(SocketAddr::new(ip(s.laddr)?, port(s.lport))), plain(SocketAddr::new(ip(s.faddr)?, port(s.fport)))))
    }

    #[cfg(test)]
    #[test]
    fn the_layout_is_the_systems() {
        // Where sys/proc_info.h has them, from the start of socket_fdinfo.
        assert_eq!(std::mem::offset_of!(SocketFdinfo, stat_uid), 24 + 16);
        assert_eq!(std::mem::offset_of!(SocketFdinfo, so), 24 + 136);
        assert_eq!(std::mem::offset_of!(SocketFdinfo, kind), 24 + 232);
        assert_eq!(std::mem::offset_of!(SocketFdinfo, fport), 24 + 240);
        assert_eq!(std::mem::offset_of!(SocketFdinfo, faddr), 24 + 240 + 32);
        assert_eq!(std::mem::offset_of!(SocketFdinfo, laddr), 24 + 240 + 48);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", windows, target_os = "macos")))]
mod os {
    /// No way to tell here: refused.
    pub fn from_this_user(_peer: std::net::SocketAddr, _listening: std::net::SocketAddr) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_from_this_program_is_this_users() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let listening = listener.local_addr().unwrap();
        let _client = std::net::TcpStream::connect(listening).unwrap();
        let (_server, peer) = listener.accept().unwrap();
        assert!(from_this_user(peer, listening));
        // A connection that isn't there has no owner to be.
        let nobody = SocketAddr::new(peer.ip(), if peer.port() == 1 { 2 } else { 1 });
        assert!(!from_this_user(nobody, listening));
    }

    #[test]
    fn an_ipv4_address_through_an_ipv6_socket_is_the_same() {
        let v4: SocketAddr = "127.0.0.1:3389".parse().unwrap();
        assert_eq!(plain("[::ffff:127.0.0.1]:3389".parse().unwrap()), v4);
        assert_eq!(plain(v4), v4);
        let v6: SocketAddr = "[::1]:3389".parse().unwrap();
        assert_eq!(plain(v6), v6);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn the_owner_is_read_from_the_kernels_table() {
        // As /proc/net/tcp has it (x86 and ARM: little-endian): user 1000's
        // 127.4.5.6:51000 connected to 127.1.2.3:3389, and user 0's own end.
        let tcp = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   \
            0: 0302017F:0D3D 0605047F:C738 01 00000000:00000000 00:00000000 00000000     0        0 1 1 0000000000000000 20 4 30 10 -1\n   \
            1: 0605047F:C738 0302017F:0D3D 01 00000000:00000000 00:00000000 00000000  1000        0 2 1 0000000000000000 20 4 30 10 -1\n";
        let (peer, listening) = ("127.4.5.6:51000".parse().unwrap(), "127.1.2.3:3389".parse().unwrap());
        assert_eq!(os::owner_in(tcp, peer, listening), Some(1000));
        assert_eq!(os::owner_in(tcp, listening, peer), Some(0));
        assert_eq!(os::owner_in(tcp, peer, "127.1.2.3:3390".parse().unwrap()), None);
        // tcp6, from a program that connected with an IPv6 socket: ::ffff:127.4.5.6.
        let tcp6 = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   \
            0: 0000000000000000FFFF00000605047F:C738 0000000000000000FFFF00000302017F:0D3D 01 00000000:00000000 00:00000000 00000000  1001        0 3 1 0000000000000000 20 4 30 10 -1\n";
        assert_eq!(os::owner_in(tcp6, peer, listening), Some(1001));
    }

    #[test]
    fn the_hooks_are_in_place() {
        crate::fork_hooks::assert_hooks("rdp.rs", include_str!("rdp.rs"), &[("forward_through", "crate::local_peer::checked(peer, local)", 1)]);
    }
}
