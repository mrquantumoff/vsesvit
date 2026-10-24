//! Owned by W1.4.
//! The single-instance channel (UA section 1.2; design section 3.1 steps 5 and 6). One process holds a
//! profile; a later launch of the same profile forwards its command line to it and exits with the
//! status it answers. The transport is the platform's: a Unix socket that only our uid may use
//! (`platform::x11::instance`), or a named pipe that only this user may open
//! (`platform::windows::instance`). This file is the protocol over it:
//!
//! - the client sends one JSON line, `{"argv": [...], "cwd": "...", "startup_id": "..." | null}`;
//! - the owner answers with zero or more `{"stderr": "..."}` lines, then `{"status": N}`.
//!
//! The owner binds at boot step 6, before the engine starts, and accepts on its own thread only from
//! [`Listener::spawn_accept`], after `cef::initialize` (no extra thread before step 9).

use std::io::{self, BufRead as _, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use serde::{Deserialize, Serialize};

#[cfg(windows)]
use crate::platform::windows::instance as transport;
#[cfg(target_os = "linux")]
use crate::platform::x11::instance as transport;

/// What a forwarding launch prints, and exits 1 with, when the owner does not answer.
pub const UNREACHABLE: &str = "vsesvit: the profile is in use by another Vsesvit process that could not be reached";
/// How long a launch waits for the owner to take its connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a launch waits for the owner's status once its request is sent.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a new owner retries binding while the previous owner of the name is still exiting: a
/// relaunch starts its successor just before the old process ends.
const BIND_PATIENCE: Duration = Duration::from_secs(2);

/// A launch's command line, as the owner receives it. The arguments already parsed in the
/// launching process, so they are Unicode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    #[serde(rename = "argv")]
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The launch's `DESKTOP_STARTUP_ID` or `XDG_ACTIVATION_TOKEN`, for raising the window.
    pub startup_id: Option<String>,
}

impl Request {
    /// This process's own launch: `args` after the program name, the working directory and the
    /// startup id from the environment.
    pub fn of_this_process(args: Vec<String>) -> Request {
        let startup_id = ["DESKTOP_STARTUP_ID", "XDG_ACTIVATION_TOKEN"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()));
        Request { args, cwd: std::env::current_dir().unwrap_or_default(), startup_id }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Answer {
    Stderr(String),
    Status(u8),
}

/// Where a profile's owner listens: the socket path or the pipe name of its canonical root, so
/// every spelling of the path names one endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(#[cfg(target_os = "linux")] PathBuf, #[cfg(windows)] String);

impl Endpoint {
    pub fn of_profile(root: &Path) -> Endpoint {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
        #[cfg(target_os = "linux")]
        {
            let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute());
            Endpoint(transport::socket_path(runtime_dir.as_deref(), &key(&root), &root))
        }
        #[cfg(windows)]
        Endpoint(transport::pipe_name(&root))
    }
}

/// The first 16 hex digits of the root's FNV-1a hash: stable across processes and builds.
#[cfg(target_os = "linux")]
fn key(root: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    let hash = root.as_os_str().as_bytes().iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

/// Why a launch could not hand its command line over.
#[derive(Debug)]
pub struct Unreachable(pub String);

impl std::fmt::Display for Unreachable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Boot step 5, `Locked`: hands `request` to the profile's owner, copies its stderr lines to
/// `stderr`, and returns its status.
pub fn forward(endpoint: &Endpoint, request: &Request, stderr: &mut dyn Write) -> Result<u8, Unreachable> {
    forward_within(endpoint, request, stderr, CONNECT_TIMEOUT)
}

fn forward_within(endpoint: &Endpoint, request: &Request, stderr: &mut dyn Write, timeout: Duration) -> Result<u8, Unreachable> {
    let failed = |what: &str, e: &dyn std::fmt::Display| Unreachable(format!("{what}: {e}"));
    let (mut writer, reader) = connect(endpoint, timeout).map_err(|e| failed("connecting", &e))?;
    let mut line = serde_json::to_string(request).map_err(|e| failed("encoding the request", &e))?;
    line.push('\n');
    writer.write_all(line.as_bytes()).and_then(|()| writer.flush()).map_err(|e| failed("sending the request", &e))?;
    // The answer is read on its own thread, so an owner that took the connection but never answers
    // cannot hang the launch (a synchronous pipe read has no timeout). This process never starts the
    // engine, so the extra thread costs nothing.
    let (lines, answers) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("instance-answer".into())
        .spawn(move || {
            for line in BufReader::new(reader).lines() {
                if lines.send(line).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| failed("reading the answer", &e))?;
    loop {
        let line = match answers.recv_timeout(ANSWER_TIMEOUT) {
            Ok(line) => line.map_err(|e| failed("reading the answer", &e))?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Err(Unreachable(format!("no answer within {ANSWER_TIMEOUT:?}"))),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match serde_json::from_str::<Answer>(&line) {
            Ok(Answer::Stderr(text)) => {
                let _ = writeln!(stderr, "{text}");
            }
            Ok(Answer::Status(status)) => return Ok(status),
            Err(e) => return Err(failed("an answer that does not parse", &e)),
        }
    }
    Err(Unreachable("the owner closed the connection without a status".into()))
}

type Halves = (Box<dyn Write>, Box<dyn Read + Send>);

#[cfg(target_os = "linux")]
fn connect(endpoint: &Endpoint, timeout: Duration) -> io::Result<Halves> {
    let deadline = Instant::now() + timeout;
    loop {
        match transport::connect(&endpoint.0) {
            Ok(stream) => return Ok((Box::new(stream.try_clone()?), Box::new(stream))),
            Err(e) if Instant::now() < deadline && matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(windows)]
fn connect(endpoint: &Endpoint, timeout: Duration) -> io::Result<Halves> {
    let peer = transport::connect(&endpoint.0, timeout)?;
    Ok((Box::new(peer.stream.try_clone()?), Box::new(peer.stream)))
}

/// The owner's end, bound at boot step 6. Accepting starts later, with [`Listener::spawn_accept`].
pub struct Listener {
    inner: transport::Listener,
}

impl Listener {
    /// Binds `endpoint`, waiting up to two seconds while a previous owner that is exiting still
    /// holds it. Our caller holds the profile's lock, so any other holder is on its way out.
    pub fn bind(endpoint: &Endpoint) -> io::Result<Listener> {
        let deadline = Instant::now() + BIND_PATIENCE;
        loop {
            match transport::Listener::bind(&endpoint.0) {
                Ok(inner) => return Ok(Listener { inner }),
                Err(e) if Instant::now() < deadline => {
                    log::debug!("instance: binding {endpoint:?}: {e}; retrying");
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Starts the accept thread (after `cef::initialize`). Each launch's request arrives on the
    /// receiver with the [`Reply`] that answers it. A connection whose request does not parse is
    /// closed unanswered.
    pub fn spawn_accept(self) -> io::Result<UnboundedReceiver<Invocation>> {
        let (sender, receiver) = unbounded();
        #[cfg_attr(target_os = "linux", allow(unused_mut))]
        let mut inner = self.inner;
        std::thread::Builder::new().name("instance-accept".into()).spawn(move || {
            loop {
                let stream = match accept(&mut inner) {
                    Ok(stream) => stream,
                    Err(e) => {
                        log::error!("instance: accept failed, later launches cannot reach this process: {e}");
                        return;
                    }
                };
                let sender = sender.clone();
                // One thread per connection, so a client that never sends cannot block the next one.
                let reader = std::thread::Builder::new().name("instance-request".into()).spawn(move || {
                    if let Some(invocation) = read_request(stream) {
                        let _ = sender.unbounded_send(invocation);
                    }
                });
                if let Err(e) = reader {
                    log::error!("instance: no thread for a connection: {e}");
                }
            }
        })?;
        Ok(receiver)
    }
}

type Stream = Box<dyn ReadWrite + Send>;

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

#[cfg(target_os = "linux")]
fn accept(listener: &mut transport::Listener) -> io::Result<Stream> {
    Ok(Box::new(listener.accept()?))
}

#[cfg(windows)]
fn accept(listener: &mut transport::Listener) -> io::Result<Stream> {
    Ok(Box::new(listener.accept()?.stream))
}

fn read_request(stream: Stream) -> Option<Invocation> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if let Err(e) = reader.read_line(&mut line) {
        log::warn!("instance: reading a request: {e}");
        return None;
    }
    match serde_json::from_str::<Request>(&line) {
        Ok(request) => Some(Invocation { request, reply: Reply { stream: Some(reader.into_inner()) } }),
        Err(e) => {
            log::warn!("instance: a request that does not parse: {e}");
            None
        }
    }
}

/// A forwarded launch, and the answer it waits for.
pub struct Invocation {
    pub request: Request,
    pub reply: Reply,
}

/// The answer to one forwarded launch: stderr lines, then one status. Dropped without
/// [`Reply::finish`], it answers status 1.
pub struct Reply {
    stream: Option<Stream>,
}

impl Reply {
    /// A line the launching process prints on its stderr.
    pub fn stderr(&mut self, text: &str) {
        self.send(&Answer::Stderr(text.to_owned()));
    }

    /// The launching process's exit status; ends the answer.
    pub fn finish(mut self, status: u8) {
        self.send(&Answer::Status(status));
        self.stream = None;
    }

    fn send(&mut self, answer: &Answer) {
        let Some(stream) = self.stream.as_mut() else { return };
        let mut line = serde_json::to_string(answer).expect("answers encode");
        line.push('\n');
        if let Err(e) = stream.write_all(line.as_bytes()).and_then(|()| stream.flush()) {
            log::warn!("instance: the launching process went away: {e}");
            self.stream = None;
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if self.stream.is_some() {
            self.send(&Answer::Stderr("vsesvit: the running Vsesvit did not handle this launch".into()));
            self.send(&Answer::Status(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;

    use super::*;

    fn endpoint(name: &str) -> (Endpoint, Option<PathBuf>) {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let unique = format!("{name}-{}-{nanos}", std::process::id());
        #[cfg(target_os = "linux")]
        {
            let dir = std::env::temp_dir().join(format!("vsesvit-boot-instance-{unique}"));
            (Endpoint(dir.join("vsesvit").join("p.sock")), Some(dir))
        }
        #[cfg(windows)]
        (Endpoint(format!(r"\\.\pipe\vsesvit-boot-test-{unique}")), None)
    }

    fn request() -> Request {
        Request {
            args: vec!["https://example.com/ü".into(), "--profile-dir".into(), "/p".into()],
            cwd: PathBuf::from("/home/ü"),
            startup_id: Some("token_TIME42".into()),
        }
    }

    /// Runs an owner that answers every launch with `answer`, on a thread.
    fn owner(endpoint: &Endpoint, answer: impl Fn(Invocation) + Send + 'static) -> std::thread::JoinHandle<Option<Request>> {
        let mut invocations = Listener::bind(endpoint).expect("bind").spawn_accept().expect("accept thread");
        std::thread::spawn(move || {
            let invocation = futures::executor::block_on(invocations.next())?;
            let request = invocation.request.clone();
            answer(invocation);
            Some(request)
        })
    }

    #[test]
    fn a_forwarded_launch_reaches_the_owner_and_gets_its_stderr_and_status() {
        let (endpoint, dir) = endpoint("relay");
        let owner = owner(&endpoint, |mut invocation| {
            invocation.reply.stderr("vsesvit: first line");
            invocation.reply.stderr("second line, ü");
            invocation.reply.finish(3);
        });
        let mut stderr = Vec::new();
        let status = forward(&endpoint, &request(), &mut stderr).expect("forwarded");
        assert_eq!(status, 3);
        assert_eq!(String::from_utf8(stderr).unwrap(), "vsesvit: first line\nsecond line, ü\n");
        assert_eq!(owner.join().unwrap(), Some(request()));
        if let Some(dir) = dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn an_unanswered_launch_exits_one_with_a_reason() {
        let (endpoint, dir) = endpoint("dropped");
        let owner = owner(&endpoint, drop);
        let mut stderr = Vec::new();
        assert_eq!(forward(&endpoint, &request(), &mut stderr).expect("forwarded"), 1);
        assert!(String::from_utf8(stderr).unwrap().contains("did not handle this launch"));
        owner.join().unwrap();
        if let Some(dir) = dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn nobody_listening_is_unreachable_after_the_timeout() {
        let (endpoint, _) = endpoint("absent");
        let started = Instant::now();
        let mut stderr = Vec::new();
        let result = forward_within(&endpoint, &request(), &mut stderr, Duration::from_millis(300));
        assert!(result.is_err(), "{result:?}");
        assert!(started.elapsed() >= Duration::from_millis(300), "it retried until the timeout");
        assert!(UNREACHABLE.contains("could not be reached"));
    }

    #[test]
    fn a_request_that_does_not_parse_is_closed_unanswered_and_the_next_one_is_served() {
        let (endpoint, dir) = endpoint("garbage");
        let owner = owner(&endpoint, |invocation| invocation.reply.finish(0));
        {
            let (mut writer, reader) = connect(&endpoint, CONNECT_TIMEOUT).expect("connect");
            writer.write_all(b"not json\n").unwrap();
            let mut rest = String::new();
            let _ = BufReader::new(reader).read_line(&mut rest);
            assert_eq!(rest, "", "closed without an answer");
        }
        assert_eq!(forward(&endpoint, &request(), &mut Vec::new()).expect("forwarded"), 0);
        assert_eq!(owner.join().unwrap(), Some(request()));
        if let Some(dir) = dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn every_spelling_of_a_profile_names_one_endpoint() {
        let root = std::env::temp_dir().join(format!("vsesvit-boot-endpoint-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let roundabout = root.join("sub").join("..");
        assert_eq!(Endpoint::of_profile(&root), Endpoint::of_profile(&roundabout));
        assert_ne!(Endpoint::of_profile(&root), Endpoint::of_profile(&root.join("sub")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
