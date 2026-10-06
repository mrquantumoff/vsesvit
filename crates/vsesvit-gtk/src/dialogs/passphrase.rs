//! The dialog asking for the sync passphrase, in the words `vsesvit_sync::status` gives for what
//! the account asks: setting one, entering it, or changing it.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use vsesvit_sync::Passphrase;
use vsesvit_sync::status::{PassphraseDialog, check_passphrase};

/// The password fields, a second one for a new passphrase, and what is wrong with what they hold.
struct Fields {
    content: gtk::Box,
    passphrase: adw::PasswordEntryRow,
    confirm: Option<adw::PasswordEntryRow>,
    problem: gtk::Label,
}

impl Fields {
    fn new(words: &PassphraseDialog, failed: Option<&str>) -> Self {
        let field = |title: &str| adw::PasswordEntryRow::builder().title(title).activates_default(true).build();
        let passphrase = field(words.field);
        let confirm = words.confirm.map(field);
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
        // Errors are worded to follow a colon; alone under the fields, one starts with a capital.
        let failed = failed.map(|e| {
            let mut chars = e.chars();
            chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
        });
        let problem = gtk::Label::builder()
            .label(failed.as_deref().unwrap_or_default())
            .visible(failed.is_some())
            .xalign(0.0)
            .wrap(true)
            .css_classes(["error"])
            .build();
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).build();
        content.append(&list);
        content.append(&problem);
        let fields = Fields { content, passphrase, confirm, problem };
        for row in fields.rows() {
            list.append(row);
        }
        fields
    }

    fn rows(&self) -> impl Iterator<Item = &adw::PasswordEntryRow> {
        std::iter::once(&self.passphrase).chain(&self.confirm)
    }

    fn check(&self) -> Result<Passphrase, &'static str> {
        check_passphrase(&self.passphrase.text(), self.confirm.as_ref().map(|c| c.text()).as_deref())
    }

    /// Says what is wrong with what was typed, but not that a confirmation not typed yet differs.
    /// Returns whether it can be accepted.
    fn validate(&self) -> bool {
        let checked = self.check();
        let confirming = self.confirm.as_ref().is_none_or(|c| !c.text().is_empty());
        let shown = if confirming { checked.as_ref().err().copied() } else { check_passphrase(&self.passphrase.text(), None).err() };
        self.problem.set_label(shown.unwrap_or_default());
        self.problem.set_visible(shown.is_some());
        checked.is_ok()
    }
}

/// Asks for the passphrase `words` describes. `failed` is why the last try failed, shown under the
/// fields until something is typed. `None` when cancelled.
pub(crate) async fn ask(parent: &impl IsA<gtk::Widget>, words: PassphraseDialog, failed: Option<&str>) -> Option<Passphrase> {
    let fields = Rc::new(Fields::new(&words, failed));
    let dialog = adw::AlertDialog::new(Some(words.title), Some(words.body));
    dialog.set_extra_child(Some(&fields.content));
    dialog.set_prefer_wide_layout(true);
    dialog.add_responses(&[("cancel", "_Cancel"), ("accept", words.accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("accept", false);
    for row in fields.rows() {
        row.connect_changed(glib::clone!(
            #[weak]
            dialog,
            #[weak]
            fields,
            move |_| dialog.set_response_enabled("accept", fields.validate())
        ));
    }
    fields.passphrase.grab_focus();
    if dialog.choose_future(Some(parent)).await != "accept" {
        return None;
    }
    fields.check().ok()
}
