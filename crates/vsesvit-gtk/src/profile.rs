//! Which profile this process runs on, decided once at startup, the application id that
//! makes one running instance per profile, and starting another profile's process.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::Profile;
use vsesvit_core::profiles::{Home, ProfileId, ProfilesDir, Startup};
use vsesvit_update::Installation;

use crate::APP_ID;
use crate::cli::PROFILE_DIR;

/// The open profile, shared by everything on the UI thread. Borrowed for one call at a
/// time and never across a call into GTK, so a borrow is never re-entered.
pub(crate) type Core = Rc<RefCell<Profile>>;

/// Where the profile lives and how the process announces itself on the session bus.
pub(crate) struct ProfileLocation {
    pub(crate) root: PathBuf,
    pub(crate) app_id: String,
    /// Its place in the profile list; none for a `--profile-dir` outside it.
    pub(crate) home: Option<Home>,
}

/// What this launch runs.
pub(crate) enum Start {
    Profile(ProfileLocation),
    /// Several profiles, none running, and the picker switched on.
    Picker(ProfilesDir),
}

impl Start {
    /// The directory given with `--profile-dir` (created if missing and canonicalized, so two
    /// spellings of one directory share one instance), or what the profile list chooses for a
    /// launch that names no profile: the last used profile, or the picker.
    pub(crate) fn resolve(profile_dir: Option<PathBuf>, has_targets: bool) -> io::Result<Start> {
        let profiles = ProfilesDir::standard();
        let dir = match profile_dir {
            Some(dir) => dir,
            None => {
                profiles.sweep();
                match profiles.startup(has_targets) {
                    Startup::Open(id) => profiles.root(&id),
                    Startup::Picker => return Ok(Start::Picker(profiles)),
                }
            }
        };
        fs::create_dir_all(&dir)?;
        let root = fs::canonicalize(&dir)?;
        let default = profiles.root(&ProfileId::default_profile());
        let home = profiles.locate(&root).map(|id| Home { dir: profiles, id });
        Ok(Start::Profile(ProfileLocation {
            app_id: app_id_for_profile(&root, &default),
            root,
            home,
        }))
    }
}

impl ProfileLocation {
    /// The arguments that bring a restarted browser back to this profile.
    pub(crate) fn relaunch_args(&self) -> Vec<OsString> {
        vec![PROFILE_DIR.into(), self.root.clone().into()]
    }
}

/// The application id of the profile at `root`: the plain one for the standard `Default`
/// profile, however it was named, and one derived from the path for any other, so a second
/// `vsesvit` on the same profile hands its URLs to the first while a different profile runs
/// as its own instance.
fn app_id_for_profile(root: &Path, default: &Path) -> String {
    if fs::canonicalize(default).is_ok_and(|default| default == root) {
        return APP_ID.to_owned();
    }
    hashed_app_id(root)
}

fn hashed_app_id(dir: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let digest =
        glib::compute_checksum_for_data(glib::ChecksumType::Sha256, dir.as_os_str().as_bytes())
            .expect("SHA-256 is always available");
    format!("{APP_ID}.Profile{}", &digest[..16])
}

/// Starts `root`'s profile, or brings its windows forward when its process already runs:
/// the new process finds the profile locked and hands its (empty) command line to the
/// running one. It is launched through the display, so on Wayland it carries an activation
/// token that lets whichever process shows the window take the focus.
pub(crate) fn launch(widget: &impl IsA<gtk::Widget>, root: &Path) -> Result<(), glib::Error> {
    let program =
        program().map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))?;
    let command = [program.as_os_str(), PROFILE_DIR.as_ref(), root.as_os_str()]
        .map(|arg| glib::shell_quote(arg).to_string_lossy().into_owned())
        .join(" ");
    let info = gio::AppInfo::create_from_commandline(
        command,
        Some("Vsesvit"),
        gio::AppInfoCreateFlags::SUPPORTS_STARTUP_NOTIFICATION,
    )?;
    let context = widget.as_ref().display().app_launch_context();
    info.launch(&[], Some(&context))
}

/// The program another profile's process runs: for an AppImage the image itself, whose mount
/// goes away with this process.
fn program() -> io::Result<PathBuf> {
    match Installation::detect() {
        Installation::AppImage { image } => Ok(image),
        _ => std::env::current_exe(),
    }
}

/// Has the process running `root`'s profile look at the profile list again: a second process
/// on the profile hands it an empty command line. Without an activation token, as the point
/// is not to bring it forward.
pub(crate) fn notify(root: &Path) {
    let spawned = program().and_then(|program| {
        std::process::Command::new(program)
            .arg(PROFILE_DIR)
            .arg(root)
            .spawn()
    });
    if let Err(e) = spawned {
        log::warn!(
            "telling the profile at {} to look at the profile list: {e}",
            root.display()
        );
    }
}

pub(crate) fn downloads_dir() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Downloads)
        .unwrap_or_else(|| glib::home_dir().join("Downloads"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_app_ids_are_valid_stable_and_distinct() {
        let a = hashed_app_id(Path::new("/home/u/profile-a"));
        let b = hashed_app_id(Path::new("/home/u/profile-b"));
        assert!(gio::Application::id_is_valid(&a), "{a}");
        assert_eq!(a, hashed_app_id(Path::new("/home/u/profile-a")));
        assert_ne!(a, b);
        assert!(a.starts_with("dev.mrquantumoff.vsesvit.Profile"));
        assert!(gio::Application::id_is_valid(APP_ID));
    }

    #[test]
    fn given_profile_dir_is_created_and_canonical() {
        let base = glib::mkdtemp(glib::tmp_dir().join("vsesvit-profile-XXXXXX")).unwrap();
        let dir = base.join("nested/./profile");
        let Start::Profile(location) = Start::resolve(Some(dir.clone()), false).unwrap() else {
            panic!("a given directory opens");
        };
        assert!(dir.is_dir());
        assert_eq!(location.root, fs::canonicalize(&dir).unwrap());
        assert_eq!(location.app_id, hashed_app_id(&location.root));
        assert!(location.home.is_none(), "outside the profile list");
        assert_eq!(
            location.relaunch_args(),
            [
                OsString::from("--profile-dir"),
                fs::canonicalize(&dir).unwrap().into()
            ]
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_default_profile_uses_the_plain_app_id_however_it_is_named() {
        let base = glib::mkdtemp(glib::tmp_dir().join("vsesvit-profiles-XXXXXX")).unwrap();
        let default = base.join("Default");
        fs::create_dir_all(&default).unwrap();
        let spelled = fs::canonicalize(base.join("./Default")).unwrap();
        assert_eq!(app_id_for_profile(&spelled, &default), APP_ID);
        let other = base.join("Profile 1");
        assert_eq!(app_id_for_profile(&other, &default), hashed_app_id(&other));
        let _ = fs::remove_dir_all(&base);
    }
}
