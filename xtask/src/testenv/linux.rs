//! Owned by W0.5.
//! Linux: Xvfb on a free display `:N` (never `:0`), `-nolisten tcp`, with a fresh MIT cookie in a
//! private auth file and the run's token on its root window; openbox as its window manager; a private
//! D-Bus session; and the program started with a cleared environment that names only those (design
//! section 7.3). Everything started here is killed when the run ends: the program's whole process
//! group, the bus, the window manager and the server.

use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::envfile::{EnvFile, Place, ROOT_PROPERTY, Token, VAR};
use super::{Run, write_env_file, write_private};
use crate::Result;

const FIRST_DISPLAY: u32 = 100;
const LAST_DISPLAY: u32 = 299;
const START_TIMEOUT: Duration = Duration::from_secs(15);
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
/// How long a process gets to clean up after SIGTERM (Xvfb removes its lock file).
const STOP_GRACE: Duration = Duration::from_secs(3);
/// Passed through from the caller's environment; nothing else is.
const PASSED: &[&str] = &["PATH", "RUST_BACKTRACE", "RUST_LOG"];
const COOKIE_NAME: &[u8] = b"MIT-MAGIC-COOKIE-1";

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

/// A process that is stopped, with its process group, when this is dropped: SIGTERM, then
/// SIGKILL after `STOP_GRACE`. Once the process itself is reaped its pid may be reused, so then
/// only its group is signalled.
struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let pid = self.0.id() as i32;
        let signal = |child: &mut Child, signal: i32| {
            let alive = matches!(child.try_wait(), Ok(None));
            unsafe {
                kill(-pid, signal);
                if alive {
                    kill(pid, signal);
                }
            }
        };
        signal(&mut self.0, SIGTERM);
        let deadline = Instant::now() + STOP_GRACE;
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        signal(&mut self.0, SIGKILL);
        let _ = self.0.wait();
    }
}

pub(super) fn launch(run: &Run, dir: &Path, token: Token) -> Result<i32> {
    let n = free_display(|n| Path::new(&format!("/tmp/.X{n}-lock")).exists() || Path::new(&format!("/tmp/.X11-unix/X{n}")).exists())?;
    let display = format!(":{n}");
    let cookie = super::random_bytes()?;
    let auth = dir.join("Xauthority");
    write_private(&auth, &xauthority(&hostname(), n, &cookie))?;

    let log = std::fs::File::create(dir.join("xvfb.log")).map_err(|e| format!("xvfb.log: {e}"))?;
    let mut xvfb = Command::new("Xvfb");
    xvfb.arg(&display)
        // Not -displayfd: with it Xvfb writes no /tmp/.X<N>-lock, which the guard reads.
        .args(["-nolisten", "tcp", "-noreset", "-screen", "0", "1600x1000x24", "-auth"])
        .arg(&auth)
        .env_clear()
        .envs(passed())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0);
    let mut xvfb = Killed(xvfb.spawn().map_err(|e| format!("Xvfb: {e} (is it on PATH?)"))?);
    let deadline = Instant::now() + START_TIMEOUT;
    let mut x = loop {
        if let Some(status) = xvfb.0.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("Xvfb {display} exited with {status}; see {}", dir.join("xvfb.log").display()));
        }
        match connect(n) {
            Ok(x) => break x,
            Err(e) if Instant::now() > deadline => return Err(format!("Xvfb not ready within {START_TIMEOUT:?}: {e}")),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let screen = mark_root(&mut x, n, &cookie, &token)?;

    // Xvfb alone never activates a window or gives one the X focus, so W1.1's focus and activation
    // checks need a window manager (log.tsv 2026-10-24). openbox keeps XTEST in this server.
    let wm_home = dir.join("wm-home");
    std::fs::create_dir_all(&wm_home).map_err(|e| format!("{}: {e}", wm_home.display()))?;
    let wm_log = std::fs::File::create(dir.join("openbox.log")).map_err(|e| format!("openbox.log: {e}"))?;
    let mut wm = Command::new("openbox");
    wm.arg("--sm-disable")
        .env_clear()
        .envs(passed())
        .env("DISPLAY", &display)
        .env("XAUTHORITY", &auth)
        .env("HOME", &wm_home)
        .stdin(Stdio::null())
        .stdout(wm_log.try_clone().map_err(|e| e.to_string())?)
        .stderr(wm_log)
        .process_group(0);
    let mut wm = Killed(wm.spawn().map_err(|e| format!("openbox: {e} (is it on PATH?)"))?);
    // Ready means managing: openbox drops map requests that arrive while it starts, even after it
    // has announced itself on the root, so a test window has to come back framed.
    let test_window = screen.id_base;
    create_window(&mut x, test_window, screen.root).map_err(|e| format!("a window on :{n}: {e}"))?;
    let deadline = Instant::now() + START_TIMEOUT;
    while !framed(&mut x, test_window, screen.root).map_err(|e| format!("asking :{n} for its window manager: {e}"))? {
        if let Some(status) = wm.0.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("openbox exited with {status}; see {}", dir.join("openbox.log").display()));
        }
        if Instant::now() > deadline {
            return Err(format!("openbox not ready within {START_TIMEOUT:?}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Closing the connection destroys the test window.
    drop(x);

    let bus_path = dir.join("bus");
    let mut bus = Command::new("dbus-daemon");
    bus.args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address=unix:path={}", bus_path.display()))
        .env_clear()
        .envs(passed())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .process_group(0);
    let mut bus = Killed(bus.spawn().map_err(|e| format!("dbus-daemon: {e}"))?);
    let dbus_address = first_line(bus.0.stdout.take(), "dbus-daemon's address")?.trim().to_owned();

    let place = Place::Xvfb { display: display.clone(), xvfb_pid: xvfb.0.id(), dbus_address: dbus_address.clone() };
    let env_file = write_env_file(dir, &EnvFile { token, place })?;
    let home = run.out_dir.join("home");
    let runtime = dir.join("run");
    for d in [&home, &runtime] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::set_permissions(&runtime, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|e| format!("{}: {e}", runtime.display()))?;

    let mut program = Command::new(&run.program);
    program
        .args(&run.args)
        .current_dir(&run.out_dir)
        .env_clear()
        .envs(passed())
        .env(VAR, &env_file)
        .env("DISPLAY", &display)
        .env("XAUTHORITY", &auth)
        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_address)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .process_group(0);
    if let Some(icd) = lavapipe() {
        program.env("VK_ICD_FILENAMES", icd);
    }
    eprintln!("testenv: Xvfb {display} (pid {}), bus {dbus_address}", xvfb.0.id());
    let mut program = Killed(program.spawn().map_err(|e| format!("{}: {e}", run.program.display()))?);
    let deadline = Instant::now() + run.timeout;
    let status = loop {
        if let Some(status) = program.0.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() > deadline {
            return Err(format!("the run did not end within {:?}; killed it", run.timeout));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(program);
    if let Some(died) = wm.0.try_wait().map_err(|e| e.to_string())? {
        return Err(format!("openbox exited with {died} during the run; see {}", dir.join("openbox.log").display()));
    }
    drop(bus);
    drop(wm);
    drop(xvfb);
    use std::os::unix::process::ExitStatusExt as _;
    status.code().ok_or(format!("the program was killed by signal {:?}", status.signal()))
}

fn passed() -> impl Iterator<Item = (&'static str, std::ffi::OsString)> {
    PASSED.iter().filter_map(|k| std::env::var_os(k).map(|v| (*k, v)))
}

/// The first display number from which no lock file or socket is in use.
fn free_display(taken: impl Fn(u32) -> bool) -> Result<u32> {
    (FIRST_DISPLAY..=LAST_DISPLAY).find(|&n| !taken(n)).ok_or(format!("no free display in :{FIRST_DISPLAY}..:{LAST_DISPLAY}"))
}

/// The first line a child prints, within `START_TIMEOUT`.
fn first_line(stdout: Option<ChildStdout>, what: &str) -> Result<String> {
    let stdout = stdout.ok_or(format!("{what}: no pipe"))?;
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let read = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = send.send(read);
    });
    match receive.recv_timeout(START_TIMEOUT) {
        Ok(Ok(line)) if !line.is_empty() => Ok(line),
        Ok(Ok(_)) => Err(format!("{what}: the process exited before it was ready")),
        Ok(Err(e)) => Err(format!("{what}: {e}")),
        Err(_) => Err(format!("{what}: nothing within {START_TIMEOUT:?}")),
    }
}

fn lavapipe() -> Option<PathBuf> {
    std::fs::read_dir("/usr/share/vulkan/icd.d").ok()?.filter_map(|e| e.ok().map(|e| e.path())).find(|p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("lvp_icd"))
    })
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_owned()).unwrap_or_default()
}

/// An Xauthority file with the cookie for display `n`: one entry for this host, one wildcard.
fn xauthority(host: &str, n: u32, cookie: &[u8]) -> Vec<u8> {
    const FAMILY_LOCAL: u16 = 256;
    const FAMILY_WILD: u16 = 0xFFFF;
    let number = n.to_string();
    let mut out = Vec::new();
    for (family, address) in [(FAMILY_LOCAL, host.as_bytes()), (FAMILY_WILD, &b""[..])] {
        out.extend(family.to_be_bytes());
        for field in [address, number.as_bytes(), COOKIE_NAME, cookie] {
            out.extend((field.len() as u16).to_be_bytes());
            out.extend(field);
        }
    }
    out
}

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

/// Sets the root window's `_VSESVIT_TESTENV` to the token, over the X11 protocol itself (xtask has
/// no X library). `-noreset` keeps the property after this client disconnects.
fn mark_root(x: &mut UnixStream, n: u32, cookie: &[u8; 16], token: &Token) -> Result<Screen> {
    let failed = |e: std::io::Error| format!("marking :{n}: {e}");
    let mut setup = vec![b'l', 0];
    setup.extend(11u16.to_le_bytes());
    setup.extend(0u16.to_le_bytes());
    setup.extend((COOKIE_NAME.len() as u16).to_le_bytes());
    setup.extend((cookie.len() as u16).to_le_bytes());
    setup.extend([0, 0]);
    for field in [COOKIE_NAME, &cookie[..]] {
        setup.extend(field);
        setup.extend(std::iter::repeat_n(0, pad4(field.len())));
    }
    x.write_all(&setup).map_err(failed)?;
    let mut head = [0u8; 8];
    x.read_exact(&mut head).map_err(failed)?;
    let mut body = vec![0u8; usize::from(u16::from_le_bytes([head[6], head[7]])) * 4];
    x.read_exact(&mut body).map_err(failed)?;
    if head[0] != 1 {
        let reason = String::from_utf8_lossy(&body[..usize::from(head[1]).min(body.len())]).into_owned();
        return Err(format!(":{n} refused the connection: {reason}"));
    }
    let root = root_window(&body).ok_or(format!(":{n} sent a setup reply without a screen"))?;
    let id_base = u32::from_le_bytes(body[4..8].try_into().expect("4 bytes"));

    let name = ROOT_PROPERTY.as_bytes();
    let mut intern = vec![16, 0];
    intern.extend((((8 + name.len() + pad4(name.len())) / 4) as u16).to_le_bytes());
    intern.extend((name.len() as u16).to_le_bytes());
    intern.extend([0, 0]);
    intern.extend(name);
    intern.extend(std::iter::repeat_n(0, pad4(name.len())));
    x.write_all(&intern).map_err(failed)?;
    let reply = read_reply(x).map_err(failed)?;
    if reply[0] != 1 {
        return Err(format!(":{n}: InternAtom failed with X error {}", reply[1]));
    }
    let atom = u32::from_le_bytes(reply[8..12].try_into().expect("4 bytes"));

    const STRING: u32 = 31;
    let value = token.as_str().as_bytes();
    let mut change = vec![18, 0];
    change.extend((((24 + value.len() + pad4(value.len())) / 4) as u16).to_le_bytes());
    for word in [root, atom, STRING] {
        change.extend(word.to_le_bytes());
    }
    change.extend([8, 0, 0, 0]);
    change.extend((value.len() as u32).to_le_bytes());
    change.extend(value);
    change.extend(std::iter::repeat_n(0, pad4(value.len())));
    // GetInputFocus: its reply comes after ChangeProperty's error, if there is one.
    change.extend([43, 0, 1, 0]);
    x.write_all(&change).map_err(failed)?;
    let reply = read_reply(x).map_err(failed)?;
    if reply[0] != 1 {
        return Err(format!(":{n}: ChangeProperty failed with X error {}", reply[1]));
    }
    Ok(Screen { root, id_base })
}

/// The two facts of a setup reply the launcher uses.
struct Screen {
    root: u32,
    /// The first resource id this connection may use.
    id_base: u32,
}

/// An unmapped 1x1 window at 0,0 that selects no events.
fn create_window(x: &mut UnixStream, id: u32, parent: u32) -> std::io::Result<()> {
    let mut create = vec![1, 0];
    create.extend(8u16.to_le_bytes());
    create.extend(id.to_le_bytes());
    create.extend(parent.to_le_bytes());
    for half in [0u16, 0, 1, 1, 0, 1] {
        create.extend(half.to_le_bytes());
    }
    create.extend([0u32, 0].iter().flat_map(|w| w.to_le_bytes()));
    x.write_all(&create)
}

/// Maps `id` (a window manager turns that into a request it answers by framing the window), then
/// reports whether it already sits in a frame rather than on the root.
fn framed(x: &mut UnixStream, id: u32, root: u32) -> std::io::Result<bool> {
    let mut requests = vec![8, 0];
    requests.extend(2u16.to_le_bytes());
    requests.extend(id.to_le_bytes());
    requests.extend([15, 0]);
    requests.extend(2u16.to_le_bytes());
    requests.extend(id.to_le_bytes());
    x.write_all(&requests)?;
    let reply = read_reply(x)?;
    if reply[0] != 1 {
        return Err(std::io::Error::other(format!("X error {} from MapWindow or QueryTree", reply[1])));
    }
    let parent = u32::from_le_bytes(reply[12..16].try_into().expect("4 bytes"));
    Ok(parent != root)
}
/// The private server listens on the abstract socket (WSL has no writable `/tmp/.X11-unix`), and
/// also on the path elsewhere.
fn connect(n: u32) -> Result<UnixStream> {
    use std::os::linux::net::SocketAddrExt as _;
    let name = format!("/tmp/.X11-unix/X{n}");
    let abstract_addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).map_err(|e| e.to_string())?;
    UnixStream::connect_addr(&abstract_addr)
        .or_else(|_| UnixStream::connect(&name))
        .map_err(|e| format!("connecting to :{n}: {e}"))
}

/// One 32-byte reply or error, with any extra reply data read and dropped.
fn read_reply(x: &mut UnixStream) -> std::io::Result<[u8; 32]> {
    let mut reply = [0u8; 32];
    x.read_exact(&mut reply)?;
    if reply[0] == 1 {
        let extra = u32::from_le_bytes(reply[4..8].try_into().expect("4 bytes")) as usize * 4;
        std::io::copy(&mut Read::take(&mut *x, extra as u64), &mut std::io::sink())?;
    }
    Ok(reply)
}

/// The first screen's root window in a setup reply's body (after its 8-byte head).
fn root_window(body: &[u8]) -> Option<u32> {
    let vendor = usize::from(u16::from_le_bytes(body.get(16..18)?.try_into().ok()?));
    let formats = usize::from(*body.get(21)?);
    let screen = 32 + vendor + pad4(vendor) + 8 * formats;
    Some(u32::from_le_bytes(body.get(screen..screen + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_start_at_100_and_skip_taken_ones() {
        assert_eq!(free_display(|_| false), Ok(100));
        assert_eq!(free_display(|n| n < 103), Ok(103));
        assert!(free_display(|_| true).is_err());
    }

    #[test]
    fn the_auth_file_has_a_host_entry_and_a_wildcard() {
        let file = xauthority("box", 101, &[7; 16]);
        let mut entry = vec![1, 0, 0, 3, b'b', b'o', b'x', 0, 3, b'1', b'0', b'1', 0, 18];
        entry.extend(COOKIE_NAME);
        entry.extend([0, 16]);
        entry.extend([7; 16]);
        assert_eq!(&file[..entry.len()], &entry[..]);
        assert_eq!(&file[entry.len()..entry.len() + 4], &[0xFF, 0xFF, 0, 0]);
        assert_eq!(file.len(), 2 * entry.len() - 3);
    }

    #[test]
    fn the_root_window_comes_after_the_vendor_and_the_formats() {
        let mut body = vec![0u8; 32];
        body[16] = 5;
        body[21] = 2;
        body.extend(b"Xorg\0\0\0\0");
        body.extend([0u8; 16]);
        body.extend(0x0000_02a1u32.to_le_bytes());
        assert_eq!(root_window(&body), Some(0x2a1));
        assert_eq!(root_window(&body[..20]), None);
    }
}
