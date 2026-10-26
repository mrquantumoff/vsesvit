//! Owned by W1.3.
//! `EngineConfig` → `cef::Settings` and the browser process's switch table (design sections 3.2 and
//! 3.3), per OS, plus the Linux helper lookup (section 1.1). The tables are pure functions of the
//! config and the OS, so both OSes' tables are tested on either.

use std::path::{Path, PathBuf};

use vsesvit_engine::{EngineConfig, PasswordStore};

/// The OS a table is for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Os {
    /// Linux (X11).
    Linux,
    /// Windows.
    Windows,
}

impl Os {
    /// The OS this build runs on.
    pub const CURRENT: Os = if cfg!(windows) { Os::Windows } else { Os::Linux };
}

/// One command-line switch, without its `--`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    /// The name.
    pub name: String,
    /// The value, for `--name=value`.
    pub value: Option<String>,
}

impl Switch {
    fn flag(name: &str) -> Switch {
        Switch { name: name.to_owned(), value: None }
    }

    fn with(name: &str, value: impl Into<String>) -> Switch {
        Switch { name: name.to_owned(), value: Some(value.into()) }
    }

    /// Parses `--name=value`, `--name` or `name`.
    pub fn parse(arg: &str) -> Option<Switch> {
        let arg = arg.trim().trim_start_matches("--");
        if arg.is_empty() {
            return None;
        }
        Some(match arg.split_once('=') {
            Some((name, value)) => Switch::with(name, value),
            None => Switch::flag(arg),
        })
    }
}

/// The switches the browser process adds in `on_before_command_line_processing` (design section 3.3),
/// except `--disable-features`, which is merged into CEF's own list by [`disable_features`].
pub fn switches(config: &EngineConfig, os: Os) -> Vec<Switch> {
    let mut out = Vec::new();
    if os == Os::Linux {
        out.push(Switch::with("ozone-platform", "x11"));
        if let Some(scale) = config.device_scale_factor {
            out.push(Switch::with("force-device-scale-factor", scale.to_string()));
        }
        let store = match config.password_store.unwrap_or(PasswordStore::Basic) {
            PasswordStore::Basic => "basic",
            PasswordStore::GnomeLibsecret => "gnome-libsecret",
        };
        out.push(Switch::with("password-store", store));
        out.push(Switch::with("lang", config.locale.clone()));
    }
    out.push(Switch::flag("noerrdialogs"));
    out.push(Switch::flag("disable-gpu-process-crash-limit"));
    if !config.smooth_scrolling {
        out.push(Switch::flag("disable-smooth-scrolling"));
    }
    if !config.hardware_acceleration {
        out.push(Switch::flag("disable-gpu"));
    }
    if !config.load_extensions.is_empty() {
        let dirs: Vec<String> = config.load_extensions.iter().map(|p| p.display().to_string()).collect();
        out.push(Switch::with("load-extension", dirs.join(",")));
    }
    if config.scripted {
        out.push(Switch::flag("use-fake-device-for-media-stream"));
        out.push(Switch::with("auto-select-desktop-capture-source", "Entire screen"));
        out.push(Switch::with("extensions-update-frequency", "86400"));
    }
    out.extend(config.extra_switches.iter().filter_map(|s| Switch::parse(s)));
    out
}

/// CEF's `--disable-features` list with ours appended, never replaced (design section 3.3).
pub fn disable_features(existing: &str, config: &EngineConfig, os: Os) -> String {
    let mut features: Vec<&str> = existing.split(',').map(str::trim).filter(|f| !f.is_empty()).collect();
    if os == Os::Windows && config.scripted {
        // Chromium treats a window on a private desktop as occluded (SW section 14.12).
        features.push("CalculateNativeWinOcclusion");
    }
    let mut seen = std::collections::BTreeSet::new();
    features.retain(|f| seen.insert(*f));
    features.join(",")
}

/// The paths of design section 3.2 that the engine derives from `root_cache_path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// `<root>/engine/cef`.
    pub root_cache_path: PathBuf,
    /// The global context's cache, `<root_cache_path>/Default`: at or below the root, or CEF silently
    /// goes in-memory (API section 3.6).
    pub cache_path: PathBuf,
    /// `<root_cache_path>/cef.log` unless the config names another; without one CEF writes `debug.log`
    /// beside the exe (G15).
    pub log_file: PathBuf,
}

impl Paths {
    /// The paths for a config.
    pub fn of(config: &EngineConfig) -> Paths {
        let root = config.root_cache_path.clone();
        Paths {
            cache_path: root.join("Default"),
            log_file: config.log_file.clone().unwrap_or_else(|| root.join("cef.log")),
            root_cache_path: root,
        }
    }
}

/// Which image CEF's sub-processes run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubprocessImage {
    /// `vsesvit-helper` (Linux).
    Helper(PathBuf),
    /// The browser's own image (`browser_subprocess_path` empty): Windows always, and a Linux debug
    /// build whose helper is missing.
    SameExe,
}

/// Finds the sub-process image (design section 1.1, "Finding the Linux helper"). Linux: `vsesvit-helper`
/// beside the exe; when it is missing a debug build falls back to the same exe and a release build
/// refuses with the message the user sees.
pub fn subprocess_image(os: Os, exe: &Path, exists: impl Fn(&Path) -> bool, release: bool) -> Result<SubprocessImage, String> {
    if os == Os::Windows {
        return Ok(SubprocessImage::SameExe);
    }
    let dir = exe.parent().unwrap_or(Path::new("."));
    let helper = dir.join("vsesvit-helper");
    if exists(&helper) {
        return Ok(SubprocessImage::Helper(helper));
    }
    if release {
        return Err(format!("vsesvit-helper is missing next to {}", exe.display()));
    }
    log::warn!("vsesvit-helper is missing next to {}; CEF's sub-processes run this image", exe.display());
    Ok(SubprocessImage::SameExe)
}

/// `cef::Settings` for a config (design section 3.3).
pub fn cef_settings(config: &EngineConfig, paths: &Paths, image: &SubprocessImage, sandboxed: bool) -> cef::Settings {
    use cef::CefString;
    let path = |p: &Path| CefString::from(p.display().to_string().as_str());
    cef::Settings {
        no_sandbox: if sandboxed || cfg!(target_os = "linux") { 0 } else { 1 },
        external_message_pump: 1,
        multi_threaded_message_loop: 0,
        windowless_rendering_enabled: 0,
        browser_subprocess_path: match image {
            SubprocessImage::Helper(helper) => path(helper),
            SubprocessImage::SameExe => CefString::from(""),
        },
        root_cache_path: path(&paths.root_cache_path),
        cache_path: path(&paths.cache_path),
        log_file: path(&paths.log_file),
        log_severity: if config.verbose_log { cef::LogSeverity::VERBOSE } else { cef::LogSeverity::WARNING },
        persist_session_cookies: config.persist_session_cookies as i32,
        background_color: config.background_color,
        remote_debugging_port: 0,
        user_agent_product: CefString::from(config.user_agent_product.as_str()),
        locale: CefString::from(config.locale.as_str()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> EngineConfig {
        EngineConfig {
            root_cache_path: PathBuf::from("/p/engine/cef"),
            log_file: None,
            verbose_log: false,
            smooth_scrolling: true,
            hardware_acceleration: true,
            user_agent_product: "Vsesvit/1.0".into(),
            persist_session_cookies: false,
            background_color: 0xff20_2020,
            locale: "uk".into(),
            device_scale_factor: None,
            password_store: None,
            load_extensions: Vec::new(),
            extensions: Default::default(),
            scripted: false,
            extra_switches: Vec::new(),
        }
    }

    fn names(switches: &[Switch]) -> Vec<String> {
        switches.iter().map(|s| match &s.value {
            Some(v) => format!("{}={v}", s.name),
            None => s.name.clone(),
        }).collect()
    }

    #[test]
    fn linux_table() {
        let mut c = config();
        c.device_scale_factor = Some(1.5);
        assert_eq!(
            names(&switches(&c, Os::Linux)),
            ["ozone-platform=x11", "force-device-scale-factor=1.5", "password-store=basic", "lang=uk", "noerrdialogs",
                "disable-gpu-process-crash-limit"]
        );
        c.password_store = Some(PasswordStore::GnomeLibsecret);
        assert!(names(&switches(&c, Os::Linux)).contains(&"password-store=gnome-libsecret".to_owned()));
    }

    #[test]
    fn windows_table_has_no_linux_switches() {
        assert_eq!(names(&switches(&config(), Os::Windows)), ["noerrdialogs", "disable-gpu-process-crash-limit"]);
    }

    #[test]
    fn restart_only_prefs_extensions_scripted_and_extra() {
        let mut c = config();
        c.smooth_scrolling = false;
        c.hardware_acceleration = false;
        c.load_extensions = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        c.scripted = true;
        c.extra_switches = vec!["--host-resolver-rules=MAP * 127.0.0.1".into(), "enable-logging".into(), "  ".into()];
        let got = names(&switches(&c, Os::Windows));
        assert_eq!(
            got[2..],
            [
                "disable-smooth-scrolling",
                "disable-gpu",
                "load-extension=/a,/b",
                "use-fake-device-for-media-stream",
                "auto-select-desktop-capture-source=Entire screen",
                "extensions-update-frequency=86400",
                "host-resolver-rules=MAP * 127.0.0.1",
                "enable-logging",
            ]
        );
        assert!(!got.iter().any(|s| s.starts_with("use-fake-ui")), "the shell's prompts are under test");
    }

    #[test]
    fn disable_features_merges_into_cefs_list() {
        let mut c = config();
        assert_eq!(disable_features("A,B", &c, Os::Windows), "A,B");
        c.scripted = true;
        assert_eq!(disable_features("A, B,", &c, Os::Windows), "A,B,CalculateNativeWinOcclusion");
        assert_eq!(disable_features("CalculateNativeWinOcclusion", &c, Os::Windows), "CalculateNativeWinOcclusion");
        assert_eq!(disable_features("", &c, Os::Linux), "");
    }

    #[test]
    fn paths_sit_under_the_root() {
        let p = Paths::of(&config());
        let root = PathBuf::from("/p/engine/cef");
        assert_eq!(p.cache_path, root.join("Default"));
        assert_eq!(p.log_file, root.join("cef.log"));
        let mut c = config();
        c.log_file = Some(PathBuf::from("/elsewhere.log"));
        assert_eq!(Paths::of(&c).log_file, PathBuf::from("/elsewhere.log"));
    }

    #[test]
    fn helper_lookup() {
        let exe = Path::new("/usr/lib/vsesvit/vsesvit");
        let helper = Path::new("/usr/lib/vsesvit").join("vsesvit-helper");
        assert_eq!(subprocess_image(Os::Linux, exe, |p| p == helper, true), Ok(SubprocessImage::Helper(helper.clone())));
        assert_eq!(subprocess_image(Os::Linux, exe, |_| false, false), Ok(SubprocessImage::SameExe));
        assert_eq!(
            subprocess_image(Os::Linux, exe, |_| false, true),
            Err("vsesvit-helper is missing next to /usr/lib/vsesvit/vsesvit".to_owned())
        );
        assert_eq!(subprocess_image(Os::Windows, exe, |_| true, true), Ok(SubprocessImage::SameExe));
    }

    #[test]
    fn settings_follow_the_config() {
        let c = config();
        let s = cef_settings(&c, &Paths::of(&c), &SubprocessImage::SameExe, false);
        assert_eq!(s.external_message_pump, 1);
        assert_eq!(s.multi_threaded_message_loop, 0);
        assert_eq!(s.remote_debugging_port, 0);
        assert_eq!(s.windowless_rendering_enabled, 0);
        assert_eq!(s.browser_subprocess_path.to_string(), "");
        assert_eq!(s.background_color, 0xff20_2020);
        assert_eq!(s.no_sandbox, if cfg!(windows) { 1 } else { 0 });
        let sandboxed = cef_settings(&c, &Paths::of(&c), &SubprocessImage::SameExe, true);
        assert_eq!(sandboxed.no_sandbox, 0);
    }
}
