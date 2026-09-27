//! Which profile this process runs on, decided once at startup, and the application id
//! that makes one running instance per profile.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::glib;
use vsesvit_core::Profile;

use crate::APP_ID;
use crate::cli::PROFILE_DIR;

/// The open profile, shared by everything on the UI thread. Borrowed for one call at a
/// time and never across a call into GTK, so a borrow is never re-entered.
pub(crate) type Core = Rc<RefCell<Profile>>;

pub(crate) const DEFAULT_PROFILE: &str = "Default";

/// Where the profile lives and how the process announces itself on the session bus.
pub(crate) struct ProfileLocation {
    pub(crate) root: PathBuf,
    pub(crate) app_id: String,
    /// Whether `--profile-dir` chose the root, which a relaunch has to repeat.
    given: bool,
}

impl ProfileLocation {
    /// The default profile, or the directory given with `--profile-dir` (created if
    /// missing and canonicalized, so two spellings of one directory share one instance).
    pub(crate) fn resolve(profile_dir: Option<PathBuf>) -> io::Result<ProfileLocation> {
        Ok(match profile_dir {
            None => ProfileLocation {
                root: Profile::default_root(DEFAULT_PROFILE),
                app_id: APP_ID.to_owned(),
                given: false,
            },
            Some(dir) => {
                fs::create_dir_all(&dir)?;
                let root = fs::canonicalize(&dir)?;
                let app_id = app_id_for_profile(&root);
                ProfileLocation {
                    root,
                    app_id,
                    given: true,
                }
            }
        })
    }

    /// The arguments that bring a restarted browser back to this profile.
    pub(crate) fn relaunch_args(&self) -> Vec<OsString> {
        if self.given {
            vec![PROFILE_DIR.into(), self.root.clone().into()]
        } else {
            Vec::new()
        }
    }
}

/// A profile-specific application id: a second `vsesvit` on the same profile hands its
/// URLs to the first, while a different profile runs as its own instance.
fn app_id_for_profile(dir: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let digest =
        glib::compute_checksum_for_data(glib::ChecksumType::Sha256, dir.as_os_str().as_bytes())
            .expect("SHA-256 is always available");
    format!("{APP_ID}.Profile{}", &digest[..16])
}

pub(crate) fn downloads_dir() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Downloads)
        .unwrap_or_else(|| glib::home_dir().join("Downloads"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gio;

    #[test]
    fn profile_app_ids_are_valid_stable_and_distinct() {
        let a = app_id_for_profile(Path::new("/home/u/profile-a"));
        let b = app_id_for_profile(Path::new("/home/u/profile-b"));
        assert!(gio::Application::id_is_valid(&a), "{a}");
        assert_eq!(a, app_id_for_profile(Path::new("/home/u/profile-a")));
        assert_ne!(a, b);
        assert!(a.starts_with("dev.mrquantumoff.vsesvit.Profile"));
        assert!(gio::Application::id_is_valid(APP_ID));
    }

    #[test]
    fn given_profile_dir_is_created_and_canonical() {
        let base = glib::mkdtemp(glib::tmp_dir().join("vsesvit-profile-XXXXXX")).unwrap();
        let dir = base.join("nested/./profile");
        let location = ProfileLocation::resolve(Some(dir.clone())).unwrap();
        assert!(dir.is_dir());
        assert_eq!(location.root, fs::canonicalize(&dir).unwrap());
        assert_eq!(location.app_id, app_id_for_profile(&location.root));
        assert_ne!(location.app_id, APP_ID);
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
    fn default_profile_uses_the_plain_app_id() {
        let location = ProfileLocation::resolve(None).unwrap();
        assert_eq!(location.app_id, APP_ID);
        assert_eq!(location.root, Profile::default_root(DEFAULT_PROFILE));
        assert!(location.relaunch_args().is_empty());
    }
}
