//! Owned by W0.5.
//! Windows: the program starts on a private desktop from `CreateDesktopW`, which nobody switches
//! to, inside a job object that kills everything it started when the run ends (SW section 1.1,
//! `spikes/win-core/src/bin/deskrun.rs`). A watchdog records the user's foreground window before the
//! run and fails the run if it ever becomes a window of the job's processes.
//!
//! Q22's Windows Sandbox route is not built: XAML runs on the private desktop (log.tsv, W0.5).

use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use super::envfile::{EnvFile, Place, Token, VAR};
use super::{Run, write_env_file};
use crate::Result;

type Handle = *mut c_void;
type Bool = i32;

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2_len: u16,
    reserved2: *mut u8,
    std_input: Handle,
    std_output: Handle,
    std_error: Handle,
}

#[repr(C)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

#[repr(C)]
#[derive(Default)]
struct BasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Default)]
struct ExtendedLimitInformation {
    basic: BasicLimitInformation,
    io_counters: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

const MAX_JOB_PIDS: usize = 1024;

#[repr(C)]
struct BasicProcessIdList {
    assigned: u32,
    in_list: u32,
    ids: [usize; MAX_JOB_PIDS],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateProcessW(
        application: *const u16,
        command_line: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit_handles: Bool,
        flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup_info: *const StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> Bool;
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> Bool;
    fn QueryInformationJobObject(job: Handle, class: i32, info: *mut c_void, len: u32, returned: *mut u32) -> Bool;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> Bool;
    fn TerminateJobObject(job: Handle, exit_code: u32) -> Bool;
    fn TerminateProcess(process: Handle, exit_code: u32) -> Bool;
    fn ResumeThread(thread: Handle) -> u32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> Bool;
    fn GetStdHandle(which: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> Bool;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        devmode: *const c_void,
        flags: u32,
        access: u32,
        attributes: *const c_void,
    ) -> Handle;
    fn CloseDesktop(desktop: Handle) -> Bool;
    fn GetForegroundWindow() -> Handle;
    fn GetWindowThreadProcessId(window: Handle, process_id: *mut u32) -> u32;
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

const GENERIC_ALL: u32 = 0x1000_0000;
const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
const STD_INPUT_HANDLE: u32 = -10i32 as u32;
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const STD_ERROR_HANDLE: u32 = -12i32 as u32;
const JOB_OBJECT_BASIC_PROCESS_ID_LIST: i32 = 3;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
const WAIT_OBJECT_0: u32 = 0;
const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;
const POLL: Duration = Duration::from_millis(50);
/// Chromium (and so WebView2) otherwise counts a window on an undisplayed desktop as covered and
/// stops rendering it (design section 7.3). WebView2 appends this to the app's own arguments.
const WEBVIEW2_ARGUMENTS: (&str, &str) = ("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-features=CalculateNativeWinOcclusion");

fn last_error(call: &str) -> String {
    format!("{call}: {}", std::io::Error::last_os_error())
}

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain([0]).collect()
}

pub(super) fn random_bytes() -> Result<[u8; 16]> {
    let mut bytes = [0u8; 16];
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), 16, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status != 0 {
        return Err(format!("BCryptGenRandom: NTSTATUS {status:#x}"));
    }
    Ok(bytes)
}

pub(super) fn launch(run: &Run, dir: &Path, token: Token) -> Result<i32> {
    let name = token.desktop_name();
    let desktop = Desktop::create(&name)?;
    let env_file = write_env_file(dir, &EnvFile { token, place: Place::Desktop { name: name.clone() } })?;
    let job = Job::create()?;
    let vars = [(OsString::from(VAR), env_file.into_os_string()), (WEBVIEW2_ARGUMENTS.0.into(), WEBVIEW2_ARGUMENTS.1.into())];
    let child = spawn(run, &desktop.name, &vars, &job)?;
    eprintln!("testenv: pid {} on desktop {}", child.pid, desktop.name);
    let ended = supervise(&child, &job, run.timeout, &UserDesktop);
    drop(child);
    drop(job);
    drop(desktop);
    ended
}

/// A desktop of this window station that is never switched to.
struct Desktop {
    handle: Handle,
    name: String,
}

impl Desktop {
    fn create(name: &str) -> Result<Desktop> {
        let handle = unsafe {
            CreateDesktopW(wide(name).as_ptr(), std::ptr::null(), std::ptr::null(), 0, GENERIC_ALL, std::ptr::null())
        };
        if handle.is_null() {
            return Err(last_error(&format!("CreateDesktopW({name})")));
        }
        Ok(Desktop { handle, name: name.to_owned() })
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe { CloseDesktop(self.handle) };
    }
}

/// Every process the run starts, killed together.
struct Job {
    handle: Handle,
}

impl Job {
    fn create() -> Result<Job> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(last_error("CreateJobObjectW"));
        }
        let job = Job { handle };
        let mut limits = ExtendedLimitInformation::default();
        limits.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = unsafe {
            SetInformationJobObject(
                job.handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&raw const limits).cast(),
                size_of::<ExtendedLimitInformation>() as u32,
            )
        };
        if set == 0 {
            return Err(last_error("SetInformationJobObject"));
        }
        Ok(job)
    }

    fn pids(&self) -> Vec<u32> {
        let mut list: Box<BasicProcessIdList> = Box::new(BasicProcessIdList { assigned: 0, in_list: 0, ids: [0; MAX_JOB_PIDS] });
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle,
                JOB_OBJECT_BASIC_PROCESS_ID_LIST,
                (&raw mut *list).cast(),
                size_of::<BasicProcessIdList>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        list.ids[..list.in_list as usize].iter().map(|&id| id as u32).collect()
    }

    fn kill(&self) {
        unsafe { TerminateJobObject(self.handle, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.kill();
        unsafe { CloseHandle(self.handle) };
    }
}

struct Child {
    process: Handle,
    pid: u32,
}

impl Drop for Child {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.process) };
    }
}

/// Starts `run.program` suspended on `desktop`, puts it in `job`, then lets it run. It gets this
/// process's environment plus `vars`, and this process's standard handles.
fn spawn(run: &Run, desktop: &str, vars: &[(OsString, OsString)], job: &Job) -> Result<Child> {
    let mut command: Vec<u16> = std::iter::once(run.program.as_os_str())
        .chain(run.args.iter().map(OsString::as_os_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(&(' ' as u16));
    command.push(0);
    let environment = environment_block(std::env::vars_os(), vars);
    let mut desktop = wide(format!("WinSta0\\{desktop}"));
    let cwd = wide(&run.out_dir);
    let mut si: StartupInfoW = unsafe { std::mem::zeroed() };
    si.cb = size_of::<StartupInfoW>() as u32;
    si.desktop = desktop.as_mut_ptr();
    si.flags = STARTF_USESTDHANDLES;
    unsafe {
        si.std_input = GetStdHandle(STD_INPUT_HANDLE);
        si.std_output = GetStdHandle(STD_OUTPUT_HANDLE);
        si.std_error = GetStdHandle(STD_ERROR_HANDLE);
    }
    let mut pi: ProcessInformation = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &si,
            &mut pi,
        )
    };
    if created == 0 {
        return Err(last_error(&format!("CreateProcessW({})", run.program.display())));
    }
    let child = Child { process: pi.process, pid: pi.process_id };
    let assigned = unsafe { AssignProcessToJobObject(job.handle, pi.process) };
    let assign_error = (assigned == 0).then(|| last_error("AssignProcessToJobObject"));
    if let Some(e) = assign_error {
        unsafe {
            TerminateProcess(pi.process, 1);
            CloseHandle(pi.thread);
        }
        return Err(e);
    }
    unsafe {
        ResumeThread(pi.thread);
        CloseHandle(pi.thread);
    }
    Ok(child)
}

/// The user's input desktop as the watchdog sees it.
pub(super) trait Foreground {
    /// The foreground window, as a number.
    fn window(&self) -> usize;
    /// The process that owns `window`.
    fn owner(&self, window: usize) -> u32;
}

struct UserDesktop;

impl Foreground for UserDesktop {
    fn window(&self) -> usize {
        unsafe { GetForegroundWindow() as usize }
    }

    fn owner(&self, window: usize) -> u32 {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window as Handle, &mut pid) };
        pid
    }
}

/// Fails the run when the user's foreground window becomes one of the run's (SW section 1.1).
struct Watchdog {
    before: usize,
}

impl Watchdog {
    fn check(&self, desk: &impl Foreground, ours: &[u32]) -> Result {
        let now = desk.window();
        if now == self.before || now == 0 {
            return Ok(());
        }
        let owner = desk.owner(now);
        if ours.contains(&owner) {
            return Err(format!(
                "SAFETY: the user's foreground window changed to window {now:#x} of pid {owner}, a process of this run; the run is failed and killed"
            ));
        }
        Ok(())
    }
}

/// Waits for the program, polling the watchdog, then kills what is left of the job.
fn supervise(child: &Child, job: &Job, timeout: Duration, desk: &impl Foreground) -> Result<i32> {
    let watchdog = Watchdog { before: desk.window() };
    let deadline = Instant::now() + timeout;
    let code = loop {
        let exited = unsafe { WaitForSingleObject(child.process, POLL.as_millis() as u32) } == WAIT_OBJECT_0;
        let mut ours = job.pids();
        ours.push(child.pid);
        if let Err(e) = watchdog.check(desk, &ours) {
            job.kill();
            return Err(e);
        }
        if exited {
            let mut code = 0u32;
            if unsafe { GetExitCodeProcess(child.process, &mut code) } == 0 {
                job.kill();
                return Err(last_error("GetExitCodeProcess"));
            }
            break code as i32;
        }
        if Instant::now() > deadline {
            job.kill();
            return Err(format!("the run did not end within {timeout:?}; killed it"));
        }
    };
    watchdog.check(desk, &job.pids())?;
    job.kill();
    Ok(code)
}

/// This process's environment with `vars` set, as a sorted `CreateProcessW` block.
fn environment_block(inherited: impl Iterator<Item = (OsString, OsString)>, vars: &[(OsString, OsString)]) -> Vec<u16> {
    let key = |k: &OsStr| k.to_string_lossy().to_uppercase();
    let mut all: Vec<(OsString, OsString)> =
        inherited.filter(|(k, _)| !vars.iter().any(|(v, _)| key(v) == key(k))).chain(vars.iter().cloned()).collect();
    all.sort_by_key(|(k, _)| key(k));
    let mut block = Vec::new();
    for (k, v) in all {
        block.extend(k.encode_wide().chain("=".encode_utf16()).chain(v.encode_wide()).chain([0]));
    }
    block.push(0);
    block
}

/// One command-line argument, quoted the way `CommandLineToArgvW` reads it back.
fn quote(arg: &OsStr) -> Vec<u16> {
    let arg: Vec<u16> = arg.encode_wide().collect();
    let plain = !arg.is_empty() && !arg.iter().any(|&c| c == ' ' as u16 || c == '\t' as u16 || c == '"' as u16);
    if plain {
        return arg;
    }
    let mut out = vec!['"' as u16];
    let mut backslashes = 0;
    for &c in &arg {
        if c == '\\' as u16 {
            backslashes += 1;
            continue;
        }
        let doubled = if c == '"' as u16 { backslashes * 2 + 1 } else { backslashes };
        out.extend(std::iter::repeat_n('\\' as u16, doubled));
        backslashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n('\\' as u16, backslashes * 2));
    out.push('"' as u16);
    out
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ffi::OsString;
    use std::time::Duration;

    use super::*;

    fn text(arg: &str) -> String {
        String::from_utf16(&quote(OsStr::new(arg))).unwrap()
    }

    #[test]
    fn arguments_are_quoted_for_command_line_to_argv() {
        assert_eq!(text("plain"), "plain");
        assert_eq!(text(r"C:\Program Files\x"), r#""C:\Program Files\x""#);
        assert_eq!(text(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(text(r"ends\ "), r#""ends\ ""#);
        assert_eq!(text(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        assert_eq!(text(""), r#""""#);
    }

    #[test]
    fn the_environment_block_overrides_inherited_variables_case_insensitively() {
        let inherited = vec![(OsString::from("Path"), OsString::from("C:\\bin")), (OsString::from("vsesvit_testenv"), OsString::from("old"))];
        let block = environment_block(inherited.into_iter(), &[(VAR.into(), "new".into())]);
        let text = String::from_utf16(&block).unwrap();
        assert_eq!(text, "Path=C:\\bin\0VSESVIT_TESTENV=new\0\0");
    }

    /// A foreground that is the user's window until `flip_after` polls, then a window owned by
    /// `owner`.
    struct Flipping {
        polls: Cell<u32>,
        flip_after: u32,
        owner: Cell<u32>,
    }

    impl Foreground for Flipping {
        fn window(&self) -> usize {
            self.polls.set(self.polls.get() + 1);
            if self.polls.get() > self.flip_after { 0x2222 } else { 0x1111 }
        }
        fn owner(&self, _: usize) -> u32 {
            self.owner.get()
        }
    }

    #[test]
    fn the_watchdog_ignores_the_users_own_foreground_changes() {
        let watchdog = Watchdog { before: 0x1111 };
        let desk = Flipping { polls: Cell::new(5), flip_after: 0, owner: Cell::new(77) };
        assert!(watchdog.check(&desk, &[10, 11]).is_ok());
        assert!(watchdog.check(&desk, &[77]).unwrap_err().starts_with("SAFETY:"));
    }

    fn private_run(args: &[&str]) -> Run {
        let out_dir = std::env::temp_dir().join(format!("xtask-testenv-{}-{}", std::process::id(), args.join("-").len()));
        std::fs::create_dir_all(&out_dir).unwrap();
        Run {
            program: std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()).into(),
            args: args.iter().map(OsString::from).collect(),
            out_dir,
            timeout: Duration::from_secs(30),
        }
    }

    /// A windowless `cmd` on a private desktop: its exit code comes back, and nothing is left.
    #[test]
    fn a_run_on_a_private_desktop_returns_its_exit_code() {
        let run = private_run(&["/c", "exit 7"]);
        let desktop = Desktop::create(&format!("xtask-test-{}", std::process::id())).unwrap();
        let job = Job::create().unwrap();
        let child = spawn(&run, &desktop.name, &[], &job).unwrap();
        let desk = Flipping { polls: Cell::new(0), flip_after: u32::MAX, owner: Cell::new(0) };
        assert_eq!(supervise(&child, &job, run.timeout, &desk), Ok(7));
        assert!(job.pids().is_empty());
    }

    /// The watchdog fails a run whose foreground changes to one of its windows, and kills it at once.
    #[test]
    fn the_watchdog_fails_and_kills_a_run_that_takes_the_foreground() {
        let run = private_run(&["/c", "ping -n 30 127.0.0.1 >nul"]);
        let desktop = Desktop::create(&format!("xtask-test-wd-{}", std::process::id())).unwrap();
        let job = Job::create().unwrap();
        let child = spawn(&run, &desktop.name, &[], &job).unwrap();
        let desk = Flipping { polls: Cell::new(0), flip_after: 3, owner: Cell::new(child.pid) };
        let started = Instant::now();
        let ended = supervise(&child, &job, run.timeout, &desk);
        assert!(ended.unwrap_err().starts_with("SAFETY:"));
        assert!(started.elapsed() < Duration::from_secs(10), "killed at once, not after ping's 30 s");
        assert_eq!(unsafe { WaitForSingleObject(child.process, 5_000) }, WAIT_OBJECT_0, "the program was killed");
    }

    #[test]
    fn a_run_past_its_timeout_is_killed_and_fails() {
        let mut run = private_run(&["/c", "ping -n 30 127.0.0.1 >nul"]);
        run.timeout = Duration::from_millis(500);
        let desktop = Desktop::create(&format!("xtask-test-to-{}", std::process::id())).unwrap();
        let job = Job::create().unwrap();
        let child = spawn(&run, &desktop.name, &[], &job).unwrap();
        let desk = Flipping { polls: Cell::new(0), flip_after: u32::MAX, owner: Cell::new(0) };
        assert!(supervise(&child, &job, run.timeout, &desk).unwrap_err().contains("did not end"));
        assert_eq!(unsafe { WaitForSingleObject(child.process, 5_000) }, WAIT_OBJECT_0);
    }
}
