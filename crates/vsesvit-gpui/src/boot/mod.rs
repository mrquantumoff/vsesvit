//! Owned by W1.4.
//! The browser process's boot, steps 1 to 11 of design section 3.1. `main` (or `RunWinMain`) has
//! already returned from any CEF sub-process (step 0). Steps 1 to 8 start no thread, because the
//! Linux sandbox wants a single thread while the engine initializes; the first ones start in step 10.

use std::cell::RefCell;
use std::ffi::OsString;
use std::process::ExitCode;
use std::rc::Rc;

use vsesvit_core::cli::Config;
use vsesvit_engine::{EngineConfig, EngineFactory, EngineInitError, EngineParts};

pub mod cli;
pub mod engine_config;
pub mod extensions;
pub mod headless;
pub mod instance;
pub mod interim;
pub mod logging;
pub mod profile_start;
pub mod start;
pub mod webkit_import;

use crate::app::quit::{self, EndedSlot, Finish, Processes};
use cli::{Launch, RunKind};

pub fn run(factory: &dyn EngineFactory) -> ExitCode {
    logging::init();
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let args = match cli::parse(argv.clone()) {
        Launch::Run(args) => args,
        Launch::Headless(command) => return headless::run(command),
    };
    if let RunKind::SelfTest(out_dir) = &args.run
        && let Err(code) = guard(out_dir)
    {
        return code;
    }
    // The arguments parsed, so every one is Unicode.
    let request = instance::Request::of_this_process(argv.into_iter().filter_map(|arg| arg.into_string().ok()).collect());
    let config = match profile_start::resolve(args) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let mut profile = match profile_start::open(&config, factory.chromium_version(), &request) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    let listener = profile_start::bind(&config);
    let extensions = extensions::metadata_pass(&mut profile);
    let engine_config = engine_config::engine_config(&config, &mut profile);
    let parts = match initialize(factory, engine_config, &crate::platform::fatal_message) {
        Ok(parts) => parts,
        Err(code) => return code,
    };

    let core = Rc::new(RefCell::new(profile));
    let driver: interim::DriverSlot = Rc::new(RefCell::new(Some(parts.driver)));
    let ended = EndedSlot::default();
    let start = start::StartArgs {
        engine: parts.engine,
        events: parts.events,
        driver: driver.clone(),
        ended: ended.clone(),
        listener,
        mode: config.mode.clone(),
        extensions,
    };
    // Only the shutdown sequence ends GPUI's loop, never its last window's close (design section 3.6).
    gpui_platform::application()
        .with_quit_mode(gpui::QuitMode::Explicit)
        .with_assets(crate::theme::icons::VsesvitAssets)
        .run(move |cx| start::start(start, cx));

    let ended = ended.borrow_mut().take();
    let driver = driver.borrow_mut().take();
    finish(ended, driver, core, &config)
}

fn finish(ended: Option<quit::Ended>, driver: Option<Box<dyn vsesvit_engine::EngineDriver>>, core: quit::Core, config: &Config) -> ExitCode {
    quit::finish(
        ended,
        Finish { driver, core, profile_root: &config.profile_dir, home: config.home.as_ref(), spawner: &Processes },
    )
}

/// Step 2a: a self-test runs only inside the launcher's private display or desktop, and exits 3 on
/// any doubt, before anything opens a profile or a window (design section 7.3).
fn guard(out_dir: &std::path::Path) -> Result<(), ExitCode> {
    #[cfg(feature = "self-test")]
    {
        let env = vsesvit_testenv::require();
        log::info!("self-test into {} in {}", out_dir.display(), env.describe());
        Ok(())
    }
    #[cfg(not(feature = "self-test"))]
    {
        eprintln!("vsesvit: --self-test {}: this build has no self-test (feature `self-test`)", out_dir.display());
        Err(ExitCode::FAILURE)
    }
}

/// Step 9. `EarlyExit` makes no further engine call; a failure reaches the user through `fatal`
/// (`platform::fatal_message`: stderr, and a message box in a Windows release build) and exits 1.
fn initialize(factory: &dyn EngineFactory, config: EngineConfig, fatal: &dyn Fn(&str)) -> Result<EngineParts, ExitCode> {
    match factory.initialize(config) {
        Ok(parts) => Ok(parts),
        Err(EngineInitError::EarlyExit(code)) => Err(code),
        Err(EngineInitError::Failed(why)) => {
            log::error!("the engine did not start: {why}");
            fatal(&format!("vsesvit: the browser engine did not start: {why}"));
            Err(ExitCode::FAILURE)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use vsesvit_engine::FakeEngineFactory;

    use super::*;

    struct Failing(EngineInitError);

    impl EngineFactory for Failing {
        fn chromium_version(&self) -> &'static str {
            "154.0.8037.98"
        }
        fn initialize(&self, _: EngineConfig) -> Result<EngineParts, EngineInitError> {
            Err(match &self.0 {
                EngineInitError::Failed(why) => EngineInitError::Failed(why.clone()),
                EngineInitError::EarlyExit(_) => EngineInitError::EarlyExit(ExitCode::from(7)),
            })
        }
    }

    fn config() -> EngineConfig {
        let root = std::env::temp_dir().join(format!("vsesvit-boot-init-{}", std::process::id()));
        let args = vsesvit_core::cli::Args { profile_dir: Some(root.clone()), ..Default::default() };
        let config = match Config::resolve(args) {
            Ok(vsesvit_core::cli::Start::Browse(config)) => config,
            other => panic!("{other:?}"),
        };
        let mut profile = vsesvit_core::Profile::open(&root, vsesvit_core::OpenOptions::default()).unwrap();
        let engine = engine_config::engine_config(&config, &mut profile);
        drop(profile);
        let _ = std::fs::remove_dir_all(&root);
        engine
    }

    #[test]
    fn an_initialization_failure_reaches_the_fatal_message() {
        let shown = RefCell::new(Vec::<String>::new());
        let fatal = |text: &str| shown.borrow_mut().push(text.to_owned());
        let failing = Failing(EngineInitError::Failed("vsesvit-helper is missing next to /opt/vsesvit/vsesvit".into()));
        assert_eq!(initialize(&failing, config(), &fatal).err(), Some(ExitCode::FAILURE));
        assert_eq!(*shown.borrow(), ["vsesvit: the browser engine did not start: vsesvit-helper is missing next to /opt/vsesvit/vsesvit"]);

        let early = Failing(EngineInitError::EarlyExit(ExitCode::SUCCESS));
        assert_eq!(initialize(&early, config(), &fatal).err(), Some(ExitCode::from(7)));
        assert_eq!(shown.borrow().len(), 1, "an early exit shows nothing");

        let fake = FakeEngineFactory::new();
        assert!(initialize(&fake, config(), &fatal).is_ok());
        let given = fake.config().expect("config");
        assert!(given.root_cache_path.ends_with(std::path::Path::new("engine").join("cef")));
        assert!(!given.scripted);
    }
}
