//! Owned by W0.5.
//! The guard's decision (design section 7.3), over what a `Machine` reports. It ties the run to the
//! display or desktop the launcher created, never to a variable alone: every answer that names the
//! run comes from the X server, `/proc` or the window station. Any doubt, including a query that
//! errors, is a refusal.

use std::path::Path;

use crate::envfile::{EnvFile, Place, Token, VAR};

/// What the guard learns about `env.json` from the file system.
pub(crate) struct FileFacts {
    pub text: String,
    /// The file's owner is this process's user.
    pub owned_by_me: bool,
    /// Unix permission bits; `None` where there are none.
    pub mode: Option<u32>,
}

/// The questions the guard asks the machine. Each OS answers its own and refuses the other's.
pub(crate) trait Machine {
    fn var(&self, name: &str) -> Option<String>;
    fn env_file(&self, path: &Path) -> Result<FileFacts, String>;
    /// The pid in `/tmp/.X<N>-lock` for `display` (`:N`).
    fn lock_pid(&self, display: &str) -> Result<u32, String>;
    /// The file name of `/proc/<pid>/exe`.
    fn exe_name(&self, pid: u32) -> Result<String, String>;
    /// The root window's `_VSESVIT_TESTENV` on `display`, if it has one.
    fn root_token(&self, display: &str) -> Result<Option<String>, String>;
    /// The name of this thread's desktop.
    fn thread_desktop(&self) -> Result<String, String>;
    /// The name of the desktop that receives the user's input.
    fn input_desktop(&self) -> Result<String, String>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Linux,
    Windows,
}

impl Os {
    pub const CURRENT: Os = if cfg!(windows) { Os::Windows } else { Os::Linux };
}

pub(crate) fn check(machine: &impl Machine, os: Os) -> Result<EnvFile, String> {
    let path = machine
        .var(VAR)
        .filter(|p| !p.is_empty())
        .ok_or(format!("{VAR} is not set; run this through `cargo xtask probe` or `cargo xtask selftest`"))?;
    let facts = machine.env_file(Path::new(&path)).map_err(|e| format!("{path}: {e}"))?;
    let file = EnvFile::parse(&facts.text).map_err(|e| format!("{path}: {e}"))?;
    match (&file.place, os) {
        (Place::Xvfb { display, xvfb_pid, dbus_address }, Os::Linux) => {
            check_xvfb(machine, &facts, &file.token, display, *xvfb_pid, dbus_address)
        }
        (Place::Desktop { name }, Os::Windows) => check_desktop(machine, &file.token, name),
        _ => Err(format!("{path} describes another OS's test environment")),
    }
    .map_err(|e| format!("{path}: {e}"))?;
    Ok(file)
}

/// The X server is asked last, once `DISPLAY` is known to be the recorded display and its lock
/// names the recorded Xvfb, so the guard never queries a server the launcher did not start.
fn check_xvfb(
    machine: &impl Machine,
    facts: &FileFacts,
    token: &Token,
    display: &str,
    xvfb_pid: u32,
    dbus_address: &str,
) -> Result<(), String> {
    if !facts.owned_by_me {
        return Err("env.json belongs to another user".into());
    }
    match facts.mode {
        Some(mode) if mode & 0o077 == 0 => {}
        Some(mode) => return Err(format!("env.json has mode {:o}; the launcher writes it 600", mode & 0o777)),
        None => return Err("env.json has no permission bits".into()),
    }
    let shown = machine.var("DISPLAY");
    if shown.as_deref() != Some(display) {
        return Err(format!("DISPLAY is {shown:?}, not the test environment's {display}"));
    }
    if let Some(wayland) = machine.var("WAYLAND_DISPLAY") {
        return Err(format!("WAYLAND_DISPLAY is set ({wayland:?}); a test run has no Wayland display"));
    }
    let bus = machine.var("DBUS_SESSION_BUS_ADDRESS");
    if bus.as_deref() != Some(dbus_address) {
        return Err(format!("the D-Bus session is {bus:?}, not the test environment's own"));
    }
    let locked = machine.lock_pid(display)?;
    if locked != xvfb_pid {
        return Err(format!("{display} is served by pid {locked}, not the launcher's Xvfb {xvfb_pid}"));
    }
    let exe = machine.exe_name(locked)?;
    if exe != "Xvfb" {
        return Err(format!("{display} is served by {exe}, not Xvfb"));
    }
    match machine.root_token(display)? {
        Some(found) if found == token.as_str() => Ok(()),
        Some(_) => Err(format!("the root window of {display} carries another run's token")),
        None => Err(format!("the root window of {display} carries no test-environment token")),
    }
}

/// Names are compared, never handle values.
fn check_desktop(machine: &impl Machine, token: &Token, name: &str) -> Result<(), String> {
    let expected = token.desktop_name();
    if name != expected {
        return Err(format!("desktop {name:?} is not the token's {expected:?}"));
    }
    let thread = machine.thread_desktop().map_err(|e| format!("this thread's desktop: {e}"))?;
    if thread != expected {
        return Err(format!("this thread runs on desktop {thread:?}, not the test environment's {expected:?}"));
    }
    let input = machine.input_desktop().map_err(|e| format!("the input desktop: {e}"))?;
    if input == thread {
        return Err(format!("this thread runs on the input desktop {input:?}, which the user sees"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const ENV_PATH: &str = "/out/testenv/env.json";
    const DISPLAY: &str = ":101";
    const XVFB: u32 = 4242;
    const BUS: &str = "unix:path=/out/testenv/bus,guid=7";

    fn token() -> Token {
        Token::from_bytes([0x5a; 16])
    }

    /// A machine whose every answer a test can change. A query of a server other than the
    /// launcher's panics: the guard must refuse before it asks.
    struct Fake {
        vars: HashMap<&'static str, String>,
        file: Result<String, String>,
        owned_by_me: bool,
        mode: Option<u32>,
        lock_pid: Result<u32, String>,
        exe: Result<String, String>,
        root: Result<Option<String>, String>,
        thread: Result<String, String>,
        input: Result<String, String>,
    }

    impl Fake {
        fn linux() -> Fake {
            let file = EnvFile {
                token: token(),
                place: Place::Xvfb { display: DISPLAY.into(), xvfb_pid: XVFB, dbus_address: BUS.into() },
            };
            Fake {
                vars: HashMap::from([
                    (VAR, ENV_PATH.to_owned()),
                    ("DISPLAY", DISPLAY.to_owned()),
                    ("DBUS_SESSION_BUS_ADDRESS", BUS.to_owned()),
                ]),
                file: Ok(file.to_json()),
                owned_by_me: true,
                mode: Some(0o100600),
                lock_pid: Ok(XVFB),
                exe: Ok("Xvfb".into()),
                root: Ok(Some(token().as_str().to_owned())),
                thread: Err("not Windows".into()),
                input: Err("not Windows".into()),
            }
        }

        fn windows() -> Fake {
            let file = EnvFile { token: token(), place: Place::Desktop { name: token().desktop_name() } };
            Fake {
                vars: HashMap::from([(VAR, r"C:\out\testenv\env.json".to_owned())]),
                file: Ok(file.to_json()),
                owned_by_me: true,
                mode: None,
                lock_pid: Err("not Linux".into()),
                exe: Err("not Linux".into()),
                root: Err("not Linux".into()),
                thread: Ok(token().desktop_name()),
                input: Ok("Default".into()),
            }
        }
    }

    impl Machine for Fake {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn env_file(&self, _: &Path) -> Result<FileFacts, String> {
            let text = self.file.clone()?;
            Ok(FileFacts { text, owned_by_me: self.owned_by_me, mode: self.mode })
        }
        fn lock_pid(&self, display: &str) -> Result<u32, String> {
            assert_eq!(display, DISPLAY, "asked about a display the launcher did not start");
            self.lock_pid.clone()
        }
        fn exe_name(&self, _: u32) -> Result<String, String> {
            self.exe.clone()
        }
        fn root_token(&self, display: &str) -> Result<Option<String>, String> {
            assert_eq!(display, DISPLAY, "queried an X server the launcher did not start");
            assert_eq!(self.lock_pid, Ok(XVFB), "queried a server whose lock names another process");
            self.root.clone()
        }
        fn thread_desktop(&self) -> Result<String, String> {
            self.thread.clone()
        }
        fn input_desktop(&self) -> Result<String, String> {
            self.input.clone()
        }
    }

    fn refused(machine: &Fake, os: Os, why: &str) {
        match check(machine, os) {
            Ok(_) => panic!("the guard passed, but should refuse: {why}"),
            Err(reason) => assert!(reason.contains(why), "refused for {reason:?}, expected {why:?}"),
        }
    }

    #[test]
    fn passes_on_the_launchers_xvfb() {
        let file = check(&Fake::linux(), Os::Linux).unwrap();
        assert_eq!(file.token, token());
    }

    #[test]
    fn passes_on_the_launchers_desktop() {
        check(&Fake::windows(), Os::Windows).unwrap();
    }

    #[test]
    fn a_re_executed_phase_in_the_same_environment_passes_again() {
        let linux = Fake::linux();
        let windows = Fake::windows();
        for _ in 0..2 {
            check(&linux, Os::Linux).unwrap();
            check(&windows, Os::Windows).unwrap();
        }
    }

    #[test]
    fn refuses_without_the_variable() {
        let mut m = Fake::linux();
        m.vars.remove(VAR);
        refused(&m, Os::Linux, "VSESVIT_TESTENV is not set");
        m.vars.insert(VAR, String::new());
        refused(&m, Os::Linux, "VSESVIT_TESTENV is not set");
    }

    #[test]
    fn refuses_a_missing_env_json() {
        let mut m = Fake::linux();
        m.file = Err("No such file or directory".into());
        refused(&m, Os::Linux, "No such file");
        let mut m = Fake::windows();
        m.file = Err("The system cannot find the file specified".into());
        refused(&m, Os::Windows, "cannot find");
    }

    #[test]
    fn refuses_a_foreign_owned_env_json() {
        let mut m = Fake::linux();
        m.owned_by_me = false;
        refused(&m, Os::Linux, "another user");
    }

    #[test]
    fn refuses_a_world_or_group_readable_env_json() {
        for mode in [0o100644, 0o100640, 0o100606] {
            let mut m = Fake::linux();
            m.mode = Some(mode);
            refused(&m, Os::Linux, "the launcher writes it 600");
        }
    }

    #[test]
    fn refuses_a_token_mismatch() {
        let mut m = Fake::linux();
        m.root = Ok(Some(Token::from_bytes([1; 16]).as_str().to_owned()));
        refused(&m, Os::Linux, "another run's token");
        let mut m = Fake::linux();
        m.root = Ok(None);
        refused(&m, Os::Linux, "no test-environment token");
        let mut m = Fake::windows();
        m.thread = Ok(Token::from_bytes([1; 16]).desktop_name());
        refused(&m, Os::Windows, "not the test environment's");
    }

    #[test]
    fn refuses_a_display_pointed_at_another_server_after_launch() {
        let mut m = Fake::linux();
        m.vars.insert("DISPLAY", ":7".into());
        refused(&m, Os::Linux, "DISPLAY is Some(\":7\")");
        let mut m = Fake::linux();
        m.vars.remove("DISPLAY");
        refused(&m, Os::Linux, "DISPLAY is None");
    }

    #[test]
    fn refuses_the_variable_inherited_into_a_shell_on_display_0() {
        let mut m = Fake::linux();
        m.vars.insert("DISPLAY", ":0".into());
        m.vars.insert("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus".into());
        m.vars.insert("WAYLAND_DISPLAY", "wayland-0".into());
        refused(&m, Os::Linux, "not the test environment's :101");
    }

    #[test]
    fn refuses_a_wayland_display_or_a_foreign_bus() {
        let mut m = Fake::linux();
        m.vars.insert("WAYLAND_DISPLAY", "wayland-0".into());
        refused(&m, Os::Linux, "WAYLAND_DISPLAY is set");
        let mut m = Fake::linux();
        m.vars.insert("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus".into());
        refused(&m, Os::Linux, "D-Bus session");
    }

    #[test]
    fn refuses_a_server_that_is_not_the_recorded_xvfb() {
        let mut m = Fake::linux();
        m.lock_pid = Ok(XVFB + 1);
        refused(&m, Os::Linux, "not the launcher's Xvfb");
        let mut m = Fake::linux();
        m.exe = Ok("Xorg".into());
        refused(&m, Os::Linux, "served by Xorg");
        let mut m = Fake::linux();
        m.lock_pid = Err("/tmp/.X101-lock: No such file or directory".into());
        refused(&m, Os::Linux, "X101-lock");
    }

    #[test]
    fn refuses_when_the_x_server_query_errors() {
        let mut m = Fake::linux();
        m.root = Err("connection refused".into());
        refused(&m, Os::Linux, "connection refused");
    }

    #[test]
    fn refuses_when_a_windows_desktop_query_fails() {
        let mut m = Fake::windows();
        m.thread = Err("GetUserObjectInformationW: Access is denied".into());
        refused(&m, Os::Windows, "this thread's desktop: GetUserObjectInformationW");
        let mut m = Fake::windows();
        m.input = Err("OpenInputDesktop: Access is denied".into());
        refused(&m, Os::Windows, "the input desktop: OpenInputDesktop");
    }

    #[test]
    fn refuses_a_thread_desktop_equal_to_the_input_desktop() {
        let mut m = Fake::windows();
        m.input = Ok(token().desktop_name());
        refused(&m, Os::Windows, "the input desktop");
    }

    #[test]
    fn refuses_a_desktop_name_that_is_not_the_tokens() {
        let mut m = Fake::windows();
        let other = EnvFile { token: token(), place: Place::Desktop { name: "Default".into() } };
        m.file = Ok(other.to_json());
        m.thread = Ok("Default".into());
        refused(&m, Os::Windows, "is not the token's");
    }

    #[test]
    fn refuses_another_oss_env_json() {
        refused(&Fake::linux(), Os::Windows, "another OS's");
        refused(&Fake::windows(), Os::Linux, "another OS's");
    }
}
