//! The local channel between `submarine --mcp` (helper.rs) and the running
//! app. No network port, ever:
//!
//! - **macOS and Linux:** a Unix socket in a folder only you can open (made
//!   with mode 0700 under `$XDG_RUNTIME_DIR`, or the per-user temp folder),
//!   and each connection is checked to come from a process running as you.
//! - **Windows:** a named pipe whose only access rule is your account, made
//!   as the first instance of its name (so nobody else's pipe can stand in
//!   for it) and refusing clients on other computers.
//!
//! The address is random for each run. A connection starts with the helper's
//! hello line, carrying its AI tab's token; the app answers with one line
//! saying whether it took it, and then the connection is MCP.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// What the helper reads the channel's address and its tab's token from.
/// Set only in the environment of an AI tool started from Home, never on a
/// command line or in a file.
pub(crate) const ENV_ADDR: &str = "SUBMARINE_AI_ADDR";
pub(crate) const ENV_TOKEN: &str = "SUBMARINE_AI_TOKEN";

/// The helper's first line.
#[derive(Serialize, Deserialize)]
pub(crate) struct Hello {
    pub submarine_ai: u32,
    pub token: String,
}

/// The app's answer to it.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Welcome {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(crate) const PROTOCOL: u32 = 1;

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for T {}

pub(crate) type OnConnect = Arc<dyn Fn(Box<dyn Stream>) + Send + Sync>;

/// A line of at most `max` bytes, without its newline. None at the end of
/// the stream, or for a longer line.
pub(crate) async fn read_line<R: AsyncBufReadExt + Unpin>(r: &mut R, max: usize) -> std::io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = (&mut *r).take(max as u64 + 1).read_until(b'\n', &mut buf).await?;
    if n == 0 || (buf.len() > max && buf.last() != Some(&b'\n')) {
        return Ok(None);
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

pub(crate) fn random_hex(bytes: usize) -> String {
    use rand::Rng;
    let mut b = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

/// The listening end, while AI tools are on. Dropping it stops listening and
/// removes the socket.
pub(crate) struct Listener {
    addr: String,
    task: tokio::task::JoinHandle<()>,
    #[cfg(unix)]
    dir: std::path::PathBuf,
}

impl Listener {
    pub(crate) fn addr(&self) -> &str {
        &self.addr
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
        #[cfg(unix)]
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// macOS and Linux: a Unix socket
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn my_uid() -> u32 {
    // SAFETY: geteuid can't fail.
    unsafe { libc::geteuid() }
}

/// Where the socket's folder goes: the per-user runtime folder on Linux
/// (only you can open it), else the temp folder (per-user on macOS). Short,
/// as a socket's path has to be.
#[cfg(unix)]
fn socket_base() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// A new folder that only you can open, checked after it's made: yours, a
/// real folder (not a link), mode 0700.
#[cfg(unix)]
fn private_dir(base: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    for _ in 0..8 {
        let dir = base.join(format!("submarine-ai-{}", random_hex(8)));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
                let m = std::fs::symlink_metadata(&dir)?;
                if !m.is_dir() || m.uid() != my_uid() || m.mode() & 0o777 != 0o700 {
                    return Err(std::io::Error::other("the socket's folder isn't private"));
                }
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("couldn't make a folder for the socket"))
}

/// Whether a connection's process runs as you.
#[cfg(unix)]
pub(crate) fn peer_is_me(stream: &tokio::net::UnixStream) -> bool {
    stream.peer_cred().is_ok_and(|c| c.uid() == my_uid())
}

/// Listen on a new socket; each connection from a process running as you
/// goes to `on_connect`.
#[cfg(unix)]
pub(crate) fn listen(on_connect: OnConnect) -> std::io::Result<Listener> {
    let dir = private_dir(&socket_base())?;
    listen_in(dir, on_connect)
}

#[cfg(unix)]
fn listen_in(dir: std::path::PathBuf, on_connect: OnConnect) -> std::io::Result<Listener> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("s");
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    if peer_is_me(&stream) {
                        on_connect(Box::new(stream));
                    } else {
                        crate::applog::write_line("ai", "Refused a connection to the AI channel from another user's process.");
                    }
                }
                // Out of file handles, say: wait, rather than spin.
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
    });
    Ok(Listener { addr: path.to_string_lossy().into_owned(), task, dir })
}

#[cfg(unix)]
pub(crate) async fn connect(addr: &str) -> std::io::Result<Box<dyn Stream>> {
    Ok(Box::new(tokio::net::UnixStream::connect(addr).await?))
}

// ---------------------------------------------------------------------------
// Windows: a named pipe
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub(crate) mod pipe {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Your account's SID, as text (S-1-5-21-…).
    pub(crate) fn my_sid() -> std::io::Result<String> {
        // SAFETY: each handle and buffer below is made here, checked, and
        // freed once; TOKEN_USER is read from a buffer the size Windows asked
        // for, aligned for it (u64s).
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr() as *mut c_void, len, &mut len);
            let err = std::io::Error::last_os_error();
            CloseHandle(token);
            if ok == 0 {
                return Err(err);
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut text: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut n = 0;
            while *text.add(n) != 0 {
                n += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
            LocalFree(text as *mut c_void);
            Ok(sid)
        }
    }

    /// A security descriptor whose only rule is: your account, full access.
    /// Nobody else (other users, the system, administrators) may open the
    /// pipe or add an instance of it.
    pub(crate) struct OnlyMe(PSECURITY_DESCRIPTOR);
    // SAFETY: the descriptor is read-only once made, and freed only on drop.
    unsafe impl Send for OnlyMe {}
    unsafe impl Sync for OnlyMe {}

    impl OnlyMe {
        pub(crate) fn new() -> std::io::Result<OnlyMe> {
            let sddl = wide(&format!("D:P(A;;GA;;;{})", my_sid()?));
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // SAFETY: sddl is NUL-terminated; sd is freed in Drop.
            if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(OnlyMe(sd))
        }
    }

    impl Drop for OnlyMe {
        fn drop(&mut self) {
            // SAFETY: made by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe { LocalFree(self.0 as *mut c_void) };
        }
    }

    /// An instance of the pipe, refusing remote clients, with `sd` as its
    /// access rules. `first`: fail if the name is already taken.
    pub(crate) fn instance(name: &str, sd: &OnlyMe, first: bool) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        // SAFETY: sa and the descriptor it points to outlive the call.
        unsafe {
            tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(name, &mut sa as *mut SECURITY_ATTRIBUTES as *mut c_void)
        }
    }
}

#[cfg(windows)]
pub(crate) fn listen(on_connect: OnConnect) -> std::io::Result<Listener> {
    let name = format!(r"\\.\pipe\submarine-ai-{}", random_hex(16));
    let sd = pipe::OnlyMe::new()?;
    let mut server = pipe::instance(&name, &sd, true)?;
    let pipe_name = name.clone();
    let task = tokio::spawn(async move {
        loop {
            if server.connect().await.is_err() {
                match pipe::instance(&pipe_name, &sd, false) {
                    Ok(s) => server = s,
                    Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                }
                continue;
            }
            // The next client's instance first, so there's always one waiting.
            let next = loop {
                match pipe::instance(&pipe_name, &sd, false) {
                    Ok(s) => break s,
                    Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                }
            };
            on_connect(Box::new(std::mem::replace(&mut server, next)));
        }
    });
    Ok(Listener { addr: name, task })
}

#[cfg(windows)]
pub(crate) async fn connect(addr: &str) -> std::io::Result<Box<dyn Stream>> {
    const ERROR_PIPE_BUSY: i32 = 231;
    // Only Submarine's own pipes.
    if !addr.starts_with(r"\\.\pipe\submarine-ai-") {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a Submarine AI channel"));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(addr) {
            Ok(c) => return Ok(Box::new(c)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn listen(_on_connect: OnConnect) -> std::io::Result<Listener> {
    Err(std::io::Error::other("AI tools aren't available on this system"))
}

#[cfg(not(any(unix, windows)))]
pub(crate) async fn connect(_addr: &str) -> std::io::Result<Box<dyn Stream>> {
    Err(std::io::Error::other("AI tools aren't available on this system"))
}

/// The helper's side of the hello: send the token, read the answer.
pub(crate) async fn hello<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut tokio::io::BufReader<S>, token: &str) -> Result<(), String> {
    let mut line = serde_json::to_string(&Hello { submarine_ai: PROTOCOL, token: token.to_string() }).map_err(|e| e.to_string())?;
    line.push('\n');
    stream.get_mut().write_all(line.as_bytes()).await.map_err(|e| format!("Couldn't reach Submarine: {e}"))?;
    stream.get_mut().flush().await.map_err(|e| format!("Couldn't reach Submarine: {e}"))?;
    let answer = tokio::time::timeout(std::time::Duration::from_secs(10), read_line(stream, 4096))
        .await
        .map_err(|_| "Submarine didn't answer.".to_string())?
        .map_err(|e| format!("Couldn't reach Submarine: {e}"))?
        .ok_or("Submarine closed the connection.")?;
    let w: Welcome = serde_json::from_str(&answer).map_err(|_| "Submarine's answer wasn't understood.".to_string())?;
    if w.ok {
        Ok(())
    } else {
        Err(w.error.unwrap_or_else(|| "Submarine refused the connection.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A listener that counts the connections it's given, and answers each
    /// with a welcome after reading one line.
    fn counting() -> (OnConnect, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        let on: OnConnect = Arc::new(move |s| {
            seen.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut s = tokio::io::BufReader::new(s);
                if let Ok(Some(_)) = read_line(&mut s, 4096).await {
                    let _ = s.get_mut().write_all(b"{\"ok\":true}\n").await;
                }
            });
        });
        (on, n)
    }

    #[tokio::test]
    async fn a_connection_from_you_gets_through() {
        let (on, n) = counting();
        let l = listen(on).unwrap();
        let mut s = tokio::io::BufReader::new(connect(l.addr()).await.unwrap());
        hello(&mut s, "t").await.unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lines_are_capped() {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
            let mut r = tokio::io::BufReader::new(&b"short\r\nxxxxxxxxxx\n"[..]);
            assert_eq!(read_line(&mut r, 8).await.unwrap().as_deref(), Some("short"));
            assert_eq!(read_line(&mut r, 8).await.unwrap(), None, "longer than the cap");
        });
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_folder_is_yours_only() {
        use std::os::unix::fs::PermissionsExt;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let (on, _) = counting();
        let l = listen(on).unwrap();
        let sock = std::path::Path::new(l.addr());
        let dir = sock.parent().unwrap().to_path_buf();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(sock).unwrap().permissions().mode() & 0o077, 0);
        drop(l);
        assert!(!dir.exists(), "removed when it stops");
    }

    /// Run `nc -U addr` as another account (`nobody`): as root by switching
    /// to it, else through `sudo -n` (GitHub's runners allow it). None when
    /// neither is possible here.
    #[cfg(unix)]
    fn connect_as_nobody(addr: &str) -> Option<std::process::Child> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let nc = ["/usr/bin/nc", "/bin/nc"].into_iter().find(|p| std::path::Path::new(p).exists())?;
        let mut cmd = if my_uid() == 0 {
            let mut c = Command::new(nc);
            c.uid(65534).gid(65534);
            c
        } else {
            let ok = Command::new("sudo").args(["-n", "-u", "nobody", "true"]).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
            if !ok {
                return None;
            }
            let mut c = Command::new("sudo");
            c.args(["-n", "-u", "nobody", nc]);
            c
        };
        cmd.args(["-U", addr]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd.spawn().ok()
    }

    /// Another user's process can't connect: the folder keeps it out, and
    /// when the folder doesn't (opened up here on purpose), the check of who
    /// is connecting does.
    #[cfg(unix)]
    #[tokio::test]
    async fn another_user_cant_connect() {
        use std::os::unix::fs::PermissionsExt;
        for open_up in [false, true] {
            let (on, n) = counting();
            let dir = private_dir(&std::env::temp_dir()).unwrap();
            let l = listen_in(dir.clone(), on).unwrap();
            if open_up {
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
                std::fs::set_permissions(l.addr(), std::fs::Permissions::from_mode(0o777)).unwrap();
            }
            let Some(mut child) = connect_as_nobody(l.addr()) else {
                eprintln!("another_user_cant_connect: no other account to try here (needs root, or sudo -n)");
                return;
            };
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(b"{\"submarine_ai\":1,\"token\":\"x\"}\n");
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let _ = child.kill();
            let _ = child.wait();
            assert_eq!(n.load(Ordering::SeqCst), 0, "another user got through (folder opened up: {open_up})");
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn the_pipe_is_yours_only() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT};
        use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
        let (on, _) = counting();
        let l = listen(on).unwrap();
        let f = std::fs::OpenOptions::new().read(true).write(true).open(l.addr()).unwrap();
        // SAFETY: the handle is open; what Windows hands back is freed once.
        let sddl = unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let rc = GetSecurityInfo(
                f.as_raw_handle() as _,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            );
            assert_eq!(rc, 0, "GetSecurityInfo");
            let mut text: *mut u16 = std::ptr::null_mut();
            let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, SDDL_REVISION_1, DACL_SECURITY_INFORMATION, &mut text, std::ptr::null_mut());
            assert_ne!(ok, 0, "ConvertSecurityDescriptorToStringSecurityDescriptorW");
            let mut n = 0;
            while *text.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
            LocalFree(text as _);
            LocalFree(sd as _);
            s
        };
        let me = pipe::my_sid().unwrap();
        assert_eq!(sddl.matches("(A;").count(), 1, "one rule only: {sddl}");
        assert!(sddl.contains(&me), "the rule is for you: {sddl}");
        assert!(sddl.starts_with("D:P"), "nothing inherited: {sddl}");
    }

    /// Another account (the anonymous logon, impersonated by a thread of
    /// this process) can't open the pipe.
    #[cfg(windows)]
    #[tokio::test]
    async fn another_user_cant_open_the_pipe() {
        use windows_sys::Win32::Security::{ImpersonateAnonymousToken, RevertToSelf};
        use windows_sys::Win32::System::Threading::GetCurrentThread;
        let (on, n) = counting();
        let l = listen(on).unwrap();
        let addr = l.addr().to_string();
        let opened = std::thread::spawn(move || {
            // SAFETY: impersonation is this thread's only, and undone before
            // it ends.
            unsafe {
                assert_ne!(ImpersonateAnonymousToken(GetCurrentThread()), 0, "ImpersonateAnonymousToken");
                let r = std::fs::OpenOptions::new().read(true).write(true).open(&addr);
                RevertToSelf();
                r.map(|_| ()).map_err(|e| e.raw_os_error())
            }
        })
        .join()
        .unwrap();
        assert_eq!(opened, Err(Some(5)), "ERROR_ACCESS_DENIED");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }

    /// A second app (or anyone) can't take over a pipe name that's in use.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_pipe_name_cant_be_taken() {
        let (on, _) = counting();
        let l = listen(on).unwrap();
        let sd = pipe::OnlyMe::new().unwrap();
        assert!(pipe::instance(l.addr(), &sd, true).is_err());
    }
}
