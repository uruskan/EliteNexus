//! Linux and macOS: whose the agents' socket is. It lives in a folder only the user can enter, found without the
//! environment an MCP client may not pass on, and both ends check that the other runs as the same user.

use std::os::unix::io::RawFd;
use std::path::PathBuf;

/// The user this process runs as.
pub fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and can't fail.
    unsafe { libc::geteuid() }
}

/// The folder for the socket: /run/user/<uid>, which the system makes for each logged-in user, or else ~/.elite-nexus,
/// readable by the user only. Not $XDG_RUNTIME_DIR or $TMPDIR: an MCP client may start `--mcp` with only HOME, LOGNAME,
/// PATH, SHELL, TERM and USER, and the two ends would look in different places.
pub fn socket_dir() -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let uid = euid();
    let run = PathBuf::from(format!("/run/user/{uid}"));
    if std::fs::metadata(&run).is_ok_and(|m| m.is_dir() && m.uid() == uid) {
        return Ok(run);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_absolute()).ok_or("HOME isn't set: there's no folder of the user's for the socket.")?;
    let dir = home.join(".elite-nexus");
    let fail = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).map_err(fail)?;
    let m = std::fs::metadata(&dir).map_err(fail)?;
    if m.uid() != uid {
        return Err(format!("{} belongs to another user.", dir.display()));
    }
    if m.mode() & 0o077 != 0 {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(fail)?;
    }
    Ok(dir)
}

/// The user the other end of a Unix socket runs as.
#[cfg(target_os = "linux")]
pub fn peer_uid(fd: RawFd) -> std::io::Result<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for getsockopt to fill.
    let r = unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut cred).cast(), &mut len) };
    if r == 0 { Ok(cred.uid) } else { Err(std::io::Error::last_os_error()) }
}

/// The user the other end of a Unix socket runs as (macOS).
#[cfg(not(target_os = "linux"))]
pub fn peer_uid(fd: RawFd) -> std::io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: `uid` and `gid` are valid for getpeereid to fill.
    let r = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    if r == 0 { Ok(uid) } else { Err(std::io::Error::last_os_error()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::io::AsRawFd;

    #[test]
    fn both_ends_are_this_user() {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_eq!(peer_uid(a.as_raw_fd()).unwrap(), euid());
        assert_eq!(peer_uid(b.as_raw_fd()).unwrap(), euid());
    }

    #[test]
    fn the_socket_folder_is_the_users_alone() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = socket_dir().unwrap();
        let m = std::fs::metadata(&dir).unwrap();
        assert!(m.is_dir() && m.uid() == euid() && m.permissions().mode() & 0o077 == 0, "{}", dir.display());
    }
}
