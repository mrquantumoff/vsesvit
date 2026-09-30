//! Settings' Sync page: the account's status with its buttons (from `vsesvit_sync::status`),
//! following the sync state while the dialog is open, and the server to sign in to.

use std::rc::{Rc, Weak};

use vsesvit_core::prefs::{DEFAULT_SYNC_SERVER, keys};
use vsesvit_sync::normalize_base_url;
use vsesvit_sync::status::{Action, Status};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
use crate::browser::Browser;
use crate::window::BrowserWindow;
use crate::{exec, sync, xaml};

pub(super) const PANEL: &str = r#"
    <ScrollViewer x:Name="SyncPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto">
      <StackPanel Spacing="28" Padding="0,0,0,12">
        <StackPanel Spacing="4">
          <TextBlock x:Name="SyncTitle" TextWrapping="Wrap" Style="{StaticResource BodyStrongTextBlockStyle}"/>
          <TextBlock x:Name="SyncSubtitle" TextWrapping="Wrap" IsTextSelectionEnabled="True"
                     Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
          <StackPanel x:Name="SyncActions" Orientation="Horizontal" Spacing="8" Margin="0,8,0,0"/>
        </StackPanel>
        <StackPanel Spacing="4">
          <TextBox x:Name="SyncServer" Header="Sync server" PlaceholderText="https://"/>
          <TextBlock x:Name="SyncServerError" TextWrapping="Wrap" Visibility="Collapsed"
                     Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource SystemFillColorCriticalBrush}"/>
        </StackPanel>
      </StackPanel>
    </ScrollViewer>"#;

pub(super) struct Page {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    /// The Settings dialog, hidden once the provider's page opens so the page shows.
    dialog: FrameworkElement,
    title: TextBlock,
    subtitle: TextBlock,
    actions: Panel,
    server: TextBox,
    server_error: TextBlock,
    /// Registered with the sync state; the page follows it while this is kept.
    follow: Rc<dyn Fn()>,
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Rc<Page>> {
    let (title, subtitle, actions) = (
        xaml::find(root, "SyncTitle")?,
        xaml::find(root, "SyncSubtitle")?,
        xaml::find(root, "SyncActions")?,
    );
    let (server, server_error) = (
        xaml::find::<TextBox>(root, "SyncServer")?,
        xaml::find(root, "SyncServerError")?,
    );
    server.SetText(&browser.core(|p| p.prefs().get(&keys::SYNC_SERVER)))?;
    let page = Rc::new_cyclic(|me: &Weak<Page>| {
        let me = me.clone();
        Page {
            browser: Rc::downgrade(browser),
            window: Rc::downgrade(window),
            dialog: root.clone(),
            title,
            subtitle,
            actions,
            server,
            server_error,
            follow: Rc::new(move || {
                if let Some(page) = me.upgrade() {
                    page.show();
                }
            }),
        }
    });
    let p = Rc::downgrade(&page);
    page.server
        .cast::<UIElement>()?
        .LostFocus(move |_, _| {
            if let Some(p) = p.upgrade() {
                p.apply_server();
            }
        })?
        .forget();
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
        let _ = self
            .server
            .cast::<Control>()
            .and_then(|c| c.SetIsEnabled(status.server_editable));
        if let Err(e) = self.fill_actions(&status) {
            log::warn!("sync buttons: {e}");
        }
    }

    /// One button per action, the first one accented.
    fn fill_actions(self: &Rc<Self>, status: &Status) -> Result<()> {
        let children = self.actions.Children()?;
        children.Clear()?;
        for (index, &action) in status.actions.iter().enumerate() {
            let style = if index == 0 {
                r#" Style="{StaticResource AccentButtonStyle}""#
            } else {
                ""
            };
            let button: Button = xaml::load(&format!(
                r#"<Button {{ns}} x:Name="Sync{action:?}" Content="{}"{style}/>"#,
                xaml::escape(action.label())
            ))?;
            button
                .cast::<Control>()?
                .SetIsEnabled(!(action == Action::SyncNow && status.busy))?;
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
            children.Append(&button.cast::<UIElement>()?)?;
        }
        Ok(())
    }

    fn act(&self, action: Action) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        match action {
            Action::SignIn => {
                if !self.apply_server() {
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
        }
    }

    /// Stores the typed server address, normalized, or shows why it is not one. False when it is
    /// not.
    pub fn apply_server(&self) -> bool {
        let Some(browser) = self.browser.upgrade() else {
            return false;
        };
        if !browser.sync().status().server_editable {
            return true;
        }
        let text = self.server.Text().unwrap_or_default();
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
                    let _ = self.server.SetText(shown);
                }
                let _ = xaml::set_visible(&self.server_error, false);
                true
            }
            Err(error) => {
                let _ = self.server_error.SetText(&error);
                let _ = xaml::set_visible(&self.server_error, true);
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
}
