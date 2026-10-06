//! The dialogs about the sync passphrase, in the words `vsesvit_sync::status` gives: the one
//! asking for it, to set it, enter it or change it, and the prompts sync shows of its own accord
//! over the browser window. Each closes once the account no longer asks what it asks.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::prefs::keys;
use vsesvit_sync::status::{Action, OFFER, PassphraseDialog, Prompt, State, check_passphrase, passphrase_dialog};
use vsesvit_sync::{Encryption, Passphrase};

use crate::sync::{self, Syncer};
use crate::window::BrowserWindow;

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

/// What became of a [`PassphraseDialog`].
enum Answer {
    Typed(Passphrase),
    Dismissed,
    /// Closed without an answer: the account no longer asks it.
    Gone,
}

/// Asks for the passphrase `encryption` asks for, in `words`. `failed` is why the last try failed,
/// shown under the fields until something is typed. A dialog whose dismiss button signs out closes
/// no other way.
async fn ask(parent: &impl IsA<gtk::Widget>, syncer: &Syncer, encryption: Encryption, words: PassphraseDialog, failed: Option<&str>) -> Answer {
    let fields = Rc::new(Fields::new(&words, failed));
    let dialog = adw::AlertDialog::new(Some(words.title), Some(words.body));
    dialog.set_extra_child(Some(&fields.content));
    dialog.set_prefer_wide_layout(true);
    dialog.add_responses(&[("dismiss", words.dismiss), ("accept", words.accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    if words.dismiss == Action::SignOut.label() {
        dialog.set_can_close(false);
        // A response closes it only through `adw_dialog_close`, which `can-close` stops.
        dialog.connect_response(None, |dialog, _| {
            dialog.force_close();
        });
    } else {
        dialog.set_close_response("dismiss");
    }
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
    let asks = move |state: &State| sync::encryption(state) == Some(encryption);
    match choose_while(dialog, parent, syncer, asks).await.as_deref() {
        Some("accept") => fields.check().map_or(Answer::Gone, Answer::Typed),
        Some(_) => Answer::Dismissed,
        None => Answer::Gone,
    }
}

/// Shows `dialog` over `parent` until it is answered, or, returning `None`, closes it once
/// `syncer`'s state no longer `asks` it or the window under it closes.
async fn choose_while(
    dialog: adw::AlertDialog,
    parent: &impl IsA<gtk::Widget>,
    syncer: &Syncer,
    asks: impl Fn(&State) -> bool + 'static,
) -> Option<glib::GString> {
    if !asks(&syncer.state()) {
        return None;
    }
    let ended = Rc::new(Cell::new(false));
    let end = Rc::new(glib::clone!(
        #[weak]
        dialog,
        #[strong]
        ended,
        move || {
            if !ended.replace(true) {
                dialog.force_close();
            }
        }
    ));
    syncer.watch(glib::clone!(
        #[strong]
        end,
        #[strong]
        ended,
        move |state: &State| {
            if !ended.get() && !asks(state) {
                end();
            }
            !ended.get()
        }
    ));
    // A window that closes leaves a dialog that cannot close unanswered.
    let window = parent.root();
    let closing = window.as_ref().map(|window| window.connect_unrealize(move |_| end()));
    let response = dialog.choose_future(Some(parent)).await;
    if let Some((window, closing)) = window.zip(closing) {
        window.disconnect(closing);
    }
    (!ended.replace(true)).then_some(response)
}

/// Asks for what the account asks of the passphrase over `parent` and runs the passphrase step
/// with the answer, `busy` meanwhile, asking again, saying why, while the step fails and the
/// account still asks the same. A dismiss button that says so signs out. `Err` is why the last
/// step failed.
pub(crate) async fn take(parent: &impl IsA<gtk::Widget>, syncer: &Syncer, busy: impl Fn(bool)) -> Result<(), String> {
    let Some((encryption, words)) = syncer.encryption().and_then(|e| Some((e, passphrase_dialog(e)?))) else { return Ok(()) };
    let mut failed = None;
    while syncer.encryption() == Some(encryption) {
        match ask(parent, syncer, encryption, words, failed.as_deref()).await {
            Answer::Typed(passphrase) => {
                busy(true);
                let taken = syncer.passphrase(passphrase).await;
                busy(false);
                match taken {
                    Ok(()) => return Ok(()),
                    Err(e) => failed = Some(e),
                }
            }
            Answer::Dismissed if words.dismiss == Action::SignOut.label() => {
                syncer.act(Action::SignOut);
                return Ok(());
            }
            Answer::Dismissed | Answer::Gone => return Ok(()),
        }
    }
    failed.map_or(Ok(()), Err)
}

/// Shows `prompt` over `window` and runs it to its end: the offer, then setting the passphrase if
/// it is accepted, or the passphrase dialog until the passphrase is entered or the device signs
/// out.
pub(crate) async fn prompt(window: &BrowserWindow, syncer: &Syncer, prompt: Prompt) -> Result<(), String> {
    if prompt == Prompt::Enter {
        return take(window, syncer, |_| {}).await;
    }
    let (title, body, accept, decline) = OFFER;
    let dialog = adw::AlertDialog::new(Some(title), Some(body));
    dialog.add_responses(&[("decline", decline), ("accept", accept)]);
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("decline");
    let offers = |state: &State| vsesvit_sync::status::prompt(state, false) == Some(Prompt::Offer);
    let Some(answer) = choose_while(dialog, window, syncer, offers).await else { return Ok(()) };
    window.browser().set_pref(&keys::SYNC_PASSPHRASE_OFFERED, &true);
    if answer == "accept" { take(window, syncer, |_| {}).await } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use vsesvit_sync::Account;

    use super::*;
    use crate::test_support::{browser, settle, wait_until};

    fn shown(browser: &crate::browser::Browser) -> Option<adw::Dialog> {
        browser.windows().iter().find_map(|w| w.visible_dialog())
    }

    #[gtk::test]
    fn the_offer_waits_for_the_welcome_and_comes_once() {
        let browser = browser();
        let window = BrowserWindow::new(&browser);
        window.present();
        let offered = browser.pref(&keys::SYNC_PASSPHRASE_OFFERED);
        browser.set_pref(&keys::SYNC_PASSPHRASE_OFFERED, &false);
        let account: Account = serde_json::from_value(serde_json::json!({
            "sign_in": "s", "server": "http://127.0.0.1:9", "name": null,
            "limits": { "max_batch": 1, "max_record_bytes": 1, "max_request_bytes": 1 },
            "download_cursor": 0, "upload_cursors": {}, "last_synced": null, "server_keys": "missing",
        }))
        .expect("an account");
        account.save(&mut browser.core().borrow_mut().sync()).expect("the account is saved");
        browser.core().borrow_mut().sync().set_secret_state("account.session", b"session").expect("the session is saved");

        let welcome = crate::dialogs::welcome::present(&window);
        browser.sync().reload();
        settle(Duration::from_millis(300));
        let over_welcome = shown(&browser);
        welcome.force_close();
        wait_until("the offer", || shown(&browser).and_downcast::<adw::AlertDialog>().is_some());
        let offer = shown(&browser).and_downcast::<adw::AlertDialog>().unwrap();
        let heading = offer.heading();
        offer.close();
        wait_until("closing the offer to answer it", || browser.pref(&keys::SYNC_PASSPHRASE_OFFERED));
        let closed = shown(&browser);
        browser.sync().reload();
        settle(Duration::from_millis(300));
        let again = shown(&browser);

        browser.sync().act(Action::SignOut);
        browser.set_pref(&keys::SYNC_PASSPHRASE_OFFERED, &offered);
        window.destroy();
        assert_eq!(over_welcome, Some(welcome.upcast()), "the offer waits for the welcome");
        assert_eq!(heading.as_deref(), Some(OFFER.0));
        assert_eq!(closed, None);
        assert_eq!(again, None, "the offer comes once");
    }

    #[gtk::test]
    fn a_dialog_that_cannot_close_ends_unanswered_when_it_is_moot_or_its_window_closes() {
        let browser = browser();
        let asks = Rc::new(Cell::new(true));
        let mut ended = Vec::new();
        for by in ["the state", "the window"] {
            let window = BrowserWindow::new(&browser);
            window.present();
            let dialog = adw::AlertDialog::new(Some(by), None);
            dialog.add_responses(&[("dismiss", "Sign Out")]);
            dialog.set_can_close(false);
            let answer = Rc::new(std::cell::RefCell::new(None));
            let (slot, syncer, asking, parent) = (answer.clone(), browser.sync().clone(), asks.clone(), window.clone());
            glib::spawn_future_local(async move {
                let response = choose_while(dialog, &parent, &syncer, move |_| asking.get()).await;
                slot.replace(Some(response));
            });
            settle(Duration::from_millis(200));
            if by == "the state" {
                asks.set(false);
                browser.sync().reload();
            } else {
                window.destroy();
            }
            wait_until(by, || answer.borrow().is_some());
            ended.push((by, answer.take().flatten()));
            asks.set(true);
            window.destroy();
        }
        assert_eq!(ended, [("the state", None), ("the window", None)]);
    }
}
