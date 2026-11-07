//! Submarine: a path the server sends is reached below the shared folder one
//! name at a time, never through a link (#132).
//!
//! The backend used to open, list, rename and delete by the whole path as
//! text, and the system follows a link anywhere in it. Submarine checks each
//! path for links before it gets here (rdp_share.rs, `inside`), but something
//! else writing in the shared folder at the same moment (a network mount of
//! the same server, a synced folder) could swap a folder for a link between
//! that check and the open. Here each folder on the way is opened relative to
//! the one before with `O_NOFOLLOW`, and the last name is reached from its
//! folder with the `*at` calls, so a link anywhere on the way fails the call
//! instead of being followed. This works the same on Linux and macOS.
//!
//! The shared folder itself (`base`) is the one the user chose, opened as it
//! is.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsFd, OwnedFd};

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{FileStat, Mode, SFlag};
use nix::unistd::UnlinkatFlags;

fn refused() -> io::Error {
    io::Error::from_raw_os_error(nix::libc::EACCES)
}

/// The names below `base` in `full` (`base` + `/name/name…`). None of them
/// may be empty, `.` or `..`: a path that leads out isn't walked at all.
fn names(base: &str, full: &str) -> io::Result<Vec<CString>> {
    let rest = full.strip_prefix(base).ok_or_else(refused)?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    rest.split('/')
        .map(|n| {
            if n.is_empty() || n == "." || n == ".." {
                return Err(refused());
            }
            CString::new(n).map_err(|_| refused())
        })
        .collect()
}

fn open_base(base: &str) -> io::Result<OwnedFd> {
    Ok(nix::fcntl::open(base, OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_CLOEXEC, Mode::empty())?)
}

fn open_folder(dir: impl AsFd, name: &CString) -> io::Result<OwnedFd> {
    Ok(nix::fcntl::openat(
        dir,
        name.as_c_str(),
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )?)
}

/// The folder holding `full`'s last name, and that name (None for `base`
/// itself), reached from `base` without following a link.
fn parent(base: &str, full: &str) -> io::Result<(OwnedFd, Option<CString>)> {
    let mut parts = names(base, full)?;
    let mut dir = open_base(base)?;
    let Some(last) = parts.pop() else { return Ok((dir, None)) };
    for name in &parts {
        dir = open_folder(&dir, name)?;
    }
    Ok((dir, Some(last)))
}

/// Opens `full` with `flags` (plus `O_NOFOLLOW`, `O_NONBLOCK` and
/// `O_CLOEXEC`): a file or a folder only. Anything else there (a FIFO, a
/// device) is refused, without waiting on it: `O_NONBLOCK` makes no
/// difference to a file or a folder.
pub fn open(base: &str, full: &str, flags: OFlag, mode: u32) -> io::Result<File> {
    let (dir, name) = parent(base, full)?;
    let fd = match name {
        None => dir,
        Some(name) => nix::fcntl::openat(
            &dir,
            name.as_c_str(),
            flags | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(mode as nix::libc::mode_t),
        )?,
    };
    let kind = SFlag::from_bits_truncate(nix::sys::stat::fstat(&fd)?.st_mode & SFlag::S_IFMT.bits());
    if kind != SFlag::S_IFREG && kind != SFlag::S_IFDIR {
        return Err(refused());
    }
    Ok(File::from(fd))
}

/// A folder of `full` to list.
pub fn open_dir(base: &str, full: &str) -> io::Result<Dir> {
    let file = open(base, full, OFlag::O_DIRECTORY | OFlag::O_RDONLY, 0)?;
    Ok(Dir::from_fd(OwnedFd::from(file))?)
}

/// Makes `full` and any folder on the way that isn't there yet.
pub fn create_dir_all(base: &str, full: &str) -> io::Result<()> {
    let mut dir = open_base(base)?;
    for name in names(base, full)? {
        dir = match open_folder(&dir, &name) {
            Ok(next) => next,
            Err(e) if e.raw_os_error() == Some(nix::libc::ENOENT) => {
                nix::sys::stat::mkdirat(&dir, name.as_c_str(), Mode::from_bits_truncate(0o777))?;
                open_folder(&dir, &name)?
            }
            Err(e) => return Err(e),
        };
    }
    Ok(())
}

/// What the backend tells the server about a file or folder.
#[derive(Debug, Clone, Copy)]
pub struct Details {
    pub is_dir: bool,
    pub readonly: bool,
    pub size: u64,
    pub ctime: i64,
    pub atime: i64,
    pub mtime: i64,
}

impl From<FileStat> for Details {
    fn from(st: FileStat) -> Self {
        let kind = SFlag::from_bits_truncate(st.st_mode & SFlag::S_IFMT.bits());
        Self {
            is_dir: kind == SFlag::S_IFDIR,
            readonly: st.st_mode & 0o222 == 0,
            size: u64::try_from(st.st_size).unwrap_or(0),
            ctime: st.st_ctime,
            atime: st.st_atime,
            mtime: st.st_mtime,
        }
    }
}

impl From<&std::fs::Metadata> for Details {
    fn from(meta: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            is_dir: meta.is_dir(),
            readonly: meta.permissions().readonly(),
            size: meta.size(),
            ctime: meta.ctime(),
            atime: meta.atime(),
            mtime: meta.mtime(),
        }
    }
}

/// `full`'s own details: a link's, not those of what it points to.
pub fn details(base: &str, full: &str) -> io::Result<Details> {
    let (dir, name) = parent(base, full)?;
    let st = match name {
        None => nix::sys::stat::fstat(&dir)?,
        Some(name) => nix::sys::stat::fstatat(&dir, name.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)?,
    };
    Ok(st.into())
}

/// Renames `from` to `to`, both below `base`.
pub fn rename(base: &str, from: &str, to: &str) -> io::Result<()> {
    let (from_dir, from_name) = parent(base, from)?;
    let (to_dir, to_name) = parent(base, to)?;
    let (Some(from_name), Some(to_name)) = (from_name, to_name) else { return Err(refused()) };
    Ok(nix::fcntl::renameat(&from_dir, from_name.as_c_str(), &to_dir, to_name.as_c_str())?)
}

/// Deletes the file `full` (a link, as itself).
pub fn remove_file(base: &str, full: &str) -> io::Result<()> {
    let (dir, name) = parent(base, full)?;
    let name = name.ok_or_else(refused)?;
    Ok(nix::unistd::unlinkat(&dir, name.as_c_str(), UnlinkatFlags::NoRemoveDir)?)
}
