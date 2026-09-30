//! Settings' Sync page: the account's status with its buttons (from `vsesvit_sync::status`),
//! following the sync state while the dialog is open, what this device syncs, and the server to
//! sign in to. The server box also serves the welcome's sync page.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use vsesvit_core::prefs::{DEFAULT_SYNC_SERVER, keys};
use vsesvit_core::sync::DataType;
use vsesvit_sync::normalize_base_url;
use vsesvit_sync::status::{Action, DELETE_CONFIRMATION, Status};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
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

pub(super) struct Page {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    /// The Settings dialog, hidden once the provider's page opens so the page shows.
    dialog: FrameworkElement,
    title: TextBlock,
    subtitle: TextBlock,
    actions: Panel,
    action_error: TextBlock,
    everything: ToggleSwitch,
    types: Vec<(DataType, ToggleSwitch)>,
    /// "Sync everything" was turned off here, so the types can be chosen one by one even while
    /// all of them are on.
    customizing: Cell<bool>,
    pub server: Rc<ServerBox>,
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
            dialog: root.clone(),
            title,
            subtitle,
            actions,
            action_error,
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
        sync::set_types(&browser, &with_type(&stored, data_type, on));
    }

    /// One button per action, the first one accented. Deleting the server's data asks first, in
    /// a flyout: no second dialog can open over this one.
    fn fill_actions(self: &Rc<Self>, status: &Status) -> Result<()> {
        let children = self.actions.Children()?;
        children.Clear()?;
        for (index, &action) in status.actions.iter().enumerate() {
            let button: Button = if action == Action::DeleteServerData {
                self.delete_button()?
            } else {
                let style = if index == 0 {
                    r#" Style="{StaticResource AccentButtonStyle}""#
                } else {
                    ""
                };
                let button = xaml::load(&format!(
                    r#"<Button {{ns}} x:Name="Sync{action:?}" Content="{}"{style}/>"#,
                    xaml::escape(action.label())
                ))?;
                let p = Rc::downgrade(self);
                on_click(&button, move || {
                    let p = p.clone();
                    // Not from inside the click of a button the new state removes.
                    exec::spawn(async move {
                        if let Some(p) = p.upgrade() {
                            p.act(action);
                        }
                    });
                })?;
                button
            };
            button
                .cast::<Control>()?
                .SetIsEnabled(!(action == Action::SyncNow && status.busy))?;
            children.Append(&button.cast::<UIElement>()?)?;
        }
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

    fn act(&self, action: Action) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        self.show_action_error(None);
        match action {
            Action::SignIn => {
                if !self.server.apply() {
                    return;
                }
                let dialog = self.dialog.clone();
                sync::sign_in(&browser, self.window.clone(), move || {
                    if let Err(e) = dialog.cast::<IContentDialog>().and_then(|d| d.Hide()) {
                        log::warn!("closing Settings for the sign-in page: {e}");
                    }
                });
            }
            Action::Cancel => sync::cancel_sign_in(&browser),
            Action::SyncNow => sync::sync_now(&browser),
            Action::SignOut => sync::sign_out(&browser),
            Action::DeleteServerData => {}
        }
    }
}

/// What the switches show: whether "Sync everything" is on, then each type's switch (in
/// `DataType::ALL`'s order) on and enabled.
fn type_switches(stored: &[DataType], customizing: bool) -> (bool, Vec<(bool, bool)>) {
    let everything = !customizing && DataType::ALL.iter().all(|t| stored.contains(t));
    let switches = DataType::ALL
        .iter()
        .map(|t| (everything || stored.contains(t), !everything))
        .collect();
    (everything, switches)
}

/// `stored` with `data_type` turned on or off, in `DataType::ALL`'s order.
fn with_type(stored: &[DataType], data_type: DataType, on: bool) -> Vec<DataType> {
    DataType::ALL
        .into_iter()
        .filter(|t| {
            if *t == data_type {
                on
            } else {
                stored.contains(t)
            }
        })
        .collect()
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
        match server_value(&text) {
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
                let _ = self.error.SetText(&error);
                let _ = xaml::set_visible(&self.error, true);
                false
            }
        }
    }
}

/// What the server box holds: nothing (the default server) or a server address, normalized.
fn server_value(text: &str) -> std::result::Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    normalize_base_url(text)
        .map(Some)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_box_is_empty_or_an_address() {
        assert_eq!(server_value("  "), Ok(None));
        assert_eq!(
            server_value(" https://sync.example.com/ "),
            Ok(Some("https://sync.example.com".into()))
        );
        assert_eq!(
            server_value("http://127.0.0.1:8080/"),
            Ok(Some("http://127.0.0.1:8080".into()))
        );
        assert_eq!(
            server_value("http://sync.example.com"),
            Err("http://sync.example.com is not a valid server address".into())
        );
    }

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
    fn a_type_turns_on_and_off_in_a_fixed_order() {
        let off = with_type(&DataType::ALL, DataType::Bookmarks, false);
        assert!(!off.contains(&DataType::Bookmarks));
        assert_eq!(off.len(), DataType::ALL.len() - 1);
        let on = with_type(&[DataType::Settings], DataType::Bookmarks, true);
        assert_eq!(on, [DataType::Bookmarks, DataType::Settings]);
    }
}
