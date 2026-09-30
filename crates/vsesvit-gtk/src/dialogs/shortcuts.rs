use adw::prelude::*;
use vsesvit_core::shortcuts::{Command, Section};

use crate::keymap::{self, Binding};

/// Sections and titles come from core. Accelerators come from the actions themselves, so
/// this never drifts from what the keys do.
pub(crate) fn present(parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::ShortcutsDialog::new();
    for section in Section::ALL {
        let group = adw::ShortcutsSection::new(Some(section.title()));
        for &cmd in Command::ALL.iter().filter(|cmd| cmd.section() == section) {
            match keymap::binding(cmd) {
                Some(Binding::Action(action)) => group.add(adw::ShortcutsItem::from_action(cmd.title(), action)),
                Some(Binding::BuiltIn(accelerator)) => group.add(adw::ShortcutsItem::new(cmd.title(), accelerator)),
                None => {}
            }
        }
        dialog.add(group);
    }
    dialog.present(Some(parent));
}
