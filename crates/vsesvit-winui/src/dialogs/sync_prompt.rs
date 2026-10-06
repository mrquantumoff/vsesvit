//! What sync asks of its own accord, in a `ContentDialog` over the browser window
//! (`vsesvit_sync::status::prompt`): once, whether to encrypt an unencrypted account with a
//! passphrase, and the account's passphrase for as long as the device waits for it. One prompt
//! shows at a time, never over another dialog, and only while the sync state asks for it.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use vsesvit_core::prefs::keys;
use vsesvit_core::private::Browsing;
use vsesvit_sync::Encryption;
use vsesvit_sync::status::{OFFER, PassphraseDialog, Prompt, State, passphrase_dialog, prompt};
use windows_core::{Interface, Result};

use super::sync_settings::{PassphraseForm, fields_markup};
use crate::bindings::*;
use crate::browser::Browser;
use crate::window::BrowserWindow;
use crate::{exec, sync, xaml};

/// How long the first window may take to be able to hold a dialog.
const FIRST_WINDOW_WAIT: Duration = Duration::from_secs(30);

/// What a prompt asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asks {
    /// In [`OFFER`]'s words.
    Offer,
    /// In [`passphrase_dialog`]'s words for this: a new passphrase, after the offer, or the
    /// account's.
    Passphrase(Encryption),
}

/// A browser's sync prompt.
#[derive(Default)]
pub(crate) struct SyncPrompt {
    shown: RefCell<Option<Rc<PromptDialog>>>,
    /// Registered with the sync state by [`follow`].
    listener: OnceCell<Rc<dyn Fn()>>,
}

/// Shows the prompt the sync state asks for once the first window can hold a dialog, and again
/// after every change of the state.
pub(crate) fn follow(browser: &Rc<Browser>) {
    let weak = Rc::downgrade(browser);
    let listener: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(browser) = weak.upgrade() {
            update(&browser);
        }
    });
    browser.sync().on_change(&listener);
    let _ = browser.sync_prompt().listener.set(listener);
    let browser = Rc::downgrade(browser);
    exec::spawn(async move {
        let ready = exec::wait_for(FIRST_WINDOW_WAIT, Duration::from_millis(50), || {
            browser.upgrade()?.windows().first()?.xaml_root().ok()
        })
        .await;
        if ready.is_some()
            && let Some(browser) = browser.upgrade()
        {
            update(&browser);
        }
    });
}

/// Shows, keeps or closes the prompt as the sync state now asks, over a normal window: never a
/// private one.
pub(crate) fn update(browser: &Browser) {
    let windows = browser.windows_of(Browsing::Normal);
    let slot = &browser.sync_prompt().shown;
    let shown = slot.borrow().clone().filter(|shown| {
        shown
            .window
            .upgrade()
            .is_some_and(|w| windows.iter().any(|open| Rc::ptr_eq(open, &w)))
    });
    if shown.is_none() {
        slot.take();
    }
    let state = browser.sync().state();
    let offered = browser.core(|p| p.prefs().get(&keys::SYNC_PASSPHRASE_OFFERED));
    let held = windows.iter().any(|w| w.has_dialog());
    match step(&state, offered, shown.as_ref().map(|s| s.asks), held) {
        Step::Stay => {}
        Step::Close => {
            if let Some(shown) = shown {
                shown.close();
            }
        }
        Step::Show(asks) => {
            let active = windows
                .iter()
                .find(|w| w.is_foreground())
                .or(windows.last());
            if let Some(window) = active {
                show(window, asks);
            }
        }
    }
}

/// Settings' Enter Passphrase…: the prompt asking for it, brought forward, or shown over
/// `window`, or the last normal window when that is private.
pub(crate) fn enter_passphrase(browser: &Browser, window: Option<Rc<BrowserWindow>>) {
    let shown = browser.sync_prompt().shown.borrow().clone();
    let host = match shown {
        Some(shown) => shown.window.upgrade(),
        None => {
            let windows = browser.windows_of(Browsing::Normal);
            let held = windows.iter().any(|w| w.has_dialog());
            let state = browser.sync().state();
            let window = window
                .filter(|w| w.browsing() == Browsing::Normal)
                .or_else(|| windows.last().cloned());
            if let (Step::Show(asks), Some(window)) = (step(&state, true, None, held), &window) {
                show(window, asks);
            }
            window
        }
    };
    if let Some(host) = host
        && browser.config().mode.is_interactive()
    {
        host.activate();
    }
}

/// The prompt shown, for the scripted runs.
#[cfg(feature = "self-test")]
pub(crate) fn shown(browser: &Browser) -> Option<Rc<PromptDialog>> {
    browser.sync_prompt().shown.borrow().clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Stay,
    Close,
    Show(Asks),
}

/// What becomes of the prompt: `shown` is the one showing, and `held` says another dialog is
/// open over a browser window, after which the prompt shows.
fn step(state: &State, offered: bool, shown: Option<Asks>, held: bool) -> Step {
    if let Some(shown) = shown {
        return if fits(shown, state) {
            Step::Stay
        } else {
            Step::Close
        };
    }
    match (prompt(state, offered), state) {
        (Some(_), _) if held => Step::Stay,
        (Some(Prompt::Offer), _) => Step::Show(Asks::Offer),
        (Some(Prompt::Enter), State::SignedIn { encryption, .. }) => {
            Step::Show(Asks::Passphrase(*encryption))
        }
        _ => Step::Stay,
    }
}

/// Whether the state still asks what `asks` asks.
fn fits(asks: Asks, state: &State) -> bool {
    let State::SignedIn {
        needs_sign_in: false,
        encryption,
        ..
    } = state
    else {
        return false;
    };
    match asks {
        Asks::Offer => *encryption == Encryption::Off,
        Asks::Passphrase(asked) => *encryption == asked,
    }
}

/// Signing out is the only other way out of a prompt for the account's passphrase.
fn must_answer(asks: Asks) -> bool {
    matches!(
        asks,
        Asks::Passphrase(Encryption::Enter | Encryption::Changed)
    )
}

/// What the user did to close a prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// The passphrase prompt stays open while it takes what was typed.
    Take,
    /// It stays open: only entering the passphrase or signing out closes it.
    Stay,
    /// Accepted the offer: the prompt for a new passphrase follows.
    SetPassphrase,
    /// Declined or closed the offer.
    NotNow,
    Cancel,
    SignOut,
}

/// `result` is the button pressed; `None` is the Close button, Escape or `Hide`.
fn answer(asks: Asks, result: ContentDialogResult) -> Answer {
    match (asks, result) {
        (Asks::Offer, ContentDialogResult::Primary) => Answer::SetPassphrase,
        (Asks::Offer, _) => Answer::NotNow,
        (Asks::Passphrase(_), ContentDialogResult::Primary) => Answer::Take,
        (asks, ContentDialogResult::Secondary) if must_answer(asks) => Answer::SignOut,
        (asks, _) if must_answer(asks) => Answer::Stay,
        (Asks::Passphrase(_), _) => Answer::Cancel,
    }
}

/// The prompt's markup. Escape closes a `ContentDialog` as its Close button does, so a prompt that
/// must be answered signs out with the secondary button and has no Close button.
fn markup(asks: Asks, words: Option<&PassphraseDialog>) -> String {
    let (title, body, accept, dismiss, content) = match words {
        None => {
            let (title, body, accept, decline) = OFFER;
            (title, body, accept, decline, String::new())
        }
        Some(words) => (
            words.title,
            words.body,
            words.accept,
            words.dismiss,
            format!(
                r#"{}
    <ProgressRing x:Name="SyncPassphraseBusy" Width="20" Height="20" IsActive="True" Visibility="Collapsed"
                  HorizontalAlignment="Left"/>"#,
                fields_markup(words)
            ),
        ),
    };
    let (dismiss_button, enabled) = match asks {
        Asks::Offer => ("CloseButtonText", ""),
        asks if must_answer(asks) => ("SecondaryButtonText", r#" IsPrimaryButtonEnabled="False""#),
        Asks::Passphrase(_) => ("CloseButtonText", r#" IsPrimaryButtonEnabled="False""#),
    };
    format!(
        r#"<ContentDialog {{ns}} Title="{}" PrimaryButtonText="{}" {dismiss_button}="{}" DefaultButton="Primary"{enabled}
             Style="{{StaticResource DefaultContentDialogStyle}}">
  <StackPanel Spacing="8">
    <TextBlock x:Name="SyncPromptBody" TextWrapping="Wrap" Text="{}"/>
    {content}
  </StackPanel>
</ContentDialog>"#,
        xaml::escape(title),
        xaml::escape(accept),
        xaml::escape(dismiss),
        xaml::escape(body)
    )
}

/// Opens the prompt for `asks` over `window`, unless a dialog is open over it.
fn show(window: &Rc<BrowserWindow>, asks: Asks) {
    let Some(browser) = window.browser() else {
        return;
    };
    if !window.begin_dialog() {
        return;
    }
    match PromptDialog::open(&browser, window, asks) {
        Ok(prompt) => {
            *browser.sync_prompt().shown.borrow_mut() = Some(prompt.clone());
            exec::spawn(prompt.run(window.clone()));
        }
        Err(e) => {
            log::warn!("sync prompt: {e}");
            window.end_dialog();
        }
    }
}

/// A prompt over a browser window.
pub(crate) struct PromptDialog {
    asks: Asks,
    window: Weak<BrowserWindow>,
    dialog: ContentDialog,
    /// For a passphrase, its fields and the ring shown while it is checked.
    form: Option<(PassphraseForm, UIElement)>,
    /// How the user closed it; `None` while it is open, and when `close` closed it.
    answer: Cell<Option<Answer>>,
    /// `close` is closing it.
    released: Cell<bool>,
    /// Set while the passphrase is checked or stored.
    working: Cell<bool>,
    closed: Cell<bool>,
}

impl PromptDialog {
    fn open(browser: &Browser, window: &Rc<BrowserWindow>, asks: Asks) -> Result<Rc<Self>> {
        let words = match asks {
            Asks::Offer => None,
            Asks::Passphrase(encryption) => {
                Some(passphrase_dialog(encryption).ok_or_else(|| {
                    windows_core::Error::new(E_FAIL, format!("nothing to ask for {encryption:?}"))
                })?)
            }
        };
        let dialog: ContentDialog = xaml::load(&markup(asks, words.as_ref()))?;
        dialog
            .cast::<UIElement>()?
            .SetXamlRoot(&window.xaml_root()?)?;
        let root = dialog.cast::<FrameworkElement>()?;
        root.SetRequestedTheme(super::element_theme(browser.theme()))?;
        let form = match &words {
            Some(words) => Some((
                PassphraseForm::find(&root, words)?,
                xaml::find(&root, "SyncPassphraseBusy")?,
            )),
            None => None,
        };
        let this = Rc::new(PromptDialog {
            asks,
            window: Rc::downgrade(window),
            dialog,
            form,
            answer: Cell::new(None),
            released: Cell::new(false),
            working: Cell::new(false),
            closed: Cell::new(false),
        });
        if let Some((form, _)) = &this.form {
            let me = Rc::downgrade(&this);
            form.on_change(move || {
                if let Some(me) = me.upgrade() {
                    me.validate();
                }
            })?;
        }
        let me = Rc::downgrade(&this);
        this.dialog
            .Closing(move |_, args| {
                if let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) {
                    me.closing(args);
                }
            })?
            .forget();
        Ok(this)
    }

    /// Shows the prompt until it closes, then does what its answer asks and shows the next
    /// prompt, if any. A prompt that could not show waits for the next change of the state.
    async fn run(self: Rc<Self>, window: Rc<BrowserWindow>) {
        let shown = match self.dialog.ShowAsync() {
            Ok(showing) => showing.await.map(drop),
            Err(e) => Err(e),
        };
        self.closed.set(true);
        window.end_dialog();
        let Some(browser) = window.browser() else {
            return;
        };
        let slot = &browser.sync_prompt().shown;
        if slot.borrow().as_ref().is_some_and(|s| Rc::ptr_eq(s, &self)) {
            slot.take();
        }
        if let Err(e) = shown {
            log::warn!("sync prompt: {e}");
            return;
        }
        match self.answer.get() {
            Some(Answer::SetPassphrase | Answer::NotNow) => {
                browser.write_pref(&keys::SYNC_PASSPHRASE_OFFERED, &true);
            }
            Some(Answer::SignOut) => sync::sign_out(&browser),
            _ => {}
        }
        let set = Asks::Passphrase(Encryption::Off);
        if self.answer.get() == Some(Answer::SetPassphrase) && fits(set, &browser.sync().state()) {
            show(&window, set);
        }
        update(&browser);
    }

    fn closing(self: &Rc<Self>, args: &ContentDialogClosingEventArgs) {
        if self.released.get() {
            return;
        }
        let result = args.Result().unwrap_or(ContentDialogResult::None);
        match answer(self.asks, result) {
            Answer::Take => {
                let _ = args.SetCancel(true);
                self.take();
            }
            Answer::Stay => {
                let _ = args.SetCancel(true);
            }
            answer => self.answer.set(Some(answer)),
        }
    }

    fn validate(&self) {
        let valid = self.form.as_ref().is_some_and(|(form, _)| form.validate());
        let _ = self
            .dialog
            .SetIsPrimaryButtonEnabled(valid && !self.working.get());
    }

    /// Takes the passphrase, busy meanwhile, and closes once it is taken; else says why not.
    fn take(self: &Rc<Self>) {
        let (Some((form, busy)), Asks::Passphrase(asked)) = (&self.form, self.asks) else {
            return;
        };
        let browser = self.window.upgrade().and_then(|w| w.browser());
        let (Ok(passphrase), Some(browser)) = (form.checked(), browser) else {
            return;
        };
        if self.working.replace(true) {
            return;
        }
        let _ = self.dialog.SetIsPrimaryButtonEnabled(false);
        let _ = xaml::set_visible(busy, true);
        let me = Rc::downgrade(self);
        sync::set_passphrase(&browser, asked, passphrase, move |error| {
            let Some(me) = me.upgrade().filter(|me| !me.closed.get()) else {
                return;
            };
            match (error, &me.form) {
                (None, _) => me.close(),
                (Some(error), Some((form, busy))) => {
                    me.working.set(false);
                    let _ = xaml::set_visible(busy, false);
                    me.validate();
                    form.show_problem(Some(&error));
                }
                (Some(_), None) => {}
            }
        });
    }

    /// Closes it without an answer: the state no longer asks it, or it took the passphrase.
    pub(crate) fn close(&self) {
        if !self.released.replace(true) {
            let _ = self.dialog.Hide();
        }
    }
}

#[cfg(feature = "self-test")]
impl PromptDialog {
    pub(crate) fn asks(&self) -> Asks {
        self.asks
    }

    pub(crate) fn is_open(&self) -> bool {
        !self.closed.get()
    }

    /// The body, then the accept and dismiss buttons' text, as the dialog shows them.
    pub(crate) fn words(&self) -> (String, String, String) {
        let text = |text: Result<String>| text.unwrap_or_default();
        let body = self
            .dialog
            .cast()
            .ok()
            .and_then(|d| xaml::find_named::<TextBlock>(&d, "SyncPromptBody"))
            .and_then(|b| b.Text().ok())
            .map(|t| t.to_string());
        let dismiss = match self.dialog.SecondaryButtonText() {
            Ok(text) if !text.is_empty() => text,
            _ => text(self.dialog.CloseButtonText()),
        };
        (
            body.unwrap_or_default(),
            text(self.dialog.PrimaryButtonText()),
            dismiss,
        )
    }

    pub(crate) fn form(&self) -> Option<&PassphraseForm> {
        self.form.as_ref().map(|(form, _)| form)
    }

    /// One of the dialog's own buttons (`PrimaryButton`, `SecondaryButton` or `CloseButton`), for
    /// the scripted runs to press.
    pub(crate) fn button(&self, name: &str) -> Option<Button> {
        xaml::find_named(&self.dialog.cast().ok()?, name)
    }

    /// Asks it to close the way Escape does, for the scripted runs.
    pub(crate) fn dismiss(&self) {
        let _ = self.dialog.Hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_in(encryption: Encryption, needs_sign_in: bool) -> State {
        State::SignedIn {
            name: None,
            server: "https://sync.example.com".into(),
            last_synced: None,
            syncing: false,
            error: None,
            needs_sign_in,
            encryption,
        }
    }

    #[test]
    fn a_prompt_waits_for_the_dialog_open_and_shows_only_while_signed_in() {
        let off = signed_in(Encryption::Off, false);
        assert_eq!(step(&off, false, None, false), Step::Show(Asks::Offer));
        assert_eq!(
            step(&off, false, None, true),
            Step::Stay,
            "the welcome is open"
        );
        assert_eq!(step(&off, true, None, false), Step::Stay, "offered once");
        for encryption in [Encryption::Enter, Encryption::Changed] {
            let state = signed_in(encryption, false);
            assert_eq!(
                step(&state, true, None, false),
                Step::Show(Asks::Passphrase(encryption))
            );
        }
        let signed_out = State::SignedOut { error: None };
        assert_eq!(step(&signed_out, false, None, false), Step::Stay);
        let expired = signed_in(Encryption::Enter, true);
        assert_eq!(step(&expired, true, None, false), Step::Stay);
    }

    #[test]
    fn a_prompt_stays_while_the_state_asks_it_and_never_stacks() {
        let off = signed_in(Encryption::Off, false);
        let enter = signed_in(Encryption::Enter, false);
        assert_eq!(step(&off, false, Some(Asks::Offer), true), Step::Stay);
        let set = Some(Asks::Passphrase(Encryption::Off));
        assert_eq!(
            step(&off, true, set, true),
            Step::Stay,
            "set after the offer"
        );
        assert_eq!(step(&enter, true, Some(Asks::Offer), true), Step::Close);
        let entering = Some(Asks::Passphrase(Encryption::Enter));
        assert_eq!(step(&enter, true, entering, true), Step::Stay);
        let changed = signed_in(Encryption::Changed, false);
        assert_eq!(
            step(&changed, true, entering, true),
            Step::Close,
            "reworded"
        );
        let ready = signed_in(Encryption::Ready, false);
        assert_eq!(step(&ready, true, entering, true), Step::Close);
        let signed_out = State::SignedOut { error: None };
        assert_eq!(step(&signed_out, true, entering, true), Step::Close);
    }

    #[test]
    fn the_passphrase_prompt_closes_only_by_entering_it_or_signing_out() {
        let enter = Asks::Passphrase(Encryption::Enter);
        assert_eq!(answer(enter, ContentDialogResult::Primary), Answer::Take);
        assert_eq!(
            answer(enter, ContentDialogResult::Secondary),
            Answer::SignOut
        );
        assert_eq!(answer(enter, ContentDialogResult::None), Answer::Stay);
        let changed = Asks::Passphrase(Encryption::Changed);
        assert_eq!(answer(changed, ContentDialogResult::None), Answer::Stay);
        let set = Asks::Passphrase(Encryption::Off);
        assert_eq!(answer(set, ContentDialogResult::Primary), Answer::Take);
        assert_eq!(answer(set, ContentDialogResult::None), Answer::Cancel);
        assert_eq!(
            answer(Asks::Offer, ContentDialogResult::Primary),
            Answer::SetPassphrase
        );
        assert_eq!(
            answer(Asks::Offer, ContentDialogResult::None),
            Answer::NotNow
        );
    }

    #[test]
    fn the_prompts_words_come_from_core() {
        let offer = markup(Asks::Offer, None);
        let (title, _, accept, decline) = OFFER;
        for words in [title, accept, decline] {
            assert!(offer.contains(&format!(r#""{words}""#)), "{offer}");
        }
        assert!(!offer.contains("PasswordBox"));
        let words = passphrase_dialog(Encryption::Enter).unwrap();
        let enter = markup(Asks::Passphrase(Encryption::Enter), Some(&words));
        assert!(
            enter.contains(r#"SecondaryButtonText="Sign Out""#),
            "{enter}"
        );
        assert!(!enter.contains("CloseButtonText"), "{enter}");
        assert_eq!(enter.matches("<PasswordBox").count(), 1);
        let words = passphrase_dialog(Encryption::Off).unwrap();
        let set = markup(Asks::Passphrase(Encryption::Off), Some(&words));
        assert!(set.contains(r#"CloseButtonText="Cancel""#), "{set}");
        assert_eq!(set.matches("<PasswordBox").count(), 2);
    }
}
