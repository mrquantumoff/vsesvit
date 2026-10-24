//! Owned by W1.4.
//! Quitting (design section 3.6): the exit intent a quit carries, the shutdown sequence from the
//! trigger to GPUI's return (steps 1 to 7), and what runs after GPUI returned (steps 8 to 11).
//!
//! The sequence is [`Sequence`], a pure state machine fed with the engine's two shutdown events, the
//! final jobs' completions and clock ticks; it answers with the next action. [`request`] starts it
//! inside GPUI, [`user_browsers_closed`] and [`all_browsers_closed`] are what the router calls for
//! the two events, and [`finish`] runs after GPUI's loop: the driver's shutdown, the profile's
//! release, the exit intent and the exit code. A shutdown that reaches the cap exits with
//! [`EXIT_SHUTDOWN_STUCK`] and calls nothing more in the engine.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{App, Global};
use vsesvit_core::Profile;
use vsesvit_core::profiles::Home;
use vsesvit_engine::{Engine, EngineDriver};
use vsesvit_update::Downloaded;

/// The exit code of a shutdown whose browsers did not all close within the cap. Distinct from the
/// self-test's 0 to 4, so the self-test and the updater can tell it apart.
pub const EXIT_SHUTDOWN_STUCK: u8 = 70;
/// Steps 3 to 6 together: from `begin_shutdown` to a live count of zero.
pub const CLOSE_CAP: Duration = Duration::from_secs(15);
/// Each of step 4's jobs (clear on exit, the final sync round), which run side by side.
pub const FINAL_JOB_CAP: Duration = Duration::from_secs(3);
const TICK: Duration = Duration::from_millis(50);

/// What happens once the engine is down and the profile released (step 10).
#[derive(Debug)]
pub enum ExitIntent {
    /// Ctrl+Q, the last window, the end of a self-test.
    Quit,
    /// Removing the current profile: no session save, no final jobs, and its data goes at step 10.
    RemoveProfile,
    /// The same image again; needs no update artifact.
    Relaunch(RelaunchArgs),
    /// "Restart to update".
    ApplyUpdateAndRelaunch(Downloaded),
    /// Windows: quitting with an update ready.
    QuitInstallingUpdate(Downloaded),
}

/// The process a [`ExitIntent::Relaunch`] starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelaunchArgs {
    pub target: RelaunchTarget,
    /// Unpacked extensions for the new session only (`--load-extension`).
    pub load_extensions: Vec<PathBuf>,
}

/// What the relaunched process opens. A self-test phase is a `--self-test` run on the self-test's
/// own profile, because `--phase` belongs to `--self-test`, which refuses `--profile-dir`
/// (design section 7.2; section 3.6's text pairs `--phase` with `--profile-dir`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelaunchTarget {
    Profile(PathBuf),
    SelfTestPhase { out_dir: PathBuf, phase: String },
}

impl RelaunchArgs {
    /// The relaunch of the profile at `root`.
    pub fn profile(root: PathBuf) -> RelaunchArgs {
        RelaunchArgs { target: RelaunchTarget::Profile(root), load_extensions: Vec::new() }
    }

    /// The new process's arguments, without the program name.
    pub fn command_line(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = match &self.target {
            RelaunchTarget::Profile(root) => vec!["--profile-dir".into(), root.into()],
            RelaunchTarget::SelfTestPhase { out_dir, phase } => {
                vec!["--self-test".into(), out_dir.into(), "--phase".into(), phase.into()]
            }
        };
        for dir in &self.load_extensions {
            args.push("--load-extension".into());
            args.push(dir.into());
        }
        args
    }
}

/// Starts processes. The real one is [`Processes`]; tests record instead.
pub trait Spawner {
    fn spawn(&self, image: &Path, args: &[OsString]) -> std::io::Result<()>;
}

/// [`Spawner`] that starts the process.
pub struct Processes;

impl Spawner for Processes {
    fn spawn(&self, image: &Path, args: &[OsString]) -> std::io::Result<()> {
        std::process::Command::new(image).args(args).spawn().map(drop)
    }
}

/// The image a relaunch starts: an AppImage's own file (`$APPIMAGE`), otherwise this executable,
/// which in a sandboxed Windows package is the renamed bootstrap.
pub fn relaunch_image() -> std::io::Result<PathBuf> {
    image_of(std::env::var_os("APPIMAGE"), std::env::current_exe())
}

fn image_of(appimage: Option<OsString>, exe: std::io::Result<PathBuf>) -> std::io::Result<PathBuf> {
    match appimage.filter(|path| cfg!(target_os = "linux") && !path.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => exe,
    }
}

/// Starts `args` with the relaunch image. Failures are logged; the caller's exit code stands.
pub fn relaunch(args: &RelaunchArgs, spawner: &dyn Spawner) {
    let started = relaunch_image().and_then(|image| {
        log::info!("relaunching {} {:?}", image.display(), args.command_line());
        spawner.spawn(&image, &args.command_line())
    });
    if let Err(e) = started {
        log::error!("the relaunch did not start: {e}");
    }
}

/// How the sequence ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// The live count reached zero: `cef::shutdown` may run.
    Drained,
    /// The cap passed with browsers alive: no further engine call.
    Stuck,
}

/// What the sequence is told.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Input {
    UserBrowsersClosed,
    AllBrowsersClosed,
    ClearDone,
    SyncDone,
    /// Time passed; the caps are checked.
    Tick,
}

/// What the sequence asks for next.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Step 4: clear on exit and the final sync round, side by side.
    RunFinalJobs,
    /// Step 5: `engine.close_internal()`.
    CloseInternal,
    /// Step 7: stop the pump, quit GPUI.
    End(End),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Stage {
    /// Step 3: the user's browsers are closing.
    ClosingUser,
    /// Step 4.
    FinalJobs { since: Instant, clear: bool, sync: bool },
    /// Steps 5 and 6: the hidden browsers are closing.
    ClosingInternal,
    Ended(End),
}

/// Steps 3 to 7 of design section 3.6, from the moment `begin_shutdown` was called.
#[derive(Debug)]
pub struct Sequence {
    began: Instant,
    final_jobs: bool,
    stage: Stage,
}

impl Sequence {
    /// The sequence after `begin_shutdown` at `now`. `final_jobs` is false for a removed profile,
    /// whose clear on exit and final sync are skipped.
    pub fn begin(now: Instant, final_jobs: bool) -> Sequence {
        Sequence { began: now, final_jobs, stage: Stage::ClosingUser }
    }

    pub fn ended(&self) -> Option<End> {
        match self.stage {
            Stage::Ended(end) => Some(end),
            _ => None,
        }
    }

    /// Takes `input` at `now` and returns the actions now due, in order.
    pub fn feed(&mut self, input: Input, now: Instant) -> Vec<Action> {
        let mut due = Vec::new();
        if self.ended().is_some() {
            return due;
        }
        match (self.stage, input) {
            (_, Input::AllBrowsersClosed) => self.stage = Stage::Ended(End::Drained),
            (Stage::ClosingUser, Input::UserBrowsersClosed) if self.final_jobs => {
                self.stage = Stage::FinalJobs { since: now, clear: false, sync: false };
                due.push(Action::RunFinalJobs);
            }
            (Stage::ClosingUser, Input::UserBrowsersClosed) => {
                self.stage = Stage::ClosingInternal;
                due.push(Action::CloseInternal);
            }
            (Stage::FinalJobs { since, clear, sync }, Input::ClearDone | Input::SyncDone | Input::Tick) => {
                let clear = clear || input == Input::ClearDone;
                let sync = sync || input == Input::SyncDone;
                if (clear && sync) || now >= since + FINAL_JOB_CAP {
                    if !(clear && sync) {
                        log::warn!("shutdown: the final jobs ran past {FINAL_JOB_CAP:?} (clear done: {clear}, sync done: {sync})");
                    }
                    self.stage = Stage::ClosingInternal;
                    due.push(Action::CloseInternal);
                } else {
                    self.stage = Stage::FinalJobs { since, clear, sync };
                }
            }
            _ => {}
        }
        if self.ended().is_none() && now >= self.began + CLOSE_CAP {
            self.stage = Stage::Ended(End::Stuck);
        }
        if let Some(end) = self.ended() {
            due.push(Action::End(end));
        }
        due
    }
}

/// What the shell does at steps 2 and 3, for the sequence. The `Shell`'s state implements it.
pub trait ShutdownShell {
    /// Step 2: save the session now (normal windows only). Not called for a removed profile.
    fn save_session(&self, cx: &mut App);
    /// Step 3, just before the engine closes the user's browsers.
    fn hide_windows(&self, cx: &mut App);
}

/// How GPUI's loop ended, for [`finish`].
#[derive(Debug)]
pub struct Ended {
    pub end: End,
    pub intent: ExitIntent,
    /// The process's exit code when the engine drained: 0, or the self-test's verdict.
    pub code: u8,
}

/// Where [`finish`] finds how the sequence ended; boot creates it before GPUI runs.
pub type EndedSlot = Rc<RefCell<Option<Ended>>>;

/// What the sequence needs from the running app; boot installs it once the engine runs.
pub struct QuitParts {
    pub engine: Rc<dyn Engine>,
    pub shell: Rc<dyn ShutdownShell>,
    /// Step 7: ends the pump task after its current step. The driver stays in boot's slot.
    pub stop_pump: Box<dyn FnOnce()>,
    pub ended: EndedSlot,
}

struct Running {
    sequence: Sequence,
    intent: ExitIntent,
    code: u8,
}

struct Quitter {
    engine: Rc<dyn Engine>,
    shell: Rc<dyn ShutdownShell>,
    stop_pump: Option<Box<dyn FnOnce()>>,
    ended: EndedSlot,
    running: Option<Running>,
}

impl Global for Quitter {}

/// Installs the sequence's parts. Once, after the engine started.
pub fn install(parts: QuitParts, cx: &mut App) {
    cx.set_global(Quitter {
        engine: parts.engine,
        shell: parts.shell,
        stop_pump: Some(parts.stop_pump),
        ended: parts.ended,
        running: None,
    });
}

/// Whether a quit is under way: new windows and tabs are not opened then.
pub fn is_quitting(cx: &App) -> bool {
    cx.try_global::<Quitter>().is_some_and(|q| q.running.is_some())
}

/// Step 1: quit with `intent` and exit code 0. A quit already under way keeps its own intent.
pub fn request(intent: ExitIntent, cx: &mut App) {
    request_with_code(intent, 0, cx)
}

/// [`request`] with the exit code a drained run returns, for the end of a self-test.
pub fn request_with_code(intent: ExitIntent, code: u8, cx: &mut App) {
    let Some(quitter) = cx.try_global::<Quitter>() else {
        log::error!("quit requested before the shell started; quitting GPUI at once");
        cx.quit();
        return;
    };
    if let Some(running) = &quitter.running {
        log::info!("quit requested ({intent:?}) while quitting ({:?}); the first one stands", running.intent);
        return;
    }
    let (engine, shell) = (quitter.engine.clone(), quitter.shell.clone());
    let final_jobs = !matches!(intent, ExitIntent::RemoveProfile);
    log::info!("quitting: {intent:?}");
    if final_jobs {
        shell.save_session(cx);
    }
    shell.hide_windows(cx);
    let now = cx.background_executor().now();
    cx.global_mut::<Quitter>().running = Some(Running { sequence: Sequence::begin(now, final_jobs), intent, code });
    engine.begin_shutdown();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(TICK).await;
            if cx.update(|cx| feed(Input::Tick, cx)).is_some() {
                break;
            }
        }
    })
    .detach();
}

/// The router's `BrowserEvent::UserBrowsersClosed` (step 3 done).
pub fn user_browsers_closed(cx: &mut App) {
    feed(Input::UserBrowsersClosed, cx);
}

/// The router's `BrowserEvent::AllBrowsersClosed` (step 6 done).
pub fn all_browsers_closed(cx: &mut App) {
    feed(Input::AllBrowsersClosed, cx);
}

/// Feeds the sequence and carries out its actions; the sequence's end, once there is one.
fn feed(input: Input, cx: &mut App) -> Option<End> {
    let now = cx.background_executor().now();
    let quitter = cx.try_global::<Quitter>()?;
    let engine = quitter.engine.clone();
    let actions = cx.global_mut::<Quitter>().running.as_mut()?.sequence.feed(input, now);
    for action in actions {
        match action {
            Action::RunFinalJobs => run_final_jobs(cx),
            Action::CloseInternal => engine.close_internal(),
            Action::End(end) => stop(end, cx),
        }
    }
    cx.global::<Quitter>().running.as_ref()?.sequence.ended()
}

fn run_final_jobs(cx: &mut App) {
    let clear = crate::privacy::site_data::clear_on_exit(cx);
    let sync = crate::sync::final_round(cx);
    cx.spawn(async move |cx| {
        clear.await;
        cx.update(|cx| feed(Input::ClearDone, cx));
    })
    .detach();
    cx.spawn(async move |cx| {
        sync.await;
        cx.update(|cx| feed(Input::SyncDone, cx));
    })
    .detach();
}

/// Step 7, or the cap: the pump stops after its current step and GPUI's loop ends.
fn stop(end: End, cx: &mut App) {
    match end {
        End::Drained => log::info!("shutdown: every browser closed"),
        End::Stuck => log::error!("shutdown: browsers still alive after {CLOSE_CAP:?}"),
    }
    let quitter = cx.global_mut::<Quitter>();
    if let Some(stop_pump) = quitter.stop_pump.take() {
        stop_pump();
    }
    if let Some(running) = quitter.running.as_mut() {
        let intent = std::mem::replace(&mut running.intent, ExitIntent::Quit);
        *quitter.ended.borrow_mut() = Some(Ended { end, intent, code: running.code });
    }
    cx.quit();
}

/// The profile after GPUI returned.
pub type Core = Rc<RefCell<Profile>>;

/// What [`finish`] needs besides how the loop ended.
pub struct Finish<'a> {
    pub driver: Option<Box<dyn EngineDriver>>,
    pub core: Core,
    pub profile_root: &'a Path,
    pub home: Option<&'a Home>,
    pub spawner: &'a dyn Spawner,
}

/// Steps 8 to 11, after GPUI's loop returned. A stuck shutdown never returns: it releases the
/// profile, flushes the log and exits with [`EXIT_SHUTDOWN_STUCK`].
pub fn finish(ended: Option<Ended>, parts: Finish<'_>) -> ExitCode {
    let ended = ended.unwrap_or_else(|| {
        log::error!("GPUI's loop ended without the shutdown sequence");
        Ended { end: End::Stuck, intent: ExitIntent::Quit, code: 1 }
    });
    // The driver itself checks that the engine drained: if not, it logs every live browser and
    // makes no engine call (`cef::shutdown` included).
    if let Some(driver) = parts.driver {
        driver.shutdown();
    }
    release(parts.core);
    if ended.end == End::Stuck {
        log::error!("exiting with {EXIT_SHUTDOWN_STUCK}; the exit intent {:?} waits for the next start", ended.intent);
        log::logger().flush();
        std::process::exit(i32::from(EXIT_SHUTDOWN_STUCK));
    }
    carry_out(ended.intent, parts.profile_root, parts.home, parts.spawner);
    log::logger().flush();
    ExitCode::from(ended.code)
}

/// Step 9: drops the profile, which frees its lock if this was the last strong reference.
fn release(core: Core) {
    match Rc::try_unwrap(core) {
        Ok(profile) => drop(profile.into_inner()),
        Err(core) => log::error!(
            "{} other references still hold the profile; its lock stays until the process exits",
            Rc::strong_count(&core) - 1
        ),
    }
}

/// Step 10.
fn carry_out(intent: ExitIntent, profile_root: &Path, home: Option<&Home>, spawner: &dyn Spawner) {
    match intent {
        ExitIntent::Quit => {}
        ExitIntent::RemoveProfile => match home {
            Some(home) if home.dir.load().is_removed(&home.id) => {
                log::info!("deleting the removed profile {}", profile_root.display());
                home.dir.sweep();
            }
            _ => log::error!("asked to remove {}, which its list does not mark removed", profile_root.display()),
        },
        ExitIntent::Relaunch(args) => relaunch(&args, spawner),
        ExitIntent::ApplyUpdateAndRelaunch(downloaded) => {
            let args = RelaunchArgs::profile(profile_root.to_owned());
            crate::updates::lifecycle::install_and_relaunch(downloaded, &args, spawner);
        }
        ExitIntent::QuitInstallingUpdate(downloaded) => crate::updates::lifecycle::install_on_exit(downloaded),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use gpui::TestAppContext;
    use vsesvit_engine::{
        BrowserEvent, Browsing, FakeCommand, FakeEngine, FakePhase, InitialPage, NativeHandle, PageFocus, PhysRect, TabId,
        TabKind, TabSpec, Url,
    };

    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn the_steps_run_in_order() {
        let t = Instant::now();
        let mut s = Sequence::begin(t, true);
        assert!(s.feed(Input::Tick, at(t, 100)).is_empty());
        assert!(s.feed(Input::AllBrowsersClosed, at(t, 200)).contains(&Action::End(End::Drained)), "a zero count ends it in any stage");
        let mut s = Sequence::begin(t, true);
        assert_eq!(s.feed(Input::UserBrowsersClosed, at(t, 100)), [Action::RunFinalJobs]);
        assert!(s.feed(Input::ClearDone, at(t, 200)).is_empty(), "the sync still runs");
        assert_eq!(s.feed(Input::SyncDone, at(t, 300)), [Action::CloseInternal]);
        assert!(s.feed(Input::UserBrowsersClosed, at(t, 350)).is_empty());
        assert_eq!(s.feed(Input::AllBrowsersClosed, at(t, 400)), [Action::End(End::Drained)]);
        assert_eq!(s.ended(), Some(End::Drained));
        assert!(s.feed(Input::Tick, at(t, 60_000)).is_empty(), "nothing after the end");
    }

    #[test]
    fn a_removed_profile_skips_the_final_jobs() {
        let t = Instant::now();
        let mut s = Sequence::begin(t, false);
        assert_eq!(s.feed(Input::UserBrowsersClosed, at(t, 10)), [Action::CloseInternal]);
    }

    #[test]
    fn each_final_job_is_capped() {
        let t = Instant::now();
        let mut s = Sequence::begin(t, true);
        s.feed(Input::UserBrowsersClosed, at(t, 1_000));
        s.feed(Input::SyncDone, at(t, 1_100));
        assert!(s.feed(Input::Tick, at(t, 3_999)).is_empty());
        assert_eq!(s.feed(Input::Tick, at(t, 4_000)), [Action::CloseInternal]);
        assert!(s.feed(Input::ClearDone, at(t, 4_100)).is_empty(), "a late job changes nothing");
    }

    #[test]
    fn a_close_under_the_cap_drains_and_one_past_it_is_stuck() {
        let t = Instant::now();
        let mut delayed = Sequence::begin(t, true);
        delayed.feed(Input::Tick, at(t, 7_900));
        assert_eq!(delayed.feed(Input::UserBrowsersClosed, at(t, 8_000)), [Action::RunFinalJobs]);
        delayed.feed(Input::ClearDone, at(t, 8_010));
        delayed.feed(Input::SyncDone, at(t, 8_020));
        assert_eq!(delayed.feed(Input::AllBrowsersClosed, at(t, 8_100)), [Action::End(End::Drained)]);

        let mut pinned = Sequence::begin(t, true);
        assert!(pinned.feed(Input::Tick, at(t, 14_999)).is_empty());
        assert_eq!(pinned.feed(Input::Tick, at(t, 15_000)), [Action::End(End::Stuck)]);
        assert!(pinned.feed(Input::AllBrowsersClosed, at(t, 15_100)).is_empty(), "too late");
        assert_eq!(pinned.ended(), Some(End::Stuck));

        let mut internal_pinned = Sequence::begin(t, false);
        internal_pinned.feed(Input::UserBrowsersClosed, at(t, 10));
        assert_eq!(internal_pinned.feed(Input::Tick, at(t, 15_000)), [Action::End(End::Stuck)], "the cap counts from step 3");
    }

    #[test]
    fn relaunch_arguments() {
        let profile = RelaunchArgs { target: RelaunchTarget::Profile("/p/Default".into()), load_extensions: vec!["/ext".into()] };
        assert_eq!(profile.command_line(), ["--profile-dir", "/p/Default", "--load-extension", "/ext"].map(OsString::from));
        let phase = RelaunchArgs {
            target: RelaunchTarget::SelfTestPhase { out_dir: "/out".into(), phase: "shutdown-stuck".into() },
            load_extensions: Vec::new(),
        };
        assert_eq!(phase.command_line(), ["--self-test", "/out", "--phase", "shutdown-stuck"].map(OsString::from));
        let parsed = vsesvit_core::cli::parse(vsesvit_core::cli::Shell::Gpui, phase.command_line());
        assert!(matches!(parsed, Ok(vsesvit_core::cli::Parsed::Run(args)) if args.phase.as_deref() == Some("shutdown-stuck")));
        let parsed = vsesvit_core::cli::parse(vsesvit_core::cli::Shell::Gpui, profile.command_line());
        assert!(matches!(parsed, Ok(vsesvit_core::cli::Parsed::Run(args)) if args.load_extensions.len() == 1));
    }

    #[test]
    fn an_appimage_relaunches_its_own_file() {
        let exe = || Ok(PathBuf::from("/tmp/.mount_x/usr/bin/vsesvit"));
        let image = image_of(Some("/home/u/Vsesvit.AppImage".into()), exe()).unwrap();
        if cfg!(target_os = "linux") {
            assert_eq!(image, Path::new("/home/u/Vsesvit.AppImage"));
        } else {
            assert_eq!(image, Path::new("/tmp/.mount_x/usr/bin/vsesvit"), "APPIMAGE means nothing off Linux");
        }
        assert_eq!(image_of(Some(OsString::new()), exe()).unwrap(), Path::new("/tmp/.mount_x/usr/bin/vsesvit"));
        assert_eq!(image_of(None, exe()).unwrap(), Path::new("/tmp/.mount_x/usr/bin/vsesvit"));
    }

    #[derive(Default)]
    struct Recorder(RefCell<Vec<(PathBuf, Vec<OsString>)>>);

    impl Spawner for Recorder {
        fn spawn(&self, image: &Path, args: &[OsString]) -> std::io::Result<()> {
            self.0.borrow_mut().push((image.to_owned(), args.to_vec()));
            Ok(())
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vsesvit-quit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn open(root: &Path) -> Core {
        Rc::new(RefCell::new(Profile::open(root, vsesvit_core::OpenOptions::default()).expect("profile")))
    }

    #[test]
    fn a_relaunch_with_no_update_starts_this_image_after_the_profile_is_released() {
        let root = scratch("relaunch");
        let recorder = Recorder::default();
        let args = RelaunchArgs::profile(root.clone());
        let ended = Ended { end: End::Drained, intent: ExitIntent::Relaunch(args.clone()), code: 0 };
        let code =
            finish(Some(ended), Finish { driver: None, core: open(&root), profile_root: &root, home: None, spawner: &recorder });
        assert_eq!(code, ExitCode::SUCCESS);
        let spawned = recorder.0.borrow().clone();
        assert_eq!(spawned, [(std::env::current_exe().unwrap(), args.command_line())]);
        assert!(Profile::open(&root, vsesvit_core::OpenOptions::default()).is_ok(), "the lock is free");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_drained_finish_shuts_the_driver_down_and_returns_the_code() {
        let root = scratch("drained");
        let (engine, _events) = FakeEngine::new();
        let recorder = Recorder::default();
        let ended = Ended { end: End::Drained, intent: ExitIntent::Quit, code: 1 };
        let parts = Finish { driver: Some(engine.driver()), core: open(&root), profile_root: &root, home: None, spawner: &recorder };
        assert_eq!(finish(Some(ended), parts), ExitCode::from(1));
        assert!(engine.driver_shut_down());
        assert!(recorder.0.borrow().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    struct Shell {
        saved: Cell<u32>,
        hidden: Cell<u32>,
    }

    impl ShutdownShell for Shell {
        fn save_session(&self, _: &mut App) {
            self.saved.set(self.saved.get() + 1);
        }
        fn hide_windows(&self, _: &mut App) {
            self.hidden.set(self.hidden.get() + 1);
        }
    }

    struct Harness {
        engine: Rc<FakeEngine>,
        shell: Rc<Shell>,
        stopped: Rc<Cell<bool>>,
        ended: EndedSlot,
    }

    fn start(cx: &mut TestAppContext) -> Harness {
        let (engine, mut events) = FakeEngine::new();
        let shell = Rc::new(Shell { saved: Cell::new(0), hidden: Cell::new(0) });
        let stopped = Rc::new(Cell::new(false));
        let ended = EndedSlot::default();
        let parts = QuitParts {
            engine: engine.clone(),
            shell: shell.clone(),
            stop_pump: Box::new({
                let stopped = stopped.clone();
                move || stopped.set(true)
            }),
            ended: ended.clone(),
        };
        cx.update(|cx| {
            install(parts, cx);
            engine.set_hidden_host(NativeHandle::from_raw(1));
            cx.spawn(async move |cx| {
                use futures::StreamExt as _;
                while let Some(event) = events.next().await {
                    cx.update(|cx| match event {
                        BrowserEvent::UserBrowsersClosed => user_browsers_closed(cx),
                        BrowserEvent::AllBrowsersClosed => all_browsers_closed(cx),
                        _ => {}
                    });
                }
            })
            .detach();
        });
        Harness { engine, shell, stopped, ended }
    }

    fn create(engine: &FakeEngine, tab: TabId) {
        engine.set_page_focus(tab, PageFocus::NEW_BROWSER);
        let spec = TabSpec {
            tab,
            kind: TabKind::Page,
            browsing: Browsing::Normal,
            parent: NativeHandle::from_raw(2),
            bounds: PhysRect::default(),
            scale_factor: 1.0,
            visible: true,
            page: InitialPage::Url(Url::parse("about:blank").unwrap()),
            opener: None,
        };
        engine.create_tab(spec).expect("created");
    }

    fn open_tab(engine: &FakeEngine) -> TabId {
        let tab = TabId::next();
        create(engine, tab);
        tab
    }

    #[gpui::test]
    fn a_quit_closes_the_user_browsers_then_the_hidden_ones(cx: &mut TestAppContext) {
        let h = start(cx);
        let tab = open_tab(&h.engine);
        h.engine.hold_close(tab);
        cx.update(|cx| request(ExitIntent::Quit, cx));
        assert_eq!((h.shell.saved.get(), h.shell.hidden.get()), (1, 1));
        assert_eq!(h.engine.phase(), FakePhase::ClosingUser);
        cx.executor().advance_clock(Duration::from_secs(8));
        cx.run_until_parked();
        assert_eq!(h.engine.phase(), FakePhase::ClosingUser, "the maintenance browser waits for the user's tab");
        assert_eq!(h.engine.live_browsers(), 2);
        h.engine.complete_close(tab);
        cx.run_until_parked();
        assert_eq!(h.engine.phase(), FakePhase::Drained);
        assert!(h.stopped.get());
        let ended = h.ended.borrow_mut().take().expect("ended");
        assert_eq!((ended.end, ended.code), (End::Drained, 0));
        assert!(matches!(ended.intent, ExitIntent::Quit));
        cx.update(|cx| request(ExitIntent::RemoveProfile, cx));
        assert_eq!(h.shell.hidden.get(), 1, "the first quit stands");
    }

    #[gpui::test]
    fn a_browser_that_never_closes_ends_stuck_at_the_cap(cx: &mut TestAppContext) {
        let h = start(cx);
        let tab = open_tab(&h.engine);
        h.engine.hold_close(tab);
        cx.update(|cx| request_with_code(ExitIntent::RemoveProfile, 1, cx));
        assert_eq!(h.shell.saved.get(), 0, "a removed profile's session is not saved");
        cx.executor().advance_clock(CLOSE_CAP - Duration::from_millis(100));
        cx.run_until_parked();
        assert!(h.ended.borrow().is_none());
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        let ended = h.ended.borrow_mut().take().expect("ended");
        assert_eq!(ended.end, End::Stuck);
        assert!(matches!(ended.intent, ExitIntent::RemoveProfile));
        assert!(h.stopped.get());
        assert_eq!(h.engine.phase(), FakePhase::ClosingUser, "close_internal never ran");
    }

    #[gpui::test]
    fn a_creation_completed_after_begin_shutdown_is_closed_and_the_quit_drains(cx: &mut TestAppContext) {
        let h = start(cx);
        let tab = TabId::next();
        h.engine.hold_creation(tab);
        create(&h.engine, tab);
        cx.update(|cx| request(ExitIntent::Quit, cx));
        cx.run_until_parked();
        assert!(h.ended.borrow().is_none(), "the held creation is still counted");
        h.engine.complete_creation(tab);
        cx.run_until_parked();
        assert_eq!(h.ended.borrow().as_ref().map(|e| e.end), Some(End::Drained));
        assert!(h.engine.tab(tab).is_none(), "closed at once");
    }

    #[gpui::test]
    fn the_maintenance_browser_lives_through_the_final_jobs(cx: &mut TestAppContext) {
        let h = start(cx);
        let tab = open_tab(&h.engine);
        cx.update(|cx| request(ExitIntent::Quit, cx));
        cx.run_until_parked();
        assert!(h.engine.tab(tab).is_none());
        assert_eq!(h.engine.phase(), FakePhase::Drained);
        let closes: Vec<_> = h
            .engine
            .commands()
            .into_iter()
            .filter(|c| matches!(c, FakeCommand::BeginShutdown | FakeCommand::CloseInternal))
            .collect();
        assert_eq!(closes, [FakeCommand::BeginShutdown, FakeCommand::CloseInternal]);
    }
}
