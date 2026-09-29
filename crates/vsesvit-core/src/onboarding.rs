//! The first-run welcome: choose a search engine, add recommended extensions, import
//! bookmarks, make Vsesvit the default browser. The shells own the flow; core decides
//! whether it runs and what it recommends.
//!
//! It runs once per profile, on the launch that created it. A profile that already
//! existed (including one made before this flow did) never sees it, and quitting midway
//! does not bring it back, as with Chrome's first run.

use crate::extensions::crx::CrxStore;
use crate::extensions::{ExtensionId, InstallSource};
use crate::prefs::keys::ONBOARDING_DONE;
use crate::{Error, Profile};

pub fn should_show(profile: &mut Profile) -> bool {
    profile.is_new() && !profile.prefs().get(&ONBOARDING_DONE)
}

/// Idempotent.
pub fn finish(profile: &mut Profile) -> Result<(), Error> {
    profile.prefs().set(&ONBOARDING_DONE, &true)
}

/// An extension the welcome flow offers, from a store whose publisher signature the
/// install pipeline checks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Recommended {
    pub name: &'static str,
    pub blurb: &'static str,
    pub store: CrxStore,
    /// A Chrome-style id in `store`.
    pub id: &'static str,
}

impl Recommended {
    pub fn id(&self) -> ExtensionId {
        ExtensionId::parse(self.id).expect("RECOMMENDED_EXTENSIONS ids are valid")
    }

    pub fn install_source(&self) -> InstallSource {
        InstallSource::web_store(self.store, self.id())
    }
}

pub const RECOMMENDED_EXTENSIONS: &[Recommended] = &[
    Recommended {
        name: "uBlock Origin Lite",
        blurb: "Blocks ads and trackers with declarativeNetRequest.",
        store: CrxStore::ChromeWebStore,
        id: "ddkjiahejlhfcafbddmgiahcphecmpfh",
    },
    Recommended {
        name: "Bitwarden",
        blurb: "Open-source password manager that syncs across your devices.",
        store: CrxStore::ChromeWebStore,
        id: "nngceckbapebfimnlniiiahkandclblb",
    },
    Recommended {
        name: "Proton Pass",
        blurb: "End-to-end encrypted password manager with hide-my-email aliases.",
        store: CrxStore::ChromeWebStore,
        id: "ghmbeldphafepmbegfdlkpapadhbakde",
    },
];
