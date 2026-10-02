//! The one WebKit network session and settings object every web view shares. Its data
//! and cache directories are the profile's (`ProfilePaths::engine_data`, `engine_cache`).
//! The settings object carries the preferences for pop-ups, scrolling and the GPU, so
//! changing one reaches every open view at once.

use std::path::PathBuf;
use std::sync::Once;

use gtk::glib;
use vsesvit_core::Profile;
use vsesvit_core::prefs::keys;

#[derive(Clone)]
pub(crate) struct Engine {
    session: webkit::NetworkSession,
    settings: webkit::Settings,
}

/// The AppImage this process runs from: `APPDIR`, which `AppRun` sets, when `exe` is inside it.
/// Another AppImage that starts this browser passes its own `APPDIR` on, which is no business
/// of a deb or rpm build.
fn appimage_dir(appdir: Option<PathBuf>, exe: Option<PathBuf>) -> Option<PathBuf> {
    let appdir = appdir?;
    exe?.starts_with(&appdir).then_some(appdir)
}

/// Lets the web process sandbox see the AppImage this runs from ([`appimage_dir`]): its libraries,
/// schemas and helpers, and the working directory the patched helper paths resolve against
/// (`xtask/src/linux/appimage.rs`), plus the GStreamer registry and pixbuf loader cache `AppRun`
/// writes. WebKit takes sandbox paths only before its first web process, and they hold for the
/// whole process, so the first engine adds them.
fn add_appimage_to_sandbox() {
    static ADDED: Once = Once::new();
    ADDED.call_once(|| {
        let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
        let Some(appdir) = appimage_dir(appdir, std::env::current_exe().ok()) else { return };
        let context = webkit::WebContext::default().expect("WebKit default web context");
        for path in [appdir, glib::user_cache_dir().join("vsesvit/appimage")] {
            context.add_path_to_sandbox(path, true);
        }
    });
}

impl Engine {
    pub(crate) fn new(profile: &mut Profile) -> Self {
        add_appimage_to_sandbox();
        let paths = profile.paths();
        // WebKit takes C strings and silently falls back to its shared default
        // directories when given none, so a non-UTF-8 profile path is refused loudly.
        let data = paths.engine_data.to_str().expect("profile paths are UTF-8");
        let cache = paths.engine_cache.to_str().expect("profile paths are UTF-8");
        let session = webkit::NetworkSession::new(Some(data), Some(cache));
        session.set_tls_errors_policy(webkit::TLSErrorsPolicy::Fail);
        if let Some(data) = session.website_data_manager() {
            data.set_favicons_enabled(true);
        }

        let settings = webkit::Settings::new();
        settings.set_enable_developer_extras(true);
        settings.set_enable_fullscreen(true);
        settings.set_enable_back_forward_navigation_gestures(true);

        let engine = Engine { session, settings };
        engine.apply_prefs(profile);
        engine
    }

    /// Blocking pop-ups only stops windows a page opens on its own; `window.open` from a
    /// click still opens one.
    pub(crate) fn apply_prefs(&self, profile: &mut Profile) {
        let mut prefs = profile.prefs();
        let settings = &self.settings;
        settings.set_javascript_can_open_windows_automatically(!prefs.get(&keys::BLOCK_POPUPS));
        settings.set_enable_smooth_scrolling(prefs.get(&keys::SMOOTH_SCROLLING));
        settings.set_hardware_acceleration_policy(if prefs.get(&keys::HARDWARE_ACCELERATION) {
            webkit::HardwareAccelerationPolicy::Always
        } else {
            webkit::HardwareAccelerationPolicy::Never
        });
    }

    #[cfg(any(test, feature = "self-test"))]
    pub(crate) fn settings(&self) -> &webkit::Settings {
        &self.settings
    }

    pub(crate) fn session(&self) -> &webkit::NetworkSession {
        &self.session
    }

    /// A tab's view. `content` is the extension runtime's manager for that tab, which
    /// carries every loaded extension's content scripts and content blockers.
    pub(crate) fn web_view(&self, content: &webkit::UserContentManager) -> webkit::WebView {
        webkit::WebView::builder()
            .network_session(&self.session)
            .settings(&self.settings)
            .user_content_manager(content)
            .build()
    }

    /// A view for `window.open` and `target=_blank`. WebKit requires it to be related to the
    /// opener, which also gives it the opener's network session and web process.
    pub(crate) fn related_web_view(
        &self,
        opener: &webkit::WebView,
        content: &webkit::UserContentManager,
    ) -> webkit::WebView {
        webkit::WebView::builder()
            .related_view(opener)
            .settings(&self.settings)
            .user_content_manager(content)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_appimage_this_runs_from_reaches_the_sandbox() {
        let dir = |p: &str| Some(PathBuf::from(p));
        let image = "/tmp/.mount_vsesvitAbc";
        assert_eq!(
            appimage_dir(dir(image), dir("/tmp/.mount_vsesvitAbc/usr/lib/vsesvit/vsesvit")),
            dir(image)
        );
        // A deb build that another AppImage started, which passed its APPDIR on.
        let other = "/tmp/.mount_EditorXyz";
        assert_eq!(appimage_dir(dir(other), dir("/usr/lib/vsesvit/vsesvit")), None);
        assert_eq!(appimage_dir(dir("/tmp/.mount_vsesvit"), dir("/tmp/.mount_vsesvitAbc/usr/lib/vsesvit/vsesvit")), None);
        assert_eq!(appimage_dir(None, dir("/usr/lib/vsesvit/vsesvit")), None);
        assert_eq!(appimage_dir(dir(image), None), None, "no executable path, no sandbox paths");
    }
}
