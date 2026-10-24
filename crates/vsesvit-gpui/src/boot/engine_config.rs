//! Owned by W1.4.
//! Boot step 8 (design sections 3.1 to 3.3): the engine's config, from the run, the profile's
//! restart-only prefs and, on Linux, the one display scale and the persisted password store. Starts
//! no thread and never touches the vault (section 3.1, "No extra threads before step 9").

use std::path::{Path, PathBuf};

use vsesvit_core::Profile;
use vsesvit_core::cli::{Config, Mode};
use vsesvit_core::prefs::{self, keys};
use vsesvit_engine::{EngineConfig, PasswordStore};

/// The engine's user-data root, `<profile root>/engine/cef` (section 3.2).
pub fn root_cache_path(profile_root: &Path) -> PathBuf {
    profile_root.join("engine").join("cef")
}

pub fn engine_config(config: &Config, profile: &mut Profile) -> EngineConfig {
    let root_cache_path = root_cache_path(&config.profile_dir);
    let mut prefs = profile.prefs();
    let dark = prefs.get(&keys::THEME) == prefs::Theme::Dark;
    EngineConfig {
        log_file: None,
        verbose_log: std::env::var("VSESVIT_CEF_LOG").as_deref() == Ok("verbose"),
        smooth_scrolling: prefs.get(&keys::SMOOTH_SCROLLING),
        hardware_acceleration: prefs.get(&keys::HARDWARE_ACCELERATION),
        user_agent_product: format!("Vsesvit/{}", env!("CARGO_PKG_VERSION")),
        persist_session_cookies: prefs.get(&keys::STARTUP) == prefs::Startup::RestoreSession,
        // The window backgrounds of the two palettes (assets/themes); a page paints over it.
        background_color: if dark { 0xff_22_22_26 } else { 0xff_fa_fa_fb },
        locale: locale(),
        device_scale_factor: device_scale_factor(),
        password_store: password_store(&root_cache_path),
        load_extensions: config.load_extensions.clone(),
        scripted: matches!(config.mode, Mode::SelfTest { .. }),
        extra_switches: Vec::new(),
        root_cache_path,
    }
}

/// Linux: the one scale for GPUI and Chromium. GPUI reads it from its variable, which is set here,
/// before GPUI starts and while this is the only thread.
#[cfg(target_os = "linux")]
fn device_scale_factor() -> Option<f32> {
    use crate::platform::x11::scale;
    let factor = scale::scale_factor();
    // SAFETY: boot step 8 runs before any other thread exists (design section 3.1).
    unsafe { std::env::set_var(scale::GPUI_SCALE_VAR, factor.to_string()) };
    Some(factor)
}

#[cfg(not(target_os = "linux"))]
fn device_scale_factor() -> Option<f32> {
    None
}

/// Linux: `<root_cache_path>/password-store`, written with `basic` at the profile's first GPUI start
/// and never changed afterwards, because a change makes Chromium's encrypted cookies unreadable (Q8).
#[cfg(target_os = "linux")]
fn password_store(root_cache_path: &Path) -> Option<PasswordStore> {
    let file = root_cache_path.join("password-store");
    match std::fs::read_to_string(&file) {
        Ok(text) => match parse_password_store(&text) {
            Some(store) => return Some(store),
            None => log::warn!("{}: {text:?} is not a password store; using basic for this run", file.display()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let written = std::fs::create_dir_all(root_cache_path).and_then(|()| std::fs::write(&file, "basic\n"));
            if let Err(e) = written {
                log::warn!("{}: {e}", file.display());
            }
        }
        Err(e) => log::warn!("{}: {e}; using basic for this run", file.display()),
    }
    Some(PasswordStore::Basic)
}

#[cfg(not(target_os = "linux"))]
fn password_store(_root_cache_path: &Path) -> Option<PasswordStore> {
    None
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_password_store(text: &str) -> Option<PasswordStore> {
    match text.trim() {
        "basic" => Some(PasswordStore::Basic),
        "gnome-libsecret" => Some(PasswordStore::GnomeLibsecret),
        _ => None,
    }
}

/// The UI language as a BCP 47 tag. Linux: the first of `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`
/// that names one (Chromium gets it as `--lang`). Windows: the user's locale.
fn locale() -> String {
    #[cfg(windows)]
    let found = windows_locale();
    #[cfg(not(windows))]
    let found = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().and_then(|value| posix_locale(&value)));
    found.unwrap_or_else(|| "en-US".to_owned())
}

/// `de_DE.UTF-8@euro` → `de-DE`; `C` and `POSIX` name no language. `LANGUAGE` lists several, `:`
/// apart; the first counts.
#[cfg_attr(windows, allow(dead_code))]
fn posix_locale(value: &str) -> Option<String> {
    let first = value.split(':').next()?;
    let tag = first.split(['.', '@']).next()?.trim();
    if tag.is_empty() || tag == "C" || tag == "POSIX" {
        return None;
    }
    Some(tag.replace('_', "-"))
}

#[cfg(windows)]
fn windows_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    // LOCALE_NAME_MAX_LENGTH.
    let mut name = [0u16; 85];
    // SAFETY: the buffer is writable for the length passed.
    let len = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    (len > 1).then(|| String::from_utf16_lossy(&name[..len as usize - 1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_locales_become_language_tags() {
        assert_eq!(posix_locale("de_DE.UTF-8").as_deref(), Some("de-DE"));
        assert_eq!(posix_locale("pt_BR.UTF-8@latin").as_deref(), Some("pt-BR"));
        assert_eq!(posix_locale("uk:en_US").as_deref(), Some("uk"));
        assert_eq!(posix_locale("C.UTF-8"), None);
        assert_eq!(posix_locale("POSIX"), None);
        assert_eq!(posix_locale(""), None);
        assert!(!locale().is_empty());
    }

    #[test]
    fn the_password_store_file_holds_one_word() {
        assert_eq!(parse_password_store("basic\n"), Some(PasswordStore::Basic));
        assert_eq!(parse_password_store(" gnome-libsecret "), Some(PasswordStore::GnomeLibsecret));
        assert_eq!(parse_password_store("kwallet5"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_first_start_writes_basic_and_later_starts_keep_what_is_there() {
        let dir = std::env::temp_dir().join(format!("vsesvit-password-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(password_store(&dir), Some(PasswordStore::Basic));
        assert_eq!(std::fs::read_to_string(dir.join("password-store")).unwrap(), "basic\n");
        std::fs::write(dir.join("password-store"), "gnome-libsecret\n").unwrap();
        assert_eq!(password_store(&dir), Some(PasswordStore::GnomeLibsecret));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_engine_data_sits_under_the_profiles_engine_folder() {
        assert_eq!(root_cache_path(Path::new("/p/Default")), Path::new("/p/Default/engine/cef"));
    }
}
