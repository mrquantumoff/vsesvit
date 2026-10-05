//! Extensions' keyboard shortcuts (the manifest's `commands`), resolved against the browser's
//! keymap.
//!
//! The user's changes are stored in [`keys::SHORTCUTS`] next to the browser's, under
//! `extension:<extension id>:<command name>` (extension ids never contain `:`). The keymap keeps
//! ids it has no [`crate::shortcuts::Command`] for and writes them back, so the whole set syncs
//! as one register, older builds keep these entries, and [`Keymap::reset_all`] resets them too.
//! As in Chrome, a command has at most one chord.
//!
//! [`keys::SHORTCUTS`]: crate::prefs::keys::SHORTCUTS

use std::collections::BTreeSet;

use super::manifest::ManifestCommand;
use super::{ExtensionId, InstalledExtension};
use crate::shortcuts::{Chord, Keymap};
use crate::{Error, Profile};

/// An enabled extension's command, as the keymap resolves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionCommand {
    pub extension: ExtensionId,
    pub extension_name: String,
    pub command: ManifestCommand,
}

impl ExtensionCommand {
    /// What the shortcuts list calls it: the description, else Chrome's words for an action
    /// command, else the name.
    pub fn title(&self) -> &str {
        match self.command.description.as_str() {
            "" if self.command.activates_action() => "Activate the extension",
            "" => &self.command.name,
            description => description,
        }
    }

    fn is(&self, extension: &ExtensionId, name: &str) -> bool {
        self.extension == *extension && self.command.name == name
    }

    /// The key its override is stored under.
    fn id(&self) -> String {
        format!("extension:{}:{}", self.extension.as_str(), self.command.name)
    }

    fn also_used_note(&self) -> String {
        format!("Also used by {}: {}. Saving moves it here.", self.extension_name, self.title())
    }
}

/// Every enabled extension's commands, in the order given ([`super::Extensions::list`] order,
/// which is install order), each extension's in name order.
pub fn extension_commands(installed: &[InstalledExtension]) -> Vec<ExtensionCommand> {
    installed
        .iter()
        .filter(|ext| ext.enabled)
        .flat_map(|ext| {
            ext.manifest.commands.iter().map(|command| ExtensionCommand {
                extension: ext.id.clone(),
                extension_name: ext.manifest.name.clone(),
                command: command.clone(),
            })
        })
        .collect()
}

/// The effective shortcut of each extension command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionShortcuts {
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    command: ExtensionCommand,
    chord: Option<Chord>,
    overridden: bool,
}

impl ExtensionShortcuts {
    pub fn iter(&self) -> impl Iterator<Item = (&ExtensionCommand, Option<Chord>)> {
        self.entries.iter().map(|e| (&e.command, e.chord))
    }

    pub fn chord(&self, extension: &ExtensionId, name: &str) -> Option<Chord> {
        self.entry(extension, name)?.chord
    }

    pub fn command_for(&self, chord: Chord) -> Option<&ExtensionCommand> {
        self.entries.iter().find(|e| e.chord == Some(chord)).map(|e| &e.command)
    }

    /// No stored override: the command has its suggested key, unless something else holds it.
    pub fn is_default(&self, extension: &ExtensionId, name: &str) -> bool {
        self.entry(extension, name).is_none_or(|e| !e.overridden)
    }

    /// The browser's shortcut-capture dialog's note for `chord` held by an extension command.
    pub fn note_for_browser(&self, chord: Chord) -> Option<String> {
        self.command_for(chord).map(ExtensionCommand::also_used_note)
    }

    fn entry(&self, extension: &ExtensionId, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.command.is(extension, name))
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        self.entries.iter().map(|e| e.command.clone()).collect()
    }

    fn chords(&self) -> Vec<Option<Chord>> {
        self.entries.iter().map(|e| e.chord).collect()
    }
}

impl Keymap {
    /// No chord goes to two commands. The browser's chords are never an extension's. Then
    /// stored overrides claim theirs in `commands` order, and the commands without one get
    /// their suggested keys in that order, so the first installed wins, as in Chrome.
    pub fn extension_shortcuts(&self, commands: Vec<ExtensionCommand>) -> ExtensionShortcuts {
        // `Some(None)`: the user removed the shortcut, or this build cannot read the stored one.
        let overrides: Vec<Option<Option<Chord>>> = commands
            .iter()
            .map(|c| self.unknown.get(&c.id()).map(|strings| strings.iter().find_map(|s| s.parse().ok())))
            .collect();
        let mut chords = vec![None; commands.len()];
        let mut claimed = BTreeSet::new();
        let overridden = overrides.iter().enumerate().filter_map(|(i, o)| Some((i, (*o)?)));
        let suggested = commands.iter().enumerate().filter(|(i, _)| overrides[*i].is_none()).map(|(i, c)| (i, c.command.suggested_key));
        for (i, wanted) in overridden.chain(suggested) {
            if let Some(chord) = wanted
                && self.command_for(chord).is_none()
                && claimed.insert(chord)
            {
                chords[i] = Some(chord);
            }
        }
        let entries = commands
            .into_iter()
            .zip(chords)
            .zip(overrides)
            .map(|((command, chord), stored)| Entry { command, chord, overridden: stored.is_some() })
            .collect();
        ExtensionShortcuts { entries }
    }

    /// What the shortcut-capture dialog makes of `chord` pressed for an extension command, as
    /// [`Keymap::offer`]: the chord saving would assign, if any, and a note about its holder.
    pub fn offer_extension(
        &self,
        shortcuts: &ExtensionShortcuts,
        extension: &ExtensionId,
        name: &str,
        chord: Chord,
    ) -> (Option<Chord>, Option<String>) {
        if let Some(holder) = self.command_for(chord) {
            return (None, Some(format!("Used by {}, which an extension cannot take", holder.title())));
        }
        match shortcuts.command_for(chord).filter(|holder| !holder.is(extension, name)) {
            Some(holder) => (Some(chord), Some(holder.also_used_note())),
            None => (Some(chord), None),
        }
    }

    /// Gives the command exactly `chord` (`None` removes its shortcut), taking it from the
    /// extension command that holds it. A chord the browser holds is never an extension's, so
    /// assigning one does nothing.
    pub fn assign_extension(&mut self, shortcuts: &ExtensionShortcuts, extension: &ExtensionId, name: &str, chord: Option<Chord>) {
        let Some(target) = shortcuts.entry(extension, name) else { return };
        if chord.is_some_and(|chord| self.command_for(chord).is_some()) {
            return;
        }
        if let Some(chord) = chord {
            self.take_extension(shortcuts, &target.command, chord);
        }
        self.unknown.insert(target.command.id(), chord.iter().map(Chord::to_string).collect());
        self.drop_restated_extensions(shortcuts);
    }

    /// Drops the command's override, so its suggested key applies again, taken from the
    /// extension command that holds it.
    pub fn reset_extension(&mut self, shortcuts: &ExtensionShortcuts, extension: &ExtensionId, name: &str) {
        let Some(target) = shortcuts.entry(extension, name) else { return };
        if let Some(chord) = target.command.command.suggested_key.filter(|&chord| self.command_for(chord).is_none()) {
            self.take_extension(shortcuts, &target.command, chord);
        }
        self.unknown.remove(&target.command.id());
        self.drop_restated_extensions(shortcuts);
    }

    /// Every other command that could claim `chord`, by its override or its suggested key,
    /// keeps the rest of its stored chords as an override, so it does not get `chord` back
    /// when `target` lets go of it.
    fn take_extension(&mut self, shortcuts: &ExtensionShortcuts, target: &ExtensionCommand, chord: Chord) {
        for e in shortcuts.entries.iter().filter(|e| e.command != *target) {
            let id = e.command.id();
            let claims = match self.unknown.get(&id) {
                Some(strings) => strings.iter().any(|s| s.parse() == Ok(chord)),
                None => e.command.command.suggested_key == Some(chord),
            };
            if claims {
                let mut rest = self.unknown.remove(&id).unwrap_or_default();
                rest.retain(|s| s.parse() != Ok(chord));
                self.unknown.insert(id, rest);
            }
        }
    }

    /// Drops each override that only restates its command's suggested key where dropping it
    /// leaves every command's chord as it is, as the keymap does for the browser's commands.
    /// An empty one stays even then: the user removed the shortcut, and an update that adds a
    /// suggested key does not bring one back.
    fn drop_restated_extensions(&mut self, shortcuts: &ExtensionShortcuts) {
        let commands = shortcuts.commands();
        let resolved = self.extension_shortcuts(commands.clone()).chords();
        for command in &commands {
            let id = command.id();
            let restated = command.command.suggested_key.is_some_and(|key| self.unknown.get(&id).is_some_and(|strings| *strings == [key.to_string()]));
            if restated {
                let stored = self.unknown.remove(&id).expect("restated overrides are present");
                if self.extension_shortcuts(commands.clone()).chords() != resolved {
                    self.unknown.insert(id, stored);
                }
            }
        }
    }
}

impl Profile {
    /// The stored keymap resolved for every enabled extension's commands.
    pub fn extension_shortcuts(&mut self) -> Result<ExtensionShortcuts, Error> {
        let commands = extension_commands(&self.extensions().list()?);
        Ok(self.prefs().keymap().extension_shortcuts(commands))
    }
}
