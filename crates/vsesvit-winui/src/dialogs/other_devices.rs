//! History's "Tabs from other devices": each device that synced its open tabs lately, newest
//! first, with how long ago, and its tabs, each opening in a new tab here.

use std::rc::{Rc, Weak};

use vsesvit_core::Url;
use vsesvit_core::session::DeviceTabs;
use windows_core::{Interface, Result};

use super::history::{ago, now_ms};
use super::on_click;
use crate::bindings::*;
use crate::browser::Browser;
use crate::window::BrowserWindow;
use crate::xaml;

pub(super) const PANEL: &str = r#"
    <ScrollViewer x:Name="OtherDevicesPanel" Grid.Column="1" Padding="0,0,16,0" VerticalScrollBarVisibility="Auto"
                  Visibility="Collapsed">
      <StackPanel Spacing="16" Padding="0,0,0,12">
        <TextBlock x:Name="OtherDevicesNote" TextWrapping="Wrap" Visibility="Collapsed"
                   Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
        <StackPanel x:Name="OtherDevicesList" Spacing="24"/>
      </StackPanel>
    </ScrollViewer>"#;

pub(super) struct OtherDevices {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    note: TextBlock,
    list: Panel,
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Rc<OtherDevices>> {
    let this = Rc::new(OtherDevices {
        browser: Rc::downgrade(browser),
        window: Rc::downgrade(window),
        note: xaml::find(root, "OtherDevicesNote")?,
        list: xaml::find(root, "OtherDevicesList")?,
    });
    this.render();
    Ok(this)
}

/// What shows instead of devices: why there are none.
fn note(signed_in: bool, devices: usize) -> Option<&'static str> {
    if !signed_in {
        Some("Sign in to sync to see tabs from your other devices.")
    } else if devices == 0 {
        Some("Tabs from your other devices appear here when they sync.")
    } else {
        None
    }
}

impl OtherDevices {
    pub fn render(&self) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let signed_in = browser.sync().signed_in();
        let devices = if signed_in {
            browser
                .core(|p| p.session().other_devices())
                .unwrap_or_else(|e| {
                    log::warn!("tabs from other devices: {e}");
                    Vec::new()
                })
        } else {
            Vec::new()
        };
        let text = note(signed_in, devices.len());
        let _ = self.note.SetText(text.unwrap_or_default());
        let _ = xaml::set_visible(&self.note, text.is_some());
        if let Err(e) = self.fill(&browser, &devices) {
            log::warn!("tabs from other devices: {e}");
        }
    }

    fn fill(&self, browser: &Browser, devices: &[DeviceTabs]) -> Result<()> {
        let children = self.list.Children()?;
        children.Clear()?;
        let now = now_ms();
        for (d, device) in devices.iter().enumerate() {
            let group: FrameworkElement = xaml::load(&format!(
                r#"<StackPanel {{ns}} x:Name="Device{d}" Spacing="2">
  <TextBlock Text="{name}" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
  <TextBlock Text="Synced {when}" Margin="0,0,0,6" Style="{{StaticResource CaptionTextBlockStyle}}"
             Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
</StackPanel>"#,
                name = xaml::escape(&device.device_name),
                when = xaml::escape(&ago(now, device.updated_ms)),
            ))?;
            let tabs = group.cast::<Panel>()?.Children()?;
            let listed = device.windows.iter().flat_map(|w| &w.tabs);
            for (t, tab) in listed.enumerate() {
                let row = self.tab_row(browser, d, t, &tab.url, &tab.title)?;
                tabs.Append(&row.cast::<UIElement>()?)?;
            }
            children.Append(&group.cast::<UIElement>()?)?;
        }
        Ok(())
    }

    /// A tab's row: its favicon when one is kept here, its title, and its address as the tooltip.
    fn tab_row(
        &self,
        browser: &Browser,
        d: usize,
        t: usize,
        url: &Url,
        title: &str,
    ) -> Result<FrameworkElement> {
        let title = if title.trim().is_empty() {
            url.as_str()
        } else {
            title
        };
        let row: FrameworkElement = xaml::load(&format!(
            r#"<Button {{ns}} x:Name="DeviceTab{d}_{t}" HorizontalAlignment="Stretch" HorizontalContentAlignment="Left"
        Background="Transparent" BorderThickness="0" Padding="8,6"
        ToolTipService.ToolTip="{url}" AutomationProperties.Name="{title}">
  <Grid ColumnSpacing="10">
    <Grid.ColumnDefinitions><ColumnDefinition Width="16"/><ColumnDefinition Width="*"/></Grid.ColumnDefinitions>
    <FontIcon x:Name="Glyph" Glyph="&#xE774;" FontSize="14"/>
    <Image x:Name="Favicon" Width="16" Height="16" Visibility="Collapsed"/>
    <TextBlock Grid.Column="1" Text="{title}" TextTrimming="CharacterEllipsis"/>
  </Grid>
</Button>"#,
            url = xaml::escape(url.as_str()),
            title = xaml::escape(title),
        ))?;
        if let Some(png) = browser.core(|p| p.favicons().get(url).ok().flatten()) {
            xaml::show_favicon(&row, png)?;
        }
        let (window, url) = (self.window.clone(), url.to_string());
        on_click(&row, move || {
            if let Some(window) = window.upgrade()
                && let Err(e) = window.open_url_tab(&url, true)
            {
                log::warn!("open {url}: {e}");
            }
        })?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_devices_says_why() {
        assert_eq!(
            note(false, 3),
            Some("Sign in to sync to see tabs from your other devices.")
        );
        assert_eq!(
            note(true, 0),
            Some("Tabs from your other devices appear here when they sync.")
        );
        assert_eq!(note(true, 1), None);
    }
}
