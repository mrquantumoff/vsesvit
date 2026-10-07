//! The profile list: every profile of an install, with its name, avatar colour and picture, the
//! one used last, and whether the profile picker shows at startup.
//!
//! A profile is a directory directly inside one [`ProfilesDir`] (`Default`, `Profile 1`, ...),
//! and one process runs each profile (`Profile::open` locks it). The list is the small file
//! `profiles.json` next to those directories, which every profile's process reads and
//! writes, like Chrome's Local State. A writer holds the OS lock on `profiles.lock` from
//! reading the file to replacing it by rename, so two processes never lose each other's
//! change and a reader, which takes no lock, always sees a whole file.
//!
//! The list never caches anything a shell must keep in step: [`ProfilesDir::load`] reads it,
//! and every change re-reads it under the lock and returns the new [`Registry`].
//!
//! A profile signed in to sync takes its name and picture from the account (`account`), until
//! the user names or colours it by hand.

mod account;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ProfilePaths;

pub use account::{AccountDetails, AccountPicture};

const FILE: &str = "profiles.json";
const LOCK: &str = "profiles.lock";
/// The longest profile name, in characters.
pub const NAME_MAX: usize = 64;

/// The directory holding the profiles and their list. The shells choose it once:
/// [`ProfilesDir::standard`] for browsing, the output directory for the scripted runs, so
/// those never touch the user's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilesDir {
    path: PathBuf,
}

/// A listed profile's place: the directory of its list and its id there. A profile opened
/// with a directory outside any list has none, and no profile menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub dir: ProfilesDir,
    pub id: ProfileId,
}

impl Home {
    pub fn root(&self) -> PathBuf {
        self.dir.root(&self.id)
    }
}

/// A profile's directory name inside its [`ProfilesDir`]: one path component that is not
/// empty, not `.` or `..`, and does not start with a dot.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileId(String);

impl ProfileId {
    pub const DEFAULT: &str = "Default";

    pub fn default_profile() -> ProfileId {
        ProfileId(Self::DEFAULT.to_owned())
    }

    pub fn parse(name: &str) -> Option<ProfileId> {
        let valid = !name.is_empty()
            && !name.starts_with('.')
            && !name.contains(['/', '\\', ':'])
            && name.trim() == name;
        valid.then(|| ProfileId(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProfileId {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        ProfileId::parse(&name).ok_or_else(|| format!("not a profile directory name: {name:?}"))
    }
}

impl From<ProfileId> for String {
    fn from(id: ProfileId) -> String {
        id.0
    }
}

/// An avatar colour: libadwaita's accent colours, so both shells paint the same circles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileColor {
    Blue,
    Teal,
    Green,
    Yellow,
    Orange,
    Red,
    Pink,
    Purple,
    Slate,
}

impl ProfileColor {
    pub const ALL: [ProfileColor; 9] = [
        ProfileColor::Blue,
        ProfileColor::Teal,
        ProfileColor::Green,
        ProfileColor::Yellow,
        ProfileColor::Orange,
        ProfileColor::Red,
        ProfileColor::Pink,
        ProfileColor::Purple,
        ProfileColor::Slate,
    ];

    /// White text reads on every one of them.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            ProfileColor::Blue => [0x35, 0x84, 0xe4],
            ProfileColor::Teal => [0x21, 0x90, 0xa4],
            ProfileColor::Green => [0x3a, 0x94, 0x4a],
            ProfileColor::Yellow => [0xc8, 0x88, 0x00],
            ProfileColor::Orange => [0xed, 0x5b, 0x00],
            ProfileColor::Red => [0xe6, 0x2d, 0x42],
            ProfileColor::Pink => [0xd5, 0x61, 0x99],
            ProfileColor::Purple => [0x91, 0x41, 0xac],
            ProfileColor::Slate => [0x6f, 0x83, 0x96],
        }
    }

    pub fn css(self) -> String {
        let [r, g, b] = self.rgb();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    pub fn label(self) -> &'static str {
        match self {
            ProfileColor::Blue => "Blue",
            ProfileColor::Teal => "Teal",
            ProfileColor::Green => "Green",
            ProfileColor::Yellow => "Yellow",
            ProfileColor::Orange => "Orange",
            ProfileColor::Red => "Red",
            ProfileColor::Pink => "Pink",
            ProfileColor::Purple => "Purple",
            ProfileColor::Slate => "Slate",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: ProfileId,
    pub name: String,
    pub color: ProfileColor,
    /// The sync account's picture, shown in place of the coloured initial: a PNG in the
    /// profile's directory, by its file name ([`ProfilesDir::picture`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    picture: Option<String>,
    /// The user named or coloured the profile: its sync account no longer changes it. `None` in
    /// lists from before profiles took details from sync, see [`ProfileEntry::set_by_hand`].
    #[serde(default)]
    set_by_hand: Option<bool>,
}

impl ProfileEntry {
    fn new(id: ProfileId, name: String, color: ProfileColor, set_by_hand: bool) -> ProfileEntry {
        ProfileEntry { id, name, color, picture: None, set_by_hand: Some(set_by_hand) }
    }

    /// For a profile listed before profiles took details from sync: when its name is not one
    /// Chrome gives unnamed profiles, the user gave it.
    pub fn set_by_hand(&self) -> bool {
        self.set_by_hand.unwrap_or_else(|| !is_unnamed(&self.name))
    }

    /// The letter on the avatar: the name's first letter or digit, upper-cased.
    pub fn initial(&self) -> String {
        avatar_initial(&self.name)
    }
}

/// "Person N", as [`Registry::next_name`] names profiles.
fn is_unnamed(name: &str) -> bool {
    name.strip_prefix("Person ").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn avatar_initial(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".to_owned())
}

/// One read of the list. Plain data; the shells keep the last one they read and compare.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    /// In the order they were added, which is the menu's order. No two share an id.
    profiles: Vec<ProfileEntry>,
    #[serde(default)]
    last_used: Option<ProfileId>,
    /// Install-wide, like Chrome's "Show on startup". On by default, as in Chrome.
    #[serde(default = "yes")]
    show_picker: bool,
    /// Removed profiles whose directories are not deleted yet, because their process was
    /// still running or the deletion failed. [`ProfilesDir::sweep`] finishes the job.
    #[serde(default)]
    removed: Vec<ProfileId>,
}

fn yes() -> bool {
    true
}

impl Default for Registry {
    fn default() -> Self {
        Registry { profiles: Vec::new(), last_used: None, show_picker: true, removed: Vec::new() }
    }
}

/// What a launch that names no profile does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Startup {
    Open(ProfileId),
    /// Show the profile picker and open nothing yet.
    Picker,
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("a profile needs a name")]
    EmptyName,
    #[error("a profile name has at most {NAME_MAX} characters")]
    LongName,
    #[error("the last profile cannot be removed")]
    LastProfile,
    #[error("that profile is no longer in the list")]
    NotListed,
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl Registry {
    pub fn profiles(&self) -> &[ProfileEntry] {
        &self.profiles
    }

    pub fn get(&self, id: &ProfileId) -> Option<&ProfileEntry> {
        self.profiles.iter().find(|p| &p.id == id)
    }

    pub fn show_picker(&self) -> bool {
        self.show_picker
    }

    pub fn last_used(&self) -> Option<&ProfileId> {
        self.last_used.as_ref()
    }

    /// Removed while its process ran: that process closes, and its data goes once it has.
    pub fn is_removed(&self, id: &ProfileId) -> bool {
        self.removed.contains(id)
    }

    /// The name window titles carry: only when there is more than one profile to tell apart.
    pub fn title_name(&self, id: &ProfileId) -> Option<&str> {
        if self.profiles.len() < 2 {
            return None;
        }
        self.get(id).map(|p| p.name.as_str())
    }

    /// Chrome keeps one profile at least.
    pub fn can_remove(&self, id: &ProfileId) -> bool {
        self.profiles.len() > 1 && self.get(id).is_some()
    }

    /// The first colour no profile has, else the least used one, for a new profile.
    pub fn next_color(&self) -> ProfileColor {
        let uses = |color: &ProfileColor| self.profiles.iter().filter(|p| p.color == *color).count();
        ProfileColor::ALL.into_iter().min_by_key(uses).expect("ALL is not empty")
    }

    /// Chrome's name for a profile nobody named: "Person N", with the first N no profile uses.
    pub fn next_name(&self) -> String {
        (1..)
            .map(|n| format!("Person {n}"))
            .find(|name| !self.profiles.iter().any(|p| &p.name == name))
            .expect("some number is free")
    }

    /// What a launch that names no profile opens, given whether any listed profile is open
    /// in a process. With several profiles and the switch on, the picker shows at the start of
    /// the browser, as in Chrome: not when a profile is already running (a second launch opens
    /// a window of the last used one) and not for addresses another app hands over.
    pub fn startup(&self, has_urls: bool, any_running: bool) -> Startup {
        if self.profiles.len() > 1 && self.show_picker && !has_urls && !any_running {
            return Startup::Picker;
        }
        let listed = |id: &ProfileId| self.get(id).is_some();
        let default = ProfileId::default_profile();
        let id = self
            .last_used
            .clone()
            .filter(listed)
            .or_else(|| listed(&default).then_some(default))
            .or_else(|| self.profiles.first().map(|p| p.id.clone()))
            .unwrap_or_else(ProfileId::default_profile);
        Startup::Open(id)
    }
}

fn valid_name(name: &str) -> Result<String, EditError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(EditError::EmptyName);
    }
    if name.chars().count() > NAME_MAX {
        return Err(EditError::LongName);
    }
    Ok(name.to_owned())
}

impl ProfilesDir {
    /// `<data_local_dir>/Vsesvit/profiles`, the parent of [`crate::Profile::default_root`].
    pub fn standard() -> ProfilesDir {
        ProfilesDir::at(crate::default_profiles_dir())
    }

    pub fn at(path: PathBuf) -> ProfilesDir {
        ProfilesDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root(&self, id: &ProfileId) -> PathBuf {
        self.path.join(id.as_str())
    }

    /// The profile `root` is, when it is a directory directly inside this one, whether or not
    /// it exists yet. Compares canonical paths where they exist, so two spellings of one
    /// directory agree.
    pub fn locate(&self, root: &Path) -> Option<ProfileId> {
        let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
        let root = std::path::absolute(root).ok()?;
        let root = match fs::canonicalize(&root) {
            Ok(root) => root,
            Err(_) => canonical(root.parent()?).join(root.file_name()?),
        };
        if root.parent()? != canonical(&self.path) {
            return None;
        }
        ProfileId::parse(root.file_name()?.to_str()?)
    }

    /// Never fails. A missing list is an empty one (nothing has opened a profile since it
    /// came). An unreadable one is logged and read as empty; the next change rewrites it.
    /// Profiles whose directory is gone are left out.
    pub fn load(&self) -> Registry {
        let mut registry = match fs::read(self.path.join(FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("{}: {e}; reading it as empty", self.path.join(FILE).display());
                Registry::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Registry::default(),
            Err(e) => {
                log::warn!("{}: {e}", self.path.join(FILE).display());
                Registry::default()
            }
        };
        let mut seen = std::collections::HashSet::new();
        registry.profiles.retain(|p| seen.insert(p.id.clone()) && self.root(&p.id).is_dir());
        registry
    }

    /// Lists `id` if it is not listed (named like Chrome's unnamed profiles) and marks it
    /// last used, which a launch that names no profile opens. The process that has a profile
    /// open calls this when it starts and whenever one of its windows comes forward, so every
    /// profile that runs is listed, an old install's `Default` included. A removed profile
    /// stays removed.
    pub fn opened(&self, id: &ProfileId) -> io::Result<Registry> {
        self.edit(|registry| {
            if registry.is_removed(id) || registry.last_used.as_ref() == Some(id) && registry.get(id).is_some() {
                return Ok(false);
            }
            if registry.get(id).is_none() {
                let entry = ProfileEntry::new(id.clone(), registry.next_name(), registry.next_color(), false);
                registry.profiles.push(entry);
            }
            registry.last_used = Some(id.clone());
            Ok(true)
        })
        .map_err(into_io)
    }

    /// Creates the directory `Profile N` (the first N that is free, as Chrome names them) and
    /// lists it. The directory exists from now on, so another process's `add` picks the next N;
    /// the profile's process fills it when it starts. A name or colour other than the ones
    /// suggested ([`Registry::next_name`], [`Registry::next_color`]) counts as set by hand.
    pub fn add(&self, name: &str, color: ProfileColor) -> Result<(ProfileId, Registry), EditError> {
        let name = valid_name(name)?;
        let mut added = None;
        let registry = self.edit(|registry| {
            let by_hand = name != registry.next_name() || color != registry.next_color();
            let id = self.claim_dir(registry)?;
            registry.profiles.push(ProfileEntry::new(id.clone(), name, color, by_hand));
            added = Some(id);
            Ok(true)
        })?;
        Ok((added.expect("edit ran the closure"), registry))
    }

    fn claim_dir(&self, registry: &Registry) -> io::Result<ProfileId> {
        fs::create_dir_all(&self.path)?;
        for n in 1.. {
            let id = ProfileId(format!("Profile {n}"));
            if registry.get(&id).is_some() || registry.removed.contains(&id) {
                continue;
            }
            match fs::create_dir(self.root(&id)) {
                Ok(()) => return Ok(id),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        unreachable!("some number is free")
    }

    /// Renames and recolours `id`, by hand: its sync account no longer changes it. A new colour
    /// shows in place of the account's picture, which is dropped.
    pub fn edit_profile(&self, id: &ProfileId, name: &str, color: ProfileColor) -> Result<Registry, EditError> {
        let name = valid_name(name)?;
        self.edit(|registry| {
            let entry = registry.profiles.iter_mut().find(|p| &p.id == id).ok_or(EditError::NotListed)?;
            if entry.name == name && entry.color == color {
                return Ok(false);
            }
            if entry.color != color
                && let Some(old) = entry.picture.take()
            {
                self.delete_picture(id, &old);
            }
            entry.name = name;
            entry.color = color;
            entry.set_by_hand = Some(true);
            Ok(true)
        })
    }

    /// Takes the name and picture of the sync account `id` is signed in to, unless the user set
    /// its details by hand. Either may be `None`, which keeps the profile's own.
    pub fn take_account_details(&self, id: &ProfileId, name: Option<&str>, picture: Option<&AccountPicture>) -> Result<Registry, EditError> {
        let name = name.and_then(|name| valid_name(name).ok());
        self.edit(|registry| {
            let entry = registry.profiles.iter_mut().find(|p| &p.id == id).ok_or(EditError::NotListed)?;
            if entry.set_by_hand() {
                return Ok(false);
            }
            let mut changed = false;
            if let Some(name) = name.filter(|name| *name != entry.name) {
                entry.name = name;
                changed = true;
            }
            if let Some(picture) = picture.filter(|p| entry.picture.as_deref() != Some(p.file_name().as_str())) {
                let file = picture.file_name();
                write_atomically(&self.root(id).join(&file), picture.png())?;
                if let Some(old) = entry.picture.replace(file) {
                    self.delete_picture(id, &old);
                }
                changed = true;
            }
            Ok(changed)
        })
    }

    /// The file of `entry`'s account picture, when it has one.
    pub fn picture(&self, entry: &ProfileEntry) -> Option<PathBuf> {
        let file = entry.picture.as_deref().filter(|f| !f.starts_with('.') && !f.contains(['/', '\\', ':']) && f.ends_with(".png"))?;
        Some(self.root(&entry.id).join(file))
    }

    fn delete_picture(&self, id: &ProfileId, file: &str) {
        let path = self.root(id).join(file);
        if let Err(e) = fs::remove_file(&path)
            && e.kind() != io::ErrorKind::NotFound
        {
            log::warn!("deleting {}: {e}", path.display());
        }
    }

    /// Takes `id` off the list at once and deletes its data, now if no process has it open,
    /// else at a later [`ProfilesDir::sweep`] once its process has quit.
    pub fn remove(&self, id: &ProfileId) -> Result<Registry, EditError> {
        self.edit(|registry| {
            if registry.get(id).is_none() {
                return Err(EditError::NotListed);
            }
            if !registry.can_remove(id) {
                return Err(EditError::LastProfile);
            }
            registry.profiles.retain(|p| &p.id != id);
            if registry.last_used.as_ref() == Some(id) {
                registry.last_used = None;
            }
            registry.removed.push(id.clone());
            Ok(true)
        })?;
        Ok(self.sweep())
    }

    /// Deletes the data of removed profiles no process has open, and forgets those it
    /// deleted. Idempotent; runs at every start and after every removal, so a profile removed
    /// while it ran goes once its process has quit.
    pub fn sweep(&self) -> Registry {
        let registry = self.load();
        if registry.removed.is_empty() {
            return registry;
        }
        let deleted: Vec<ProfileId> = registry.removed.into_iter().filter(|id| self.delete(id)).collect();
        let result = self.edit(|registry| {
            let before = registry.removed.len();
            registry.removed.retain(|id| !deleted.contains(id));
            Ok(registry.removed.len() != before)
        });
        result.unwrap_or_else(|e| {
            log::warn!("profiles: {e}");
            self.load()
        })
    }

    /// Deletes a removed profile's directory and cache unless its process runs. True once
    /// nothing is left.
    fn delete(&self, id: &ProfileId) -> bool {
        let root = self.root(id);
        if !root.exists() {
            return true;
        }
        let Some(lock) = try_lock(&root) else { return false };
        let cache = ProfilePaths::new(&root).engine_cache;
        // Everything but the lock first, so no process can open the profile midway.
        let result = fs::read_dir(&root).and_then(|entries| {
            for entry in entries {
                let path = entry?.path();
                if path.file_name() == Some("LOCK".as_ref()) {
                    continue;
                }
                if path.is_dir() { fs::remove_dir_all(&path)? } else { fs::remove_file(&path)? }
            }
            Ok(())
        });
        drop(lock);
        let result = result.and_then(|()| fs::remove_dir_all(&root));
        if !cache.starts_with(&root)
            && cache.exists()
            && let Err(e) = fs::remove_dir_all(&cache)
        {
            log::warn!("deleting {}: {e}", cache.display());
        }
        match result {
            Ok(()) => true,
            Err(e) => {
                log::warn!("deleting the removed profile {}: {e}; trying again later", root.display());
                false
            }
        }
    }

    pub fn set_show_picker(&self, on: bool) -> io::Result<Registry> {
        self.edit(|registry| Ok(std::mem::replace(&mut registry.show_picker, on) != on)).map_err(into_io)
    }

    /// What a launch that names no profile does ([`Registry::startup`]), checking which
    /// listed profiles a process has open.
    pub fn startup(&self, has_urls: bool) -> Startup {
        let registry = self.load();
        let any_running = || registry.profiles.iter().any(|p| self.is_running(&p.id));
        let picker_possible = registry.startup(has_urls, false) == Startup::Picker;
        registry.startup(has_urls, picker_possible && any_running())
    }

    /// Whether a process has `id` open: its lock is held. Taking it for a moment cannot make
    /// a starting process fail, because `Profile::open` waits a little for the lock.
    pub fn is_running(&self, id: &ProfileId) -> bool {
        let root = self.root(id);
        root.join("LOCK").exists() && try_lock(&root).is_none()
    }

    /// Reads the list, applies `change` and writes the result, all under the list's lock.
    /// `Ok(false)` from `change` writes nothing.
    fn edit(&self, change: impl FnOnce(&mut Registry) -> Result<bool, EditError>) -> Result<Registry, EditError> {
        fs::create_dir_all(&self.path)?;
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.path.join(LOCK))?;
        lock.lock()?;
        let mut registry = self.load();
        if change(&mut registry)? {
            let json = serde_json::to_vec_pretty(&registry).map_err(io::Error::other)?;
            write_atomically(&self.path.join(FILE), &json)?;
        }
        Ok(registry)
    }
}

/// Writes a temporary file beside `path` and renames it over `path`, so a reader sees the old
/// file or the new one, never a part.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.{}.tmp", uuid::Uuid::new_v4()));
    let written = fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    written.and_then(|()| fs::rename(&tmp, path)).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn into_io(e: EditError) -> io::Error {
    match e {
        EditError::Io(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

/// The profile's lock, if no process holds it.
fn try_lock(root: &Path) -> Option<fs::File> {
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join("LOCK")).ok()?;
    lock.try_lock().ok().map(|()| lock)
}

/// How long `Profile::open` keeps trying a held lock before it reports the profile as
/// locked, which covers another process checking it for a moment ([`ProfilesDir::is_running`]).
pub(crate) const LOCK_PATIENCE: Duration = Duration::from_millis(100);

/// `"{page} - {profile}"` when the title carries a profile name, else the page title.
pub fn with_profile(page: &str, profile: Option<&str>) -> String {
    match profile {
        Some(name) => format!("{page} - {name}"),
        None => page.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OpenError, OpenOptions, Profile};

    struct Dir(ProfilesDir);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.path());
        }
    }
    impl std::ops::Deref for Dir {
        type Target = ProfilesDir;
        fn deref(&self) -> &ProfilesDir {
            &self.0
        }
    }
    fn dir() -> Dir {
        let path = std::env::temp_dir().join(format!("vsesvit-profiles-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Dir(ProfilesDir::at(path))
    }
    fn id(name: &str) -> ProfileId {
        ProfileId::parse(name).unwrap()
    }
    fn names(registry: &Registry) -> Vec<&str> {
        registry.profiles().iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn ids_are_single_plain_path_components() {
        for good in ["Default", "Profile 1", "work"] {
            assert!(ProfileId::parse(good).is_some(), "{good}");
        }
        for bad in ["", ".", "..", ".trash", "a/b", "a\\b", "C:", " x", "x "] {
            assert!(ProfileId::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_list_is_empty_with_the_picker_on_and_opens_default() {
        let dir = dir();
        let registry = dir.load();
        assert!(registry.profiles().is_empty());
        assert!(registry.show_picker());
        assert_eq!(dir.startup(false), Startup::Open(ProfileId::default_profile()));
        assert!(!dir.path().join(FILE).exists(), "reading writes nothing");
    }

    #[test]
    fn opening_lists_an_old_install_default_once() {
        let dir = dir();
        fs::create_dir_all(dir.root(&ProfileId::default_profile())).unwrap();
        let first = dir.opened(&ProfileId::default_profile()).unwrap();
        assert_eq!(names(&first), ["Person 1"]);
        assert_eq!(first.last_used(), Some(&ProfileId::default_profile()));
        let modified = fs::metadata(dir.path().join(FILE)).unwrap().modified().unwrap();
        assert_eq!(dir.opened(&ProfileId::default_profile()).unwrap(), first);
        assert_eq!(fs::metadata(dir.path().join(FILE)).unwrap().modified().unwrap(), modified, "nothing changed, nothing written");
        assert_eq!(dir.load(), first);
    }

    #[test]
    fn add_claims_the_first_free_profile_n_and_an_unused_colour() {
        let dir = dir();
        fs::create_dir_all(dir.root(&ProfileId::default_profile())).unwrap();
        dir.opened(&ProfileId::default_profile()).unwrap();
        fs::create_dir_all(dir.path().join("Profile 1")).unwrap();
        let registry = dir.load();
        assert_eq!(registry.next_color(), ProfileColor::Teal);
        let (work, registry) = dir.add("  Work ", registry.next_color()).unwrap();
        assert_eq!(work, id("Profile 2"), "an existing directory is skipped");
        assert!(dir.root(&work).is_dir());
        assert_eq!(names(&registry), ["Person 1", "Work"]);
        assert_eq!(registry.get(&work).unwrap().color, ProfileColor::Teal);
        assert_eq!(registry.next_name(), "Person 2");
        let (home, _) = dir.add("Home", ProfileColor::Blue).unwrap();
        assert_eq!(home, id("Profile 3"));
        assert!(matches!(dir.add(" ", ProfileColor::Blue), Err(EditError::EmptyName)));
        assert!(matches!(dir.add(&"x".repeat(NAME_MAX + 1), ProfileColor::Blue), Err(EditError::LongName)));
    }

    #[test]
    fn edits_and_titles() {
        let dir = dir();
        fs::create_dir_all(dir.root(&ProfileId::default_profile())).unwrap();
        let registry = dir.opened(&ProfileId::default_profile()).unwrap();
        assert_eq!(registry.title_name(&ProfileId::default_profile()), None, "one profile needs no name");
        let (work, registry) = dir.add("Work", ProfileColor::Green).unwrap();
        assert_eq!(registry.title_name(&ProfileId::default_profile()), Some("Person 1"));
        let registry = dir.edit_profile(&work, "Office", ProfileColor::Red).unwrap();
        assert_eq!(registry.title_name(&work), Some("Office"));
        assert_eq!(registry.get(&work).unwrap().color, ProfileColor::Red);
        assert_eq!(registry.title_name(&id("Elsewhere")), None);
        assert!(matches!(dir.edit_profile(&id("Gone"), "x", ProfileColor::Red), Err(EditError::NotListed)));
        assert_eq!(with_profile("Inbox", Some("Office")), "Inbox - Office");
        assert_eq!(with_profile("Inbox", None), "Inbox");
        assert_eq!(registry.get(&work).unwrap().initial(), "O");
        assert_eq!(avatar_initial("  élan"), "É");
        assert_eq!(avatar_initial("--"), "?");
    }

    #[test]
    fn startup_follows_chrome() {
        let mut registry = Registry::default();
        assert_eq!(registry.startup(false, false), Startup::Open(ProfileId::default_profile()));
        registry.profiles.push(ProfileEntry::new(id("Profile 1"), "Work".into(), ProfileColor::Blue, true));
        assert_eq!(registry.startup(false, false), Startup::Open(id("Profile 1")), "the only profile, even if not Default");
        registry.profiles.push(ProfileEntry::new(ProfileId::default_profile(), "Me".into(), ProfileColor::Teal, true));
        assert_eq!(registry.startup(false, false), Startup::Picker);
        assert_eq!(registry.startup(true, false), Startup::Open(ProfileId::default_profile()), "addresses skip the picker");
        assert_eq!(registry.startup(false, true), Startup::Open(ProfileId::default_profile()), "a running browser opens a window");
        registry.last_used = Some(id("Profile 1"));
        assert_eq!(registry.startup(true, false), Startup::Open(id("Profile 1")));
        registry.show_picker = false;
        assert_eq!(registry.startup(false, false), Startup::Open(id("Profile 1")));
        registry.last_used = Some(id("Removed"));
        assert_eq!(registry.startup(false, false), Startup::Open(ProfileId::default_profile()));
    }

    #[test]
    fn a_running_profile_keeps_the_picker_away_and_is_deleted_only_after_it_quits() {
        let dir = dir();
        let default = ProfileId::default_profile();
        let profile = Profile::open(&dir.root(&default), OpenOptions::default()).unwrap();
        dir.opened(&default).unwrap();
        let (work, _) = dir.add("Work", ProfileColor::Green).unwrap();
        assert!(dir.is_running(&default));
        assert!(!dir.is_running(&work));
        assert_eq!(dir.startup(false), Startup::Open(default.clone()));
        assert!(matches!(
            Profile::open(&dir.root(&default), OpenOptions::default()),
            Err(OpenError::Locked)
        ));

        let registry = dir.remove(&default).unwrap();
        assert_eq!(names(&registry), ["Work"]);
        assert_eq!(registry.removed, std::slice::from_ref(&default), "still open, so kept for later");
        assert!(dir.root(&default).join("vsesvit.db").exists());
        assert!(matches!(dir.remove(&work), Err(EditError::LastProfile)));
        assert!(matches!(dir.remove(&default), Err(EditError::NotListed)));

        drop(profile);
        let registry = dir.sweep();
        assert!(registry.removed.is_empty());
        assert!(!dir.root(&default).exists());
        assert_eq!(dir.sweep(), registry, "sweeping again changes nothing");
        assert_eq!(dir.startup(false), Startup::Open(work));
    }

    #[test]
    fn a_removed_profile_stays_removed_when_it_opens() {
        let dir = dir();
        let default = ProfileId::default_profile();
        let profile = Profile::open(&dir.root(&default), OpenOptions::default()).unwrap();
        dir.opened(&default).unwrap();
        dir.add("Work", ProfileColor::Green).unwrap();
        dir.remove(&default).unwrap();
        let registry = dir.opened(&default).unwrap();
        assert!(registry.is_removed(&default));
        assert_eq!(names(&registry), ["Work"]);
        drop(profile);
        dir.sweep();
        assert!(!dir.root(&default).exists());
    }

    #[test]
    fn concurrent_edits_lose_nothing() {
        let dir = dir();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let dir = dir.0.clone();
                std::thread::spawn(move || {
                    for _ in 0..5 {
                        dir.add("P", ProfileColor::Blue).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(dir.load().profiles().len(), 20);
    }

    #[test]
    fn a_corrupt_list_reads_as_empty_and_the_next_change_rewrites_it() {
        let dir = dir();
        fs::write(dir.path().join(FILE), b"{not json").unwrap();
        assert_eq!(dir.load(), Registry::default());
        fs::create_dir_all(dir.root(&ProfileId::default_profile())).unwrap();
        dir.opened(&ProfileId::default_profile()).unwrap();
        assert_eq!(names(&dir.load()), ["Person 1"]);
    }

    #[test]
    fn profiles_whose_directory_is_gone_are_left_out() {
        let dir = dir();
        let (work, _) = dir.add("Work", ProfileColor::Blue).unwrap();
        fs::remove_dir(dir.root(&work)).unwrap();
        assert!(dir.load().profiles().is_empty());
    }

    #[test]
    fn locate_accepts_direct_children_only() {
        let dir = dir();
        let root = dir.path().join("Profile 1");
        fs::create_dir_all(root.join("nested")).unwrap();
        assert_eq!(dir.locate(&root), Some(id("Profile 1")));
        assert_eq!(dir.locate(&dir.path().join("./Profile 1")), Some(id("Profile 1")));
        assert_eq!(dir.locate(&root.join("nested")), None);
        assert_eq!(dir.locate(dir.path()), None);
        assert_eq!(dir.locate(&dir.path().join("Profile 2")), Some(id("Profile 2")), "not made yet");
    }

    fn picture(rgba: [u8; 4]) -> AccountPicture {
        let mut png = Vec::new();
        image::DynamicImage::from(image::RgbaImage::from_pixel(4, 4, image::Rgba(rgba)))
            .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        AccountPicture::decode(&png).unwrap()
    }

    #[test]
    fn a_profile_takes_its_sync_accounts_details_until_set_by_hand() {
        let dir = dir();
        let default = ProfileId::default_profile();
        fs::create_dir_all(dir.root(&default)).unwrap();
        dir.opened(&default).unwrap();
        let modified = || fs::metadata(dir.path().join(FILE)).unwrap().modified().unwrap();
        let before = modified();
        assert_eq!(dir.take_account_details(&default, None, None).unwrap(), dir.load());
        assert_eq!(modified(), before, "an account with no details changes nothing");

        let first = picture([1, 2, 3, 255]);
        let registry = dir.take_account_details(&default, Some("Demir"), Some(&first)).unwrap();
        let entry = registry.get(&default).unwrap();
        assert_eq!(entry.name, "Demir");
        assert!(!entry.set_by_hand());
        let first_file = dir.picture(entry).unwrap();
        assert_eq!(fs::read(&first_file).unwrap(), first.png());
        assert_eq!(dir.load(), registry);

        let registry = dir.take_account_details(&default, None, Some(&picture([3, 2, 1, 255]))).unwrap();
        let second_file = dir.picture(registry.get(&default).unwrap()).unwrap();
        assert_ne!(second_file, first_file);
        assert!(second_file.exists() && !first_file.exists(), "the old picture is deleted");
        assert_eq!(registry.get(&default).unwrap().name, "Demir", "no name keeps the name");

        let registry = dir.edit_profile(&default, "Me", ProfileColor::Blue).unwrap();
        let entry = registry.get(&default).unwrap();
        assert!(entry.set_by_hand());
        assert_eq!(dir.picture(entry), Some(second_file.clone()), "renamed only, so the picture stays");
        let registry = dir.take_account_details(&default, Some("Someone"), Some(&first)).unwrap();
        assert_eq!(registry.get(&default).unwrap().name, "Me");
        assert_eq!(dir.picture(registry.get(&default).unwrap()), Some(second_file.clone()));

        let registry = dir.edit_profile(&default, "Me", ProfileColor::Red).unwrap();
        assert_eq!(dir.picture(registry.get(&default).unwrap()), None, "a colour chosen by hand replaces the picture");
        assert!(!second_file.exists());
    }

    #[test]
    fn a_profile_added_with_the_suggested_name_and_colour_is_not_set_by_hand() {
        let dir = dir();
        let registry = dir.load();
        let (suggested, registry) = dir.add(&registry.next_name(), registry.next_color()).unwrap();
        assert!(!registry.get(&suggested).unwrap().set_by_hand());
        let (named, registry) = dir.add("Work", registry.next_color()).unwrap();
        assert!(registry.get(&named).unwrap().set_by_hand());
        let (coloured, registry) = dir.add(&registry.next_name(), ProfileColor::Blue).unwrap();
        assert!(registry.get(&coloured).unwrap().set_by_hand());
        assert!(matches!(dir.take_account_details(&id("Gone"), Some("x"), None), Err(EditError::NotListed)));
    }

    #[test]
    fn a_list_from_before_counts_every_name_but_person_n_as_set_by_hand() {
        let dir = dir();
        for name in ["Default", "Profile 1", "Profile 2"] {
            fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        let old = r#"{"profiles":[{"id":"Default","name":"Person 1","color":"blue"},{"id":"Profile 1","name":"Work","color":"teal"},
            {"id":"Profile 2","name":"Person 12","color":"green","picture":"../../elsewhere.png"}]}"#;
        fs::write(dir.path().join(FILE), old).unwrap();
        let registry = dir.load();
        let by_hand: Vec<bool> = registry.profiles().iter().map(ProfileEntry::set_by_hand).collect();
        assert_eq!(by_hand, [false, true, false]);
        assert_eq!(dir.picture(&registry.profiles()[2]), None, "a picture outside the profile's directory is never read");
    }

    #[test]
    fn the_picker_switch_is_stored() {
        let dir = dir();
        assert!(!dir.set_show_picker(false).unwrap().show_picker());
        assert!(!dir.load().show_picker());
        assert!(dir.set_show_picker(true).unwrap().show_picker());
    }
}
