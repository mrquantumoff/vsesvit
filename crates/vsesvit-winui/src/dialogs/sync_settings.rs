//! Settings' Sync page: the account's status with its buttons (from `vsesvit_sync::status`),
//! following the sync state while the dialog is open, the passphrase flyout, what this device
//! syncs, and the server to sign in to. The server box also serves the welcome's sync page.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use vsesvit_core::prefs::{DEFAULT_SYNC_SERVER, keys};
use vsesvit_core::sync::DataType;
use vsesvit_sync::status::{
    Action, DELETE_CONFIRMATION, PassphraseDialog, State, Status, check_passphrase,
    passphrase_dialog,
};
use vsesvit_sync::{Encryption, server_input};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
use crate::bookmark_editor::VK_RETURN;
use crate::browser::Browser;
use crate::window::BrowserWindow;
use crate::{exec, sync, xaml};

pub(super) fn panel() -> String {
    format!(
        r#"
    <ScrollViewer x:Name="SyncPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="28" Padding="0,0,0,12">
        <StackPanel Spacing="4">
          <TextBlock x:Name="SyncTitle" TextWrapping="Wrap" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
          <TextBlock x:Name="SyncSubtitle" TextWrapping="Wrap" IsTextSelectionEnabled="True"
                     Style="{{StaticResource CaptionTextBlockStyle}}" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
          <StackPanel x:Name="SyncActions" Orientation="Horizontal" Spacing="8" Margin="0,8,0,0"/>
          <TextBlock x:Name="SyncActionError" TextWrapping="Wrap" Visibility="Collapsed" IsTextSelectionEnabled="True"
                     Style="{{StaticResource CaptionTextBlockStyle}}" Foreground="{{ThemeResource SystemFillColorCriticalBrush}}"/>
        </StackPanel>
        <StackPanel Spacing="4">
          <TextBlock Text="Customize sync" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
          <ToggleSwitch x:Name="SyncEverything" Header="Sync everything"/>
          <StackPanel x:Name="SyncTypes" Margin="12,0,0,0"/>
        </StackPanel>
        {server}
      </StackPanel>
    </ScrollViewer>"#,
        server = server_markup("Sync")
    )
}

/// The server box and where its error shows; `prefix` names them `{prefix}Server` and
/// `{prefix}ServerError`.
pub(super) fn server_markup(prefix: &str) -> String {
    format!(
        r#"<StackPanel Spacing="4">
          <TextBox x:Name="{prefix}Server" Header="Sync server" PlaceholderText="https://"/>
          <TextBlock x:Name="{prefix}ServerError" TextWrapping="Wrap" Visibility="Collapsed"
                     Style="{{StaticResource CaptionTextBlockStyle}}" Foreground="{{ThemeResource SystemFillColorCriticalBrush}}"/>
        </StackPanel>"#
    )
}

/// The confirmation's destructive button: the critical color, as Windows shows a destructive
/// choice.
const DELETE_BUTTON: &str = r#"<Button {ns} x:Name="SyncDeleteServerData" Content="LABEL">
  <Button.Flyout>
    <Flyout x:Name="SyncDeleteFlyout" Placement="BottomEdgeAlignedLeft">
      <StackPanel Width="360" Spacing="12">
        <TextBlock TextWrapping="Wrap" Text="TITLE" Style="{StaticResource BodyStrongTextBlockStyle}"/>
        <TextBlock TextWrapping="Wrap" Text="BODY"/>
        <Button x:Name="SyncDeleteConfirm" Content="CONFIRM"
                Background="{ThemeResource SystemFillColorCriticalBrush}"
                Foreground="{ThemeResource TextOnAccentFillColorPrimaryBrush}"/>
      </StackPanel>
    </Flyout>
  </Button.Flyout>
</Button>"#;

pub(crate) struct Page {
    browser: Weak<Browser>,
    /// Where the sign-in page opens, brought over the Settings window then.
    window: Weak<BrowserWindow>,
    title: TextBlock,
    subtitle: TextBlock,
    actions: Panel,
    /// The buttons `actions` holds, kept while the actions stay the same so that a flyout opened
    /// from one stays where it is.
    buttons: RefCell<Vec<(Action, Button)>>,
    action_error: TextBlock,
    /// The passphrase flyout while it is open.
    passphrase: RefCell<Option<Rc<PassphraseFlyout>>>,
    everything: ToggleSwitch,
    types: Vec<(DataType, ToggleSwitch)>,
    /// "Sync everything" was turned off here, so the types can be chosen one by one even while
    /// all of them are on.
    customizing: Cell<bool>,
    pub(super) server: Rc<ServerBox>,
    /// Registered with the sync state; the page follows it while this is kept.
    follow: Rc<dyn Fn()>,
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Rc<Page>> {
    let list: Panel = xaml::find(root, "SyncTypes")?;
    let children = list.Children()?;
    let mut types = Vec::new();
    for data_type in DataType::ALL {
        let switch: ToggleSwitch = xaml::load(&format!(
            r#"<ToggleSwitch {{ns}} x:Name="SyncType{data_type:?}" Header="{}"/>"#,
            xaml::escape(data_type.label())
        ))?;
        children.Append(&switch.cast::<UIElement>()?)?;
        types.push((data_type, switch));
    }
    let (title, subtitle, actions, action_error, everything) = (
        xaml::find(root, "SyncTitle")?,
        xaml::find(root, "SyncSubtitle")?,
        xaml::find(root, "SyncActions")?,
        xaml::find(root, "SyncActionError")?,
        xaml::find(root, "SyncEverything")?,
    );
    let server = ServerBox::wire(root, browser, "Sync")?;
    let page = Rc::new_cyclic(|me: &Weak<Page>| {
        let me = me.clone();
        Page {
            browser: Rc::downgrade(browser),
            window: Rc::downgrade(window),
            title,
            subtitle,
            actions,
            buttons: RefCell::default(),
            action_error,
            passphrase: RefCell::default(),
            everything,
            types,
            customizing: Cell::new(false),
            server,
            follow: Rc::new(move || {
                if let Some(page) = me.upgrade() {
                    page.show();
                }
            }),
        }
    });
    let p = Rc::downgrade(&page);
    page.everything
        .Toggled(move |_, _| {
            if let Some(p) = p.upgrade() {
                p.everything_toggled();
            }
        })?
        .forget();
    for (data_type, switch) in &page.types {
        let (p, data_type, source) = (Rc::downgrade(&page), *data_type, switch.clone());
        switch
            .Toggled(move |_, _| {
                if let (Some(p), Ok(on)) = (p.upgrade(), source.IsOn()) {
                    p.type_toggled(data_type, on);
                }
            })?
            .forget();
    }
    page.show();
    browser.sync().on_change(&page.follow);
    Ok(page)
}

impl Page {
    fn show(self: &Rc<Self>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let status = browser.sync().status();
        let _ = self.title.SetText(&status.title);
        let _ = self.subtitle.SetText(&status.subtitle);
        self.server.set_editable(status.server_editable);
        if let Err(e) = self.fill_actions(&status) {
            log::warn!("sync buttons: {e}");
        }
        self.show_types(&browser);
    }

    fn show_types(&self, browser: &Browser) {
        let stored = browser.core(|p| p.prefs().get(&keys::SYNC_TYPES));
        let (everything, switches) = type_switches(&stored, self.customizing.get());
        let _ = self.everything.SetIsOn(everything);
        for ((_, switch), (on, enabled)) in self.types.iter().zip(switches) {
            let _ = switch.SetIsOn(on);
            let _ = switch
                .cast::<Control>()
                .and_then(|c| c.SetIsEnabled(enabled));
        }
    }

    fn everything_toggled(&self) {
        let (Some(browser), Ok(on)) = (self.browser.upgrade(), self.everything.IsOn()) else {
            return;
        };
        let stored = browser.core(|p| p.prefs().get(&keys::SYNC_TYPES));
        if on == type_switches(&stored, self.customizing.get()).0 {
            return;
        }
        self.customizing.set(!on);
        if on && stored != DataType::ALL {
            sync::set_types(&browser, &DataType::ALL);
        }
        self.show_types(&browser);
    }

    fn type_toggled(&self, data_type: DataType, on: bool) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let stored = browser.core(|p| p.prefs().get(&keys::SYNC_TYPES));
        if on == stored.contains(&data_type) {
            return;
        }
        sync::set_types(&browser, &DataType::toggled(&stored, data_type, on));
    }

    /// One button per action, the first one accented. Deleting the server's data asks first, and
    /// the passphrase is typed, in a flyout: no second dialog can open over this one.
    fn fill_actions(self: &Rc<Self>, status: &Status) -> Result<()> {
        let shown: Vec<Action> = self.buttons.borrow().iter().map(|(a, _)| *a).collect();
        if shown != status.actions {
            self.make_buttons(status)?;
        }
        for (action, button) in self.buttons.borrow().iter() {
            button
                .cast::<Control>()?
                .SetIsEnabled(!(*action == Action::SyncNow && status.busy))?;
        }
        Ok(())
    }

    fn make_buttons(self: &Rc<Self>, status: &Status) -> Result<()> {
        let children = self.actions.Children()?;
        children.Clear()?;
        let mut buttons = Vec::new();
        for (index, &action) in status.actions.iter().enumerate() {
            let button: Button = if action == Action::DeleteServerData {
                self.delete_button()?
            } else {
                let style = if index == 0 {
                    r#" Style="{StaticResource AccentButtonStyle}""#
                } else {
                    ""
                };
                let button: Button = xaml::load(&format!(
                    r#"<Button {{ns}} x:Name="Sync{action:?}" Content="{}"{style}/>"#,
                    xaml::escape(action.label())
                ))?;
                let (p, anchor) = (Rc::downgrade(self), button.cast::<FrameworkElement>()?);
                on_click(&button, move || {
                    let (p, anchor) = (p.clone(), anchor.clone());
                    // Not from inside the click of a button the new state removes.
                    exec::spawn(async move {
                        if let Some(p) = p.upgrade() {
                            p.act(action, &anchor);
                        }
                    });
                })?;
                button
            };
            children.Append(&button.cast::<UIElement>()?)?;
            buttons.push((action, button));
        }
        *self.buttons.borrow_mut() = buttons;
        Ok(())
    }

    fn delete_button(self: &Rc<Self>) -> Result<Button> {
        let (title, body, confirm) = DELETE_CONFIRMATION;
        let button: Button = xaml::load(
            &DELETE_BUTTON
                .replacen("LABEL", &xaml::escape(Action::DeleteServerData.label()), 1)
                .replacen("TITLE", &xaml::escape(title), 1)
                .replacen("BODY", &xaml::escape(body), 1)
                .replacen("CONFIRM", &xaml::escape(confirm), 1),
        )?;
        let flyout = button.cast::<IButton>()?.Flyout()?;
        let confirm: Button = xaml::find(&button.cast()?, "SyncDeleteConfirm")?;
        let p = Rc::downgrade(self);
        on_click(&confirm, move || {
            let _ = flyout.Hide();
            let p = p.clone();
            exec::spawn(async move {
                if let Some(p) = p.upgrade() {
                    p.delete_server_data();
                }
            });
        })?;
        Ok(button)
    }

    fn delete_server_data(self: &Rc<Self>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        self.show_action_error(None);
        let p = Rc::downgrade(self);
        sync::delete_server_data(&browser, move |error| {
            if let Some(p) = p.upgrade() {
                p.show_action_error(error.as_deref());
            }
        });
    }

    fn show_action_error(&self, error: Option<&str>) {
        let _ = self.action_error.SetText(error.unwrap_or_default());
        let _ = xaml::set_visible(&self.action_error, error.is_some());
    }

    fn act(self: &Rc<Self>, action: Action, anchor: &FrameworkElement) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        self.show_action_error(None);
        match action {
            Action::SignIn => {
                if !self.server.apply() {
                    return;
                }
                let (window, interactive) =
                    (self.window.clone(), browser.config().mode.is_interactive());
                sync::sign_in(&browser, self.window.clone(), move || {
                    if let Some(window) = window.upgrade()
                        && interactive
                    {
                        window.activate();
                    }
                });
            }
            Action::Cancel => sync::cancel_sign_in(&browser),
            Action::SyncNow => sync::sync_now(&browser),
            Action::SignOut => sync::sign_out(&browser),
            Action::SetPassphrase | Action::EnterPassphrase | Action::ChangePassphrase => {
                self.open_passphrase(&browser, anchor);
            }
            Action::DeleteServerData => {}
        }
    }

    /// Opens the passphrase flyout under `anchor`, worded for what the device asks of the
    /// passphrase.
    fn open_passphrase(self: &Rc<Self>, browser: &Browser, anchor: &FrameworkElement) {
        let State::SignedIn { encryption, .. } = browser.sync().state() else {
            return;
        };
        let Some(dialog) = passphrase_dialog(encryption) else {
            return;
        };
        match PassphraseFlyout::open(self, browser, encryption, &dialog, anchor) {
            Ok(flyout) => *self.passphrase.borrow_mut() = Some(flyout),
            Err(e) => log::warn!("sync passphrase flyout: {e}"),
        }
    }

    /// The passphrase flyout once it has opened.
    pub(crate) fn passphrase(&self) -> Option<Rc<PassphraseFlyout>> {
        self.passphrase.borrow().clone().filter(|f| f.opened.get())
    }
}

const PASSPHRASE: &str = r#"<Flyout {ns} Placement="BottomEdgeAlignedLeft"/>"#;

/// The flyout's content: `dialog`'s words, its fields (a PasswordBox peeks at what was typed),
/// where a problem shows, and the buttons.
fn passphrase_markup(dialog: &PassphraseDialog) -> String {
    let confirm = dialog.confirm.map_or(String::new(), |label| {
        format!(
            r#"<PasswordBox x:Name="SyncPassphraseConfirm" Header="{}"/>"#,
            xaml::escape(label)
        )
    });
    format!(
        r#"
<StackPanel {{ns}} Width="380" Spacing="8">
  <TextBlock Text="{title}" TextWrapping="Wrap" Margin="0,0,0,4" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
  <TextBlock Text="{body}" TextWrapping="Wrap"/>
  <PasswordBox x:Name="SyncPassphrase" Header="{field}"/>
  {confirm}
  <TextBlock x:Name="SyncPassphraseProblem" TextWrapping="Wrap" Visibility="Collapsed" IsTextSelectionEnabled="True"
             Style="{{StaticResource CaptionTextBlockStyle}}" Foreground="{{ThemeResource SystemFillColorCriticalBrush}}"/>
  <StackPanel Orientation="Horizontal" Spacing="8" Margin="0,4,0,0" HorizontalAlignment="Right">
    <ProgressRing x:Name="SyncPassphraseBusy" Width="20" Height="20" IsActive="True" Visibility="Collapsed"/>
    <Button x:Name="SyncPassphraseAccept" Content="{accept}" Style="{{StaticResource AccentButtonStyle}}" IsEnabled="False"/>
    <Button x:Name="SyncPassphraseCancel" Content="Cancel"/>
  </StackPanel>
</StackPanel>"#,
        title = xaml::escape(dialog.title),
        body = xaml::escape(dialog.body),
        field = xaml::escape(dialog.field),
        accept = xaml::escape(dialog.accept),
    )
}

/// The flyout that sets, enters or changes the sync passphrase. It stays open while the
/// passphrase is checked, and shows why it was not taken.
pub(crate) struct PassphraseFlyout {
    flyout: FlyoutBase,
    /// Shown, with its fields loaded; the scripted runs type into it only then.
    opened: Cell<bool>,
    /// What the device asked of the passphrase when the flyout opened.
    asked: Encryption,
    field: PasswordBox,
    confirm: Option<PasswordBox>,
    problem: TextBlock,
    busy: UIElement,
    accept: Control,
    /// Set while the passphrase is checked or stored.
    working: Cell<bool>,
    page: Weak<Page>,
}

impl PassphraseFlyout {
    fn open(
        page: &Rc<Page>,
        browser: &Browser,
        asked: Encryption,
        dialog: &PassphraseDialog,
        anchor: &FrameworkElement,
    ) -> Result<Rc<Self>> {
        let flyout: Flyout = xaml::load(PASSPHRASE)?;
        let content: FrameworkElement = xaml::load(&passphrase_markup(dialog))?;
        content.SetRequestedTheme(super::element_theme(browser.theme()))?;
        flyout.SetContent(&content)?;
        let this = Rc::new(PassphraseFlyout {
            flyout: flyout.cast()?,
            opened: Cell::new(false),
            asked,
            field: xaml::find(&content, "SyncPassphrase")?,
            confirm: dialog
                .confirm
                .map(|_| xaml::find(&content, "SyncPassphraseConfirm"))
                .transpose()?,
            problem: xaml::find(&content, "SyncPassphraseProblem")?,
            busy: xaml::find(&content, "SyncPassphraseBusy")?,
            accept: xaml::find(&content, "SyncPassphraseAccept")?,
            working: Cell::new(false),
            page: Rc::downgrade(page),
        });
        for field in [Some(&this.field), this.confirm.as_ref()]
            .into_iter()
            .flatten()
        {
            let me = Rc::downgrade(&this);
            field
                .PasswordChanged(move |_, _| {
                    if let Some(me) = me.upgrade() {
                        me.validate();
                    }
                })?
                .forget();
        }
        let me = Rc::downgrade(&this);
        on_click(&this.accept, move || {
            if let Some(me) = me.upgrade() {
                me.accept();
            }
        })?;
        let me = Rc::downgrade(&this);
        on_click(
            &xaml::find::<Button>(&content, "SyncPassphraseCancel")?,
            move || {
                if let Some(me) = me.upgrade() {
                    me.close();
                }
            },
        )?;
        let me = Rc::downgrade(&this);
        content
            .cast::<UIElement>()?
            .KeyDown(move |_, args| {
                let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) else {
                    return;
                };
                if args.Key().is_ok_and(|k| k.0 == VK_RETURN) {
                    let _ = args.SetHandled(true);
                    me.accept();
                }
            })?
            .forget();
        let me = Rc::downgrade(&this);
        this.flyout
            .Opened(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.opened.set(true);
                }
            })?
            .forget();
        let p = Rc::downgrade(page);
        this.flyout
            .Closed(move |_, _| {
                if let Some(p) = p.upgrade() {
                    p.passphrase.borrow_mut().take();
                }
            })?
            .forget();

        this.flyout.ShowAt(anchor)?;
        let first = this.field.clone();
        exec::spawn(async move {
            let _ = first
                .cast::<UIElement>()
                .and_then(|b| b.Focus(FocusState::Programmatic));
        });
        Ok(this)
    }

    /// What was typed, as `check_passphrase` takes it.
    fn typed(&self) -> (String, Option<String>) {
        let text = |field: &PasswordBox| field.Password().unwrap_or_default();
        (text(&self.field), self.confirm.as_ref().map(text))
    }

    /// Says why what was typed is no passphrase, once anything was, and enables accepting only
    /// when it is one.
    fn validate(&self) {
        let (text, confirm) = self.typed();
        let checked = check_passphrase(&text, confirm.as_deref());
        let typed = !text.is_empty() || confirm.as_deref().is_some_and(|c| !c.is_empty());
        self.show_problem(checked.as_ref().err().copied().filter(|_| typed));
        let _ = self
            .accept
            .SetIsEnabled(checked.is_ok() && !self.working.get());
    }

    fn show_problem(&self, problem: Option<&str>) {
        let _ = self.problem.SetText(problem.unwrap_or_default());
        let _ = xaml::set_visible(&self.problem, problem.is_some());
    }

    /// Takes the passphrase, busy meanwhile, and closes once it is taken; else says why not,
    /// under the fields or, once the flyout has closed, under the page's buttons.
    pub(crate) fn accept(self: &Rc<Self>) {
        let (text, confirm) = self.typed();
        let (Some(page), Ok(passphrase)) = (
            self.page.upgrade(),
            check_passphrase(&text, confirm.as_deref()),
        ) else {
            return;
        };
        let Some(browser) = page.browser.upgrade() else {
            return;
        };
        if self.working.replace(true) {
            return;
        }
        let _ = self.accept.SetIsEnabled(false);
        let _ = xaml::set_visible(&self.busy, true);
        let (me, page) = (Rc::downgrade(self), self.page.clone());
        sync::set_passphrase(&browser, self.asked, passphrase, move |error| {
            let open = me
                .upgrade()
                .filter(|me| me.flyout.IsOpen().unwrap_or(false));
            match (open, error) {
                (Some(me), None) => me.close(),
                (Some(me), Some(error)) => {
                    me.working.set(false);
                    let _ = xaml::set_visible(&me.busy, false);
                    me.validate();
                    me.show_problem(Some(&error));
                }
                (None, Some(error)) => {
                    if let Some(page) = page.upgrade() {
                        page.show_action_error(Some(&error));
                    }
                }
                (None, None) => {}
            }
        });
    }

    pub(crate) fn close(&self) {
        let _ = self.flyout.Hide();
    }

    /// Types `text` into the field, and `confirm` into the second one if there is one, for the
    /// scripted runs.
    pub(crate) fn fill(&self, text: &str, confirm: &str) -> Result<()> {
        self.field.SetPassword(text)?;
        match &self.confirm {
            Some(field) => field.SetPassword(confirm),
            None => Ok(()),
        }
    }

    /// How many fields it has, the problem it shows, if any, and whether accepting is enabled.
    pub(crate) fn shown(&self) -> (usize, Option<String>, bool) {
        let problem = xaml::is_visible(&self.problem)
            .then(|| self.problem.Text().ok())
            .flatten()
            .map(|t| t.to_string());
        let fields = 1 + usize::from(self.confirm.is_some());
        (fields, problem, self.accept.IsEnabled().unwrap_or(false))
    }
}

/// What the switches show: whether "Sync everything" is on, then each type's switch (in
/// `DataType::ALL`'s order) on and enabled.
fn type_switches(stored: &[DataType], customizing: bool) -> (bool, Vec<(bool, bool)>) {
    let everything = !customizing && DataType::all_in(stored);
    let switches = DataType::ALL
        .iter()
        .map(|t| (everything || stored.contains(t), !everything))
        .collect();
    (everything, switches)
}

/// The sync server's address box, stored when it loses focus, editable only while signed out.
pub(super) struct ServerBox {
    browser: Weak<Browser>,
    text: TextBox,
    error: TextBlock,
}

impl ServerBox {
    pub fn wire(scope: &FrameworkElement, browser: &Rc<Browser>, prefix: &str) -> Result<Rc<Self>> {
        let this = Rc::new(Self {
            browser: Rc::downgrade(browser),
            text: xaml::find(scope, &format!("{prefix}Server"))?,
            error: xaml::find(scope, &format!("{prefix}ServerError"))?,
        });
        this.text
            .SetText(&browser.core(|p| p.prefs().get(&keys::SYNC_SERVER)))?;
        let me = Rc::downgrade(&this);
        this.text
            .cast::<UIElement>()?
            .LostFocus(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.apply();
                }
            })?
            .forget();
        Ok(this)
    }

    pub fn set_editable(&self, editable: bool) {
        let _ = self
            .text
            .cast::<Control>()
            .and_then(|c| c.SetIsEnabled(editable));
    }

    /// Stores the typed server address, normalized, or shows why it is not one. False when it is
    /// not.
    pub fn apply(&self) -> bool {
        let Some(browser) = self.browser.upgrade() else {
            return false;
        };
        if !browser.sync().status().server_editable {
            return true;
        }
        let text = self.text.Text().unwrap_or_default();
        let stored = browser.core(|p| p.prefs().get(&keys::SYNC_SERVER));
        match server_input(&text) {
            Ok(value) => {
                let shown = value.as_deref().unwrap_or(DEFAULT_SYNC_SERVER);
                match &value {
                    None => {
                        if let Err(e) = browser.core(|p| p.prefs().reset(&keys::SYNC_SERVER)) {
                            log::warn!("sync server: {e}");
                        }
                    }
                    Some(url) if *url != stored => browser.write_pref(&keys::SYNC_SERVER, url),
                    Some(_) => {}
                }
                if text != shown {
                    let _ = self.text.SetText(shown);
                }
                let _ = xaml::set_visible(&self.error, false);
                true
            }
            Err(error) => {
                let _ = self.error.SetText(&error.to_string());
                let _ = xaml::set_visible(&self.error, true);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_everything_shows_every_type_on_and_fixed() {
        let (everything, switches) = type_switches(&DataType::ALL, false);
        assert!(everything);
        assert!(switches.iter().all(|&s| s == (true, false)));
        let (everything, switches) = type_switches(&DataType::ALL, true);
        assert!(!everything, "turned off to customize");
        assert!(switches.iter().all(|&s| s == (true, true)));
        let (everything, switches) = type_switches(&[DataType::History], false);
        assert!(!everything);
        assert_eq!(switches[0], (false, true));
        assert_eq!(switches[1], (true, true));
    }

    #[test]
    fn the_flyout_words_come_from_core_and_a_new_passphrase_is_typed_twice() {
        let fields = |encryption| {
            let markup = passphrase_markup(&passphrase_dialog(encryption).unwrap());
            (markup.matches("<PasswordBox").count(), markup)
        };
        let (count, set) = fields(Encryption::Set);
        assert_eq!(count, 2);
        assert!(set.contains(r#"Header="Confirm passphrase""#), "{set}");
        assert!(set.contains("You'll enter it on each device"), "{set}");
        assert_eq!(fields(Encryption::Enter).0, 1);
        assert_eq!(fields(Encryption::Changed).0, 1);
        assert_eq!(fields(Encryption::Ready).0, 2);
    }
}
