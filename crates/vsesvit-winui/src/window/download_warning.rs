//! Chrome's warning about a downloaded file that can run code: a bubble under the downloads
//! button with Keep and Discard, as the Downloads view offers them too.

use vsesvit_core::downloads::{Download, status_line};
use windows_core::{Interface, Result};

use super::{BrowserWindow, hide, open_content, with};
use crate::bindings::*;
use crate::downloads::file_name;
use crate::{dialogs, xaml};

const FLYOUT: &str = r#"<Flyout {ns} Placement="BottomEdgeAlignedRight"/>"#;

impl BrowserWindow {
    /// Shows the warning about `download`, an unconfirmed file, under the downloads button.
    pub fn warn_about_download(&self, download: &Download) {
        if let Err(e) = self.show_download_warning(download) {
            log::warn!("download warning: {e}");
        }
    }

    fn show_download_warning(&self, download: &Download) -> Result<()> {
        // Loaded as its own root, so that its names resolve before it is shown.
        let content: FrameworkElement = xaml::load(&format!(
            r#"<StackPanel {{ns}} Width="300" Spacing="16">
                 <StackPanel Spacing="4">
                   <TextBlock Text="{name}" FontWeight="SemiBold" TextTrimming="CharacterEllipsis"/>
                   <TextBlock Text="{warning}" TextWrapping="Wrap"/>
                 </StackPanel>
                 <StackPanel Orientation="Horizontal" HorizontalAlignment="Right" Spacing="8">
                   <Button x:Name="WarningKeep" Content="Keep" MinWidth="96"/>
                   <Button x:Name="WarningDiscard" Content="Discard" MinWidth="96"
                           Style="{{StaticResource AccentButtonStyle}}"/>
                 </StackPanel>
               </StackPanel>"#,
            name = xaml::escape(&file_name(&download.path)),
            warning = xaml::escape(&status_line(download, None, false)),
        ))?;
        let flyout: Flyout = xaml::load(FLYOUT)?;
        flyout.SetContent(&content)?;
        for (name, keep) in [("WarningKeep", true), ("WarningDiscard", false)] {
            let window = self.me.clone();
            let download = download.clone();
            dialogs::on_click(&xaml::find::<Button>(&content, name)?, move || {
                with(&window, |w| {
                    w.hide_download_warning();
                    match w.browser() {
                        Some(b) if keep => b.keep_download(&download),
                        Some(b) => b.discard_download(&download),
                        None => {}
                    }
                });
            })?;
        }
        self.hide_download_warning();
        let options = FlyoutShowOptions::new()?;
        options.SetShowMode(if self.is_foreground() {
            FlyoutShowMode::Standard
        } else {
            FlyoutShowMode::Transient
        })?;
        flyout
            .cast::<FlyoutBase>()?
            .ShowAtWithOptions(&self.ui.downloads.cast::<FrameworkElement>()?, &options)?;
        *self.download_warning.borrow_mut() = Some(flyout);
        Ok(())
    }

    fn hide_download_warning(&self) {
        if let Some(flyout) = self.download_warning.borrow().as_ref() {
            hide(flyout);
        }
    }

    /// The warning about a downloaded file while it is open.
    pub fn download_warning(&self) -> Option<FrameworkElement> {
        open_content(self.download_warning.borrow().as_ref()?)
    }
}
