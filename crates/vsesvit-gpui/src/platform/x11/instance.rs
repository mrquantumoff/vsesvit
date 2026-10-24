//! Owned by W1.1.
//! The single-instance socket's transport (UA section 1.2): a Unix socket in a 0700 directory that
//! only peers of our own uid may use (`SO_PEERCRED`). The protocol over it (one JSON request line,
//! stderr lines, a status) is `boot/instance.rs`'s.

use std::ffi::c_void;
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// `$XDG_RUNTIME_DIR/vsesvit/<key>.sock`, or `<root>/instance.sock` without a runtime directory.
/// `key` names the profile root (UA: the first 16 hex digits of its canonical path's SHA-256).
pub fn socket_path(runtime_dir: Option<&Path>, key: &str, root: &Path) -> PathBuf {
    match runtime_dir {
        Some(dir) => dir.join("vsesvit").join(format!("{key}.sock")),
        None => root.join("instance.sock"),
    }
}

/// The primary's end. Only the profile lock's holder binds, so a socket file already at the path
/// is a dead primary's and is replaced. Removed again on drop.
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
}

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Listener> {
        // A missing directory is created 0700; an existing one (the profile root) is left alone.
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        Ok(Listener { listener: UnixListener::bind(path)?, path: path.to_owned() })
    }

    /// The next connection from our own uid. Others are closed unanswered.
    pub fn accept(&self) -> io::Result<UnixStream> {
        loop {
            let (stream, _) = self.listener.accept()?;
            match peer_uid(&stream) {
                Ok(uid) if uid == my_uid() => return Ok(stream),
                Ok(uid) => log::warn!("instance: refused a connection from uid {uid}"),
                Err(e) => log::warn!("instance: refused a connection with no credentials: {e}"),
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A second process's end. The caller checks the server's reply.
pub fn connect(path: &Path) -> io::Result<UnixStream> {
    let stream = UnixStream::connect(path)?;
    match peer_uid(&stream)? {
        uid if uid == my_uid() => Ok(stream),
        uid => Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("the socket belongs to uid {uid}"))),
    }
}

#[repr(C)]
struct UCred {
    pid: i32,
    uid: u32,
    gid: u32,
}

unsafe extern "C" {
    fn getsockopt(fd: i32, level: i32, name: i32, value: *mut c_void, len: *mut u32) -> i32;
    fn getuid() -> u32;
}

const SOL_SOCKET: i32 = 1;
/// x86, x86-64 and arm64; the targets vsesvit builds for.
const SO_PEERCRED: i32 = 17;

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut cred = UCred { pid: 0, uid: u32::MAX, gid: 0 };
    let mut len = std::mem::size_of::<UCred>() as u32;
    // SAFETY: `cred` and `len` are valid for writes of the sizes passed, for the call's duration.
    let rc = unsafe { getsockopt(stream.as_raw_fd(), SOL_SOCKET, SO_PEERCRED, (&raw mut cred).cast(), &mut len) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(cred.uid)
}

fn my_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { getuid() }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn paths_prefer_the_runtime_directory() {
        let root = Path::new("/home/u/.local/share/vsesvit/default");
        assert_eq!(socket_path(Some(Path::new("/run/user/1000")), "0123abcd", root), Path::new("/run/user/1000/vsesvit/0123abcd.sock"));
        assert_eq!(socket_path(None, "0123abcd", root), root.join("instance.sock"));
    }

    #[test]
    fn a_same_user_client_reaches_the_listener_and_the_file_goes_with_it() {
        let dir = std::env::temp_dir().join(format!("vsesvit-instance-test-{}", std::process::id()));
        let path = dir.join("vsesvit").join("k.sock");
        let listener = Listener::bind(&path).expect("bind");
        let mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let server = std::thread::spawn(move || {
            let stream = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream).write_all(b"{\"status\": 0}\n").unwrap();
            (line, listener)
        });
        let mut client = connect(&path).expect("connect");
        client.write_all(b"{\"argv\": []}\n").unwrap();
        let mut reply = String::new();
        BufReader::new(&client).read_line(&mut reply).unwrap();
        let (line, listener) = server.join().unwrap();
        assert_eq!(line, "{\"argv\": []}\n");
        assert_eq!(reply, "{\"status\": 0}\n");
        drop(listener);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_primarys_socket_is_replaced() {
        let dir = std::env::temp_dir().join(format!("vsesvit-instance-stale-{}", std::process::id()));
        let path = dir.join("s.sock");
        std::fs::create_dir_all(&dir).unwrap();
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let listener = Listener::bind(&path).expect("rebind over a stale socket");
        assert!(connect(&path).is_ok());
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
