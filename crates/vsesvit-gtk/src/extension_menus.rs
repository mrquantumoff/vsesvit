//! Extensions' own menu items (`chrome.contextMenus`), as the runtime hands them over, in the
//! page's context menu and in a toolbar action's menu.

use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use vsesvit_webext::menus::{Entry, ItemId};

/// What choosing one of an extension's items does.
pub(crate) type Choose = Rc<dyn Fn(&ItemId)>;

/// `title` as a menu label, which would otherwise read `_` as a mnemonic.
fn label(title: &str) -> String {
    title.replace('_', "__")
}

/// The label and action of an entry chosen like a command: an item, checkable for a checkbox
/// or radio item, which is how GTK and WebKit menus show both, or a disabled submenu, which
/// shows greyed out and never opens. `None` for a separator or a submenu that opens.
fn command(entry: &Entry, name: &str, choose: &Choose) -> Option<(String, gio::SimpleAction)> {
    match entry {
        Entry::Item { id, title, enabled, checked } => {
            let action = match checked {
                Some(checked) => gio::SimpleAction::new_stateful(name, None, &checked.to_variant()),
                None => gio::SimpleAction::new(name, None),
            };
            action.set_enabled(*enabled);
            let (id, choose) = (id.clone(), choose.clone());
            action.connect_activate(move |_, _| choose(&id));
            Some((label(title), action))
        }
        Entry::Submenu { title, enabled: false, .. } => {
            let action = gio::SimpleAction::new(name, None);
            action.set_enabled(false);
            Some((label(title), action))
        }
        Entry::Submenu { enabled: true, .. } | Entry::Separator => None,
    }
}

/// `entry` as an item of the page's context menu. WebKit keeps one menu's actions by name, so
/// `n` numbers them.
pub(crate) fn page_item(entry: &Entry, choose: &Choose, n: &mut usize) -> webkit::ContextMenuItem {
    *n += 1;
    if let Some((label, action)) = command(entry, &format!("extension-item-{n}"), choose) {
        return webkit::ContextMenuItem::from_gaction(&action, &label, None);
    }
    match entry {
        Entry::Submenu { title, children, .. } => {
            let submenu = webkit::ContextMenu::new();
            for child in children {
                submenu.append(&page_item(child, choose, n));
            }
            webkit::ContextMenuItem::with_submenu(&label(title), &submenu)
        }
        _ => webkit::ContextMenuItem::new_separator(),
    }
}

/// `entries` as a menu of sections, split at the separators, whose actions go into `group`,
/// which the menu's popover must offer as `prefix`.
pub(crate) fn model(entries: &[Entry], group: &gio::SimpleActionGroup, prefix: &str, choose: &Choose) -> gio::Menu {
    let menu = gio::Menu::new();
    let mut section = gio::Menu::new();
    for entry in entries {
        let name = format!("item-{}", group.list_actions().len());
        if let Some((label, action)) = command(entry, &name, choose) {
            group.add_action(&action);
            section.append(Some(&label), Some(&format!("{prefix}.{name}")));
            continue;
        }
        match entry {
            Entry::Submenu { title, children, .. } => section.append_submenu(Some(&label(title)), &model(children, group, prefix, choose)),
            _ => menu.append_section(None, &std::mem::replace(&mut section, gio::Menu::new())),
        }
    }
    menu.append_section(None, &section);
    menu
}
