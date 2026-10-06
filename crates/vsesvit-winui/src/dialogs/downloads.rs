//! Downloads: the list newest first, each entry's actions (pausing and resuming, keeping or
//! discarding a file that can run code), the download folder and clearing the list. Updates
//! live while open.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use vsesvit_core::downloads::{Download, State, status_line};
use windows_core::{Interface, Result};

use super::{Wired, on_click};
use crate::bindings::*;
use crate::browser::Browser;
use crate::downloads::{Change, Subscriber, file_name};
use crate::{exec, platform, xaml};

pub(super) const MARKUP: &str = r#"
  <Grid RowSpacing="12">
    <Grid.RowDefinitions>
      <RowDefinition Height="Auto"/><RowDefinition Height="*"/>
    </Grid.RowDefinitions>
    <StackPanel Orientation="Horizontal" Spacing="8" HorizontalAlignment="Right">
      <Button x:Name="DownloadsOpenFolder" Content="Open download folder"/>
      <Button x:Name="DownloadsClear" Content="Clear list"/>
    </StackPanel>
    <Grid Grid.Row="1">
      <ScrollViewer>
        <StackPanel x:Name="DownloadRows" Spacing="4"/>
      </ScrollViewer>
      <TextBlock x:Name="DownloadsEmpty" Text="Files you download appear here" HorizontalAlignment="Center"
                 VerticalAlignment="Center" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    </Grid>
  </Grid>"#;

/// What a row offers, in the order its buttons appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Open,
    ShowInFolder,
    Pause,
    Resume,
    Cancel,
    Keep,
    Discard,
    Remove,
}

const ACTIONS: [(Action, &str, &str); 8] = [
    (Action::Open, "Open", "Open"),
    (Action::ShowInFolder, "ShowInFolder", "Show in folder"),
    (Action::Pause, "Pause", "Pause"),
    (Action::Resume, "Resume", "Resume"),
    (Action::Cancel, "Cancel", "Cancel"),
    (Action::Keep, "Keep", "Keep"),
    (Action::Discard, "Discard", "Discard"),
    (Action::Remove, "Remove", "Remove from list"),
];

impl Action {
    /// Whether an entry in `state` whose file does or does not `exist` offers this.
    fn applies(self, state: State, exists: bool) -> bool {
        match self {
            Action::Open => state == State::Completed && exists,
            Action::ShowInFolder => exists && state != State::Unconfirmed,
            Action::Pause => state == State::InProgress,
            Action::Resume => matches!(state, State::Paused | State::Interrupted),
            Action::Cancel => state.is_live(),
            Action::Keep | Action::Discard => state == State::Unconfirmed,
            Action::Remove => state.is_final(),
        }
    }
}

/// A row whose status line and bar follow a download the engine holds.
struct Row {
    download: Download,
    status: TextBlock,
    progress: ProgressBar,
}

struct Page {
    browser: Weak<Browser>,
    rows: Panel,
    empty: UIElement,
    running: RefCell<Vec<Row>>,
}

pub(super) fn wire(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<Wired> {
    let page = Rc::new(Page {
        browser: Rc::downgrade(browser),
        rows: xaml::find(root, "DownloadRows")?,
        empty: xaml::find(root, "DownloadsEmpty")?,
        running: RefCell::new(Vec::new()),
    });
    let p = Rc::downgrade(&page);
    let subscriber: Subscriber = Rc::new(move |change| {
        let Some(p) = p.upgrade() else { return };
        match change {
            Change::List => p.render(),
            Change::Progress => p.refresh_running(),
        }
    });
    browser.subscribe_downloads(&subscriber);
    page.render();

    let b = Rc::downgrade(browser);
    on_click(
        &xaml::find::<Button>(root, "DownloadsOpenFolder")?,
        move || {
            if let Some(b) = b.upgrade() {
                let dir = b.download_dir();
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    log::warn!("{}: {e}", dir.display());
                }
                platform::open_in_shell(dir.as_os_str());
            }
        },
    )?;
    let b = Rc::downgrade(browser);
    on_click(&xaml::find::<Button>(root, "DownloadsClear")?, move || {
        if let Some(b) = b.upgrade() {
            b.clear_downloads();
        }
    })?;
    Ok(Wired {
        _alive: vec![page, Rc::new(subscriber)],
        on_close: None,
    })
}

impl Page {
    fn render(self: &Rc<Self>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let downloads = browser.download_list();
        let Ok(children) = self.rows.Children() else {
            return;
        };
        let _ = children.Clear();
        let mut running = Vec::new();
        for download in downloads {
            let built = self.row(&download).and_then(|(element, row)| {
                children.Append(&element)?;
                Ok(row)
            });
            match built {
                Ok(row) if download.state.is_live() => running.push(row),
                Ok(_) => {}
                Err(e) => log::warn!("download row: {e}"),
            }
        }
        let _ = xaml::set_visible(&self.empty, children.Size().unwrap_or(0) == 0);
        *self.running.borrow_mut() = running;
        self.refresh_running();
    }

    fn refresh_running(&self) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        for row in self.running.borrow().iter() {
            let live = browser.download_progress(row.download.id);
            let _ = row.status.SetText(&status_line(&row.download, live, true));
            let (received, total) = live.unwrap_or((row.download.received, row.download.total));
            let _ = super::set_progress(&row.progress, fraction(received, total));
        }
    }

    fn row(self: &Rc<Self>, download: &Download) -> Result<(UIElement, Row)> {
        let exists = download.path.exists();
        let element: FrameworkElement = xaml::load(&format!(
            r#"<Grid {{ns}} Padding="12,8" ColumnSpacing="12" CornerRadius="4"
                     Background="{{ThemeResource CardBackgroundFillColorDefaultBrush}}">
                 <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
                 <StackPanel Spacing="2" VerticalAlignment="Center">
                   <TextBlock Text="{name}" FontWeight="SemiBold" TextTrimming="CharacterEllipsis"/>
                   <TextBlock x:Name="Status" Text="{status}" Style="{{StaticResource CaptionTextBlockStyle}}"
                              Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
                   <ProgressBar x:Name="Progress" Margin="0,4,0,0" Visibility="Collapsed"
                                ShowPaused="{paused}" ShowError="{interrupted}"/>
                 </StackPanel>
                 <StackPanel x:Name="Actions" Grid.Column="1" Orientation="Horizontal" Spacing="4"
                             VerticalAlignment="Center"/>
               </Grid>"#,
            name = xaml::escape(&file_name(&download.path)),
            status = xaml::escape(&status_line(download, None, exists)),
            paused = download.state == State::Paused,
            interrupted = download.state == State::Interrupted,
        ))?;
        let actions: Panel = xaml::find(&element, "Actions")?;
        let buttons = actions.Children()?;
        for (action, id, label) in ACTIONS {
            if !action.applies(download.state, exists) {
                continue;
            }
            let button: Button = xaml::load(&format!(
                r#"<Button {{ns}} x:Name="{id}" Content="{label}"/>"#
            ))?;
            let p = Rc::downgrade(self);
            let entry = download.clone();
            on_click(&button, move || {
                // The row may be rebuilt by the action; the click finishes first.
                let (p, entry) = (p.clone(), entry.clone());
                exec::spawn(async move {
                    if let Some(p) = p.upgrade() {
                        p.act(action, &entry);
                    }
                });
            })?;
            buttons.Append(&button.cast::<UIElement>()?)?;
        }
        let row = Row {
            download: download.clone(),
            status: xaml::find(&element, "Status")?,
            progress: xaml::find(&element, "Progress")?,
        };
        xaml::set_visible(&row.progress, download.state.is_live())?;
        Ok((element.cast()?, row))
    }

    fn act(&self, action: Action, download: &Download) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        match action {
            Action::Open => platform::open_in_shell(download.path.as_os_str()),
            Action::ShowInFolder => platform::show_in_folder(&download.path),
            Action::Pause => browser.pause_download(download.id),
            Action::Resume => browser.resume_download(download.id),
            Action::Cancel => browser.cancel_download(download.id),
            Action::Keep => browser.keep_download(download.id),
            Action::Discard => browser.discard_download(download.id),
            Action::Remove => browser.remove_download(download.id),
        }
    }
}

/// How much of a download has arrived, while its size is known.
fn fraction(received: u64, total: Option<u64>) -> Option<f64> {
    total.filter(|t| *t > 0).map(|t| received as f64 / t as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(state: State, exists: bool) -> Vec<Action> {
        ACTIONS
            .iter()
            .map(|(action, _, _)| *action)
            .filter(|a| a.applies(state, exists))
            .collect()
    }

    #[test]
    fn rows_offer_what_the_entry_allows() {
        use Action::*;
        assert_eq!(offered(State::InProgress, false), [Pause, Cancel]);
        assert_eq!(offered(State::Paused, false), [Resume, Cancel]);
        assert_eq!(offered(State::Interrupted, false), [Resume, Cancel]);
        assert_eq!(
            offered(State::Unconfirmed, true),
            [Keep, Discard],
            "another file has the name"
        );
        assert_eq!(
            offered(State::Completed, true),
            [Open, ShowInFolder, Remove]
        );
        assert_eq!(offered(State::Completed, false), [Remove], "deleted");
        assert_eq!(offered(State::Failed, false), [Remove]);
        assert_eq!(offered(State::Cancelled, true), [ShowInFolder, Remove]);
    }

    #[test]
    fn download_fraction_is_unknown_without_a_size() {
        assert_eq!(fraction(5, None), None);
        assert_eq!(fraction(5, Some(0)), None);
        assert_eq!(fraction(1, Some(4)), Some(0.25));
    }
}
