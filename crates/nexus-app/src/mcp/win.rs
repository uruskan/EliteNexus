//! Windows: whose the agents' pipe is. Its name holds the user's SID, so no two accounts share one; the app creates it
//! with the user as owner and a DACL that lets only the user in; and `--mcp` opens it at identification level (the app
//! can't act as the user through it) and answers only through a pipe the user owns. Pipe names are machine-wide, and
//! the name is free whenever the app is closed or AI agents are off: another account could create it first.

use std::ffi::c_void;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{EqualSid, GetLengthSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// `SecurityIdentification << 16`, for `OpenOptionsExt::security_qos_flags`: the pipe's server may learn who connects,
/// not act as them.
pub const SECURITY_IDENTIFICATION: u32 = 1 << 16;

/// The logged-in user's SID.
pub struct UserSid {
    /// Its bytes, 8-byte aligned as Windows wants a SID.
    raw: Vec<u64>,
    /// As text: "S-1-5-21-…".
    pub text: String,
}

impl UserSid {
    fn as_psid(&self) -> PSID {
        self.raw.as_ptr() as PSID
    }
}

/// The SID this process runs as. None only if Windows won't say, which it always does.
pub fn user_sid() -> Option<UserSid> {
    // SAFETY: plain Win32 calls on this process's own token; every buffer is sized as Windows asks, the token handle is
    // closed and the string Windows allocates is freed.
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        let n = GetLengthSid(sid) as usize;
        let mut raw = vec![0u64; n.div_ceil(8)];
        std::ptr::copy_nonoverlapping(sid as *const u8, raw.as_mut_ptr().cast::<u8>(), n);
        let mut text_ptr: *mut u16 = null_mut();
        if ConvertSidToStringSidW(sid, &mut text_ptr) == 0 || text_ptr.is_null() {
            return None;
        }
        let mut end = 0;
        while *text_ptr.add(end) != 0 {
            end += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(text_ptr, end));
        LocalFree(text_ptr.cast());
        Some(UserSid { raw, text })
    }
}

/// A security descriptor for the pipe: the user as owner, and a protected DACL that lets only the user in (no read
/// access for Everyone or Anonymous, as a pipe's default one gives).
pub struct OwnerOnly(*mut c_void);

// SAFETY: the descriptor is written once, here, and only read after; LocalFree frees it once, on drop.
unsafe impl Send for OwnerOnly {}
unsafe impl Sync for OwnerOnly {}

impl OwnerOnly {
    pub fn new(user: &UserSid) -> Option<Self> {
        let sddl: Vec<u16> = format!("O:{0}D:P(A;;GA;;;{0})", user.text).encode_utf16().chain([0]).collect();
        let mut sd: *mut c_void = null_mut();
        // SAFETY: a NUL-terminated SDDL string; Windows allocates the descriptor, freed on drop.
        let ok = unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) };
        (ok != 0 && !sd.is_null()).then_some(OwnerOnly(sd))
    }

    /// `SECURITY_ATTRIBUTES` for creating a pipe instance; valid while `self` lives.
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.0, bInheritHandle: 0 }
    }
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed once.
        unsafe { LocalFree(self.0) };
    }
}

/// Whether the object behind `handle` (the pipe `--mcp` opened) is owned by `user`.
pub fn owned_by(handle: HANDLE, user: &UserSid) -> bool {
    let mut owner: PSID = null_mut();
    let mut sd: *mut c_void = null_mut();
    // SAFETY: GetSecurityInfo fills `owner` with a pointer into `sd`, which Windows allocates and LocalFree frees.
    unsafe {
        if GetSecurityInfo(handle, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, null_mut(), null_mut(), null_mut(), &mut sd) != 0 {
            return false;
        }
        let same = !owner.is_null() && EqualSid(owner, user.as_psid()) != 0;
        LocalFree(sd);
        same
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;

    #[test]
    fn the_user_owns_their_files_and_not_the_systems() {
        let me = user_sid().expect("this process's SID");
        assert!(me.text.starts_with("S-1-"), "{}", me.text);
        assert!(OwnerOnly::new(&me).is_some());
        let path = std::env::temp_dir().join(format!("enx-owner-{}.txt", std::process::id()));
        let mine = std::fs::File::create(&path).unwrap();
        assert!(owned_by(mine.as_raw_handle(), &me), "a file this test made is the user's");
        drop(mine);
        let _ = std::fs::remove_file(&path);
        let system = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("System32").join("kernel32.dll");
        let theirs = std::fs::File::open(system).unwrap();
        assert!(!owned_by(theirs.as_raw_handle(), &me), "Windows' own files aren't the user's");
    }
}
