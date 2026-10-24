//! Owned by W1.2.
//! The transport of the single-instance channel on Windows (design section 1.4, UA §1.2): a named pipe
//! per profile, `\\.\pipe\vsesvit-<hash>`, that only this user can open. W1.4's `boot/instance.rs`
//! speaks the protocol over it. The owner binds at boot step 7 and accepts on its own thread; a later
//! process connects, which also grants the owner the right to take the foreground for the raise.

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::Path;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_ALL, GENERIC_READ,
    GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, EqualSid, GetLengthSid, GetTokenInformation,
    InitializeAcl, InitializeSecurityDescriptor, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
    SetSecurityDescriptorDacl, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use super::wide;

const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
const BUFFER: u32 = 64 * 1024;

/// The pipe of the profile at `profile_root`. Case-insensitive in the path, like the file system;
/// FNV-1a, so stable across processes and builds.
pub fn pipe_name(profile_root: &Path) -> String {
    let path = profile_root.to_string_lossy().to_lowercase();
    let path = path.trim_end_matches(['\\', '/']);
    let hash = path.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!(r"\\.\pipe\vsesvit-{hash:016x}")
}

/// One end of a connection, and the process at the other end.
pub struct Peer {
    pub stream: File,
    pub pid: u32,
}

/// The owner's end: always one pipe instance waiting for the next client.
pub struct Listener {
    name: Vec<u16>,
    waiting: OwnedHandle,
}

impl Listener {
    /// Binds `name`. Fails when any process already holds it, ours included.
    pub fn bind(name: &str) -> io::Result<Listener> {
        let name = wide(name);
        let waiting = instance(&name, true)?;
        Ok(Listener { name, waiting })
    }

    /// Waits for the next client. Blocking: the instance accept thread runs it.
    pub fn accept(&mut self) -> io::Result<Peer> {
        let pipe = self.waiting.as_raw_handle() as HANDLE;
        loop {
            if unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) } != 0 {
                break;
            }
            match unsafe { GetLastError() } {
                ERROR_PIPE_CONNECTED => break,
                // A client that connected and closed before this call: free the instance, wait again.
                ERROR_NO_DATA => {
                    unsafe { DisconnectNamedPipe(pipe) };
                }
                error => return Err(io::Error::from_raw_os_error(error as i32)),
            }
        }
        let connected = std::mem::replace(&mut self.waiting, instance(&self.name, false)?);
        let mut pid = 0;
        if unsafe { GetNamedPipeClientProcessId(connected.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Peer { stream: File::from(connected), pid })
    }
}

/// Connects to the owner of `name`, retrying until `timeout` while it is busy or not bound yet. The
/// owner must run as this user. It may then take the foreground once, to raise its window.
pub fn connect(name: &str, timeout: Duration) -> io::Result<Peer> {
    let wide_name = wide(name);
    let deadline = Instant::now() + timeout;
    let handle = loop {
        let handle = unsafe {
            CreateFileW(wide_name.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut())
        };
        if handle != INVALID_HANDLE_VALUE {
            break unsafe { OwnedHandle::from_raw_handle(handle as _) };
        }
        let error = unsafe { GetLastError() };
        let left = deadline.saturating_duration_since(Instant::now());
        match error {
            _ if left.is_zero() => return Err(io::Error::from_raw_os_error(error as i32)),
            ERROR_PIPE_BUSY => {
                unsafe { WaitNamedPipeW(wide_name.as_ptr(), left.as_millis().min(u32::MAX as u128) as u32) };
            }
            ERROR_FILE_NOT_FOUND => std::thread::sleep(left.min(Duration::from_millis(50))),
            _ => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    };
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(handle.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !same_user(pid)? {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{name} is held by another user's process")));
    }
    unsafe { AllowSetForegroundWindow(pid) };
    Ok(Peer { stream: File::from(handle), pid })
}

fn instance(name: &[u16], first: bool) -> io::Result<OwnedHandle> {
    let security = OwnerOnly::new()?;
    let open_mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &security.attributes,
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(pipe as _) })
}

/// The SID bytes of the user a process runs as.
fn user_of(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token as _) };
    let mut len = 0u32;
    unsafe { GetTokenInformation(token.as_raw_handle() as HANDLE, TokenUser, std::ptr::null_mut(), 0, &mut len) };
    // u64 storage keeps TOKEN_USER and the SID after it aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    if unsafe { GetTokenInformation(token.as_raw_handle() as HANDLE, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid };
    let sid_len = unsafe { GetLengthSid(sid) } as usize;
    Ok(unsafe { std::slice::from_raw_parts(sid as *const u8, sid_len) }.to_vec())
}

fn same_user(pid: u32) -> io::Result<bool> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let theirs = user_of(process);
    unsafe { CloseHandle(process) };
    let (mut theirs, mut ours) = (theirs?, user_of(unsafe { GetCurrentProcess() })?);
    Ok(unsafe { EqualSid(theirs.as_mut_ptr().cast(), ours.as_mut_ptr().cast()) } != 0)
}

/// Security attributes whose DACL lets only this process's user in. A pipe's default DACL also gives
/// Everyone read access.
struct OwnerOnly {
    attributes: SECURITY_ATTRIBUTES,
    _descriptor: Box<SECURITY_DESCRIPTOR>,
    _acl: Vec<u64>,
    _sid: Vec<u8>,
}

impl OwnerOnly {
    fn new() -> io::Result<OwnerOnly> {
        let mut sid = user_of(unsafe { GetCurrentProcess() })?;
        let acl_len = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() + sid.len();
        let mut acl = vec![0u64; acl_len.div_ceil(8)];
        let acl_ptr = acl.as_mut_ptr() as *mut ACL;
        let mut descriptor: Box<SECURITY_DESCRIPTOR> = Box::new(unsafe { std::mem::zeroed() });
        let descriptor_ptr = &mut *descriptor as *mut SECURITY_DESCRIPTOR as PSECURITY_DESCRIPTOR;
        unsafe {
            if InitializeAcl(acl_ptr, acl_len as u32, ACL_REVISION) == 0
                || AddAccessAllowedAce(acl_ptr, ACL_REVISION, GENERIC_ALL, sid.as_mut_ptr() as *mut c_void) == 0
                || InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) == 0
                || SetSecurityDescriptorDacl(descriptor_ptr, 1, acl_ptr, 0) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor_ptr,
            bInheritHandle: 0,
        };
        Ok(OwnerOnly { attributes, _descriptor: descriptor, _acl: acl, _sid: sid })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::path::Path;
    use std::time::Duration;

    use super::{Listener, connect, pipe_name};

    fn unique() -> String {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        format!(r"\\.\pipe\vsesvit-test-{}-{nanos}", std::process::id())
    }

    #[test]
    fn one_pipe_per_profile_whatever_the_spelling() {
        let a = pipe_name(Path::new(r"C:\Users\Me\Vsesvit\Profiles\Default"));
        assert_eq!(a, pipe_name(Path::new(r"c:\users\me\vsesvit\profiles\default\")));
        assert_ne!(a, pipe_name(Path::new(r"C:\Users\Me\Vsesvit\Profiles\Work")));
        assert!(a.starts_with(r"\\.\pipe\vsesvit-"));
    }

    #[test]
    fn a_client_reaches_the_owner_and_both_know_each_other() {
        let name = unique();
        let mut listener = Listener::bind(&name).unwrap();
        let owner = std::thread::spawn(move || {
            let peer = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&peer.stream).read_line(&mut line).unwrap();
            (peer.pid, line, listener)
        });
        let mut client = connect(&name, Duration::from_secs(2)).unwrap();
        client.stream.write_all(b"{\"argv\":[]}\n").unwrap();
        let (pid, line, mut listener) = owner.join().unwrap();
        assert_eq!(pid, std::process::id());
        assert_eq!(client.pid, std::process::id());
        assert_eq!(line, "{\"argv\":[]}\n");
        // The next client finds a fresh instance waiting, also after one that left before it was seen.
        drop(connect(&name, Duration::from_secs(2)).unwrap());
        let second = std::thread::spawn(move || listener.accept().map(|p| p.pid));
        let _held = connect(&name, Duration::from_secs(2)).unwrap();
        assert_eq!(second.join().unwrap().unwrap(), std::process::id());
    }

    #[test]
    fn a_held_name_cannot_be_bound_again() {
        let name = unique();
        let _owner = Listener::bind(&name).unwrap();
        assert!(Listener::bind(&name).is_err());
    }

    #[test]
    fn nobody_listening_times_out() {
        let started = std::time::Instant::now();
        assert!(connect(&unique(), Duration::from_millis(200)).is_err());
        assert!(started.elapsed() >= Duration::from_millis(200));
    }
}
