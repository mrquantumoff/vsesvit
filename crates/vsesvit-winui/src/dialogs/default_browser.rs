//! Whether Vsesvit is the default browser, and the button that takes the user to where Windows
//! lets them choose it. Shared by the welcome and Settings; the status follows the choice made
//! in Windows Settings while the dialog is open.

use std::rc::Rc;
use std::time::Duration;

use windows_core::Result;

use super::on_click;
use crate::bindings::*;
use crate::platform::{self, DefaultBrowser};
use crate::{exec, xaml};

/// The status, the button and a line under them.
pub(super) const MARKUP: &str = r#"
  <StackPanel Spacing="8">
    <TextBlock x:Name="DefaultBrowserStatus" TextWrapping="Wrap"/>
    <Button x:Name="DefaultBrowserMake" Content="Make Vsesvit the default browser"
            Style="{StaticResource AccentButtonStyle}"/>
    <TextBlock x:Name="DefaultBrowserHint" TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
  </StackPanel>"#;

const POLL: Duration = Duration::from_secs(1);

pub(super) struct Row {
    status: TextBlock,
    make: Button,
    hint: TextBlock,
}

impl Row {
    fn show(&self, state: DefaultBrowser) {
        let (status, hint) = describe(state);
        let _ = self.status.SetText(status);
        let _ = self.hint.SetText(hint);
        let _ = xaml::set_visible(&self.make, state == DefaultBrowser::Other);
    }
}

/// The status line and the line under the button.
pub(crate) fn describe(state: DefaultBrowser) -> (&'static str, &'static str) {
    match state {
        DefaultBrowser::Vsesvit => ("Vsesvit is your default browser.", ""),
        DefaultBrowser::Other => (
            "Another browser opens web links on this PC.",
            "Windows Settings opens on Vsesvit's default apps: choose Set default there.",
        ),
        DefaultBrowser::Unregistered => (
            "This copy of Vsesvit is not registered with Windows as a browser, so Windows cannot \
             make it the default.",
            "Vsesvit installed with its installer registers itself.",
        ),
    }
}

/// Wires the row of `MARKUP` inside `root`; it follows Windows while the caller keeps it.
pub(super) fn wire(root: &FrameworkElement) -> Result<Rc<Row>> {
    let row = Rc::new(Row {
        status: xaml::find(root, "DefaultBrowserStatus")?,
        make: xaml::find(root, "DefaultBrowserMake")?,
        hint: xaml::find(root, "DefaultBrowserHint")?,
    });
    let mut shown = platform::default_browser();
    row.show(shown);
    on_click(&row.make, platform::open_default_apps_settings)?;
    let weak = Rc::downgrade(&row);
    exec::spawn(async move {
        loop {
            exec::sleep(POLL).await;
            let Some(row) = weak.upgrade() else { break };
            let now = platform::default_browser();
            if now != shown {
                shown = now;
                row.show(now);
            }
        }
    });
    Ok(row)
}
