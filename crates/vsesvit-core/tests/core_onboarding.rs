//! First run: fresh-profile detection, the done flag, the recommended extensions table.

use std::path::PathBuf;

use vsesvit_core::crdt::Seq;
use vsesvit_core::extensions::InstallSource;
use vsesvit_core::extensions::crx::CrxStore;
use vsesvit_core::onboarding::{self, RECOMMENDED_EXTENSIONS};
use vsesvit_core::prefs::{Scope, keys};
use vsesvit_core::sync::Kind;
use vsesvit_core::{OpenOptions, Profile};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> TempDir {
    TempDir(std::env::temp_dir().join(format!("vsesvit-onboarding-{}", uuid::Uuid::new_v4())))
}

#[test]
fn a_profile_is_new_only_on_the_open_that_created_it() {
    let dir = tmp();
    let p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    assert!(p.is_new());
    drop(p);
    let p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    assert!(!p.is_new());
}

#[test]
fn shows_on_a_new_profile_until_finished() {
    let dir = tmp();
    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    assert!(onboarding::should_show(&mut p));
    onboarding::finish(&mut p).unwrap();
    assert!(!onboarding::should_show(&mut p));
    onboarding::finish(&mut p).unwrap();
    assert!(p.prefs().get(&keys::ONBOARDING_DONE));
}

#[test]
fn never_shows_on_a_profile_that_already_existed() {
    let dir = tmp();
    drop(Profile::open(&dir.0, OpenOptions::default()).unwrap());
    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    assert!(!p.prefs().get(&keys::ONBOARDING_DONE), "a profile from before the flow has no flag");
    assert!(!onboarding::should_show(&mut p));
}

#[test]
fn the_done_flag_is_local_and_never_exported() {
    assert_eq!(keys::ONBOARDING_DONE.scope, Scope::Local);
    let dir = tmp();
    let mut p = Profile::open(&dir.0, OpenOptions::default()).unwrap();
    onboarding::finish(&mut p).unwrap();
    assert!(p.sync().changes_since(Kind::Prefs, Seq::ZERO, usize::MAX).unwrap().records.is_empty());
}

#[test]
fn recommended_extensions_are_store_installs_with_chrome_style_ids() {
    assert!(!RECOMMENDED_EXTENSIONS.is_empty());
    for r in RECOMMENDED_EXTENSIONS {
        let id = r.id();
        assert!(id.is_chrome_style(), "{}", r.name);
        let expected = match r.store {
            CrxStore::ChromeWebStore => InstallSource::parse(&format!("https://chromewebstore.google.com/detail/{}", r.id)),
            CrxStore::EdgeAddons => InstallSource::parse(&format!("https://microsoftedge.microsoft.com/addons/detail/{}", r.id)),
        };
        assert_eq!(r.install_source(), expected.unwrap(), "{}", r.name);
        assert!(!r.blurb.is_empty() && !r.blurb.contains('\n'), "{}", r.name);
    }
    let ids: std::collections::HashSet<_> = RECOMMENDED_EXTENSIONS.iter().map(|r| r.id).collect();
    assert_eq!(ids.len(), RECOMMENDED_EXTENSIONS.len(), "no duplicates");
}
