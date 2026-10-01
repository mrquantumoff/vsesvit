//! History: recent pages or a search, opening one, deleting a page, and clearing a time range;
//! and, as Chrome's history page has beside it, tabs from other devices.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use vsesvit_core::history::HistoryEntry;
use vsesvit_core::sync::Changed;
use windows_core::{IInspectable, Interface, Result};

use super::{Category, Wired, on_click, side_list};
use crate::bindings::*;
use crate::browser::Browser;
use crate::window::BrowserWindow;
use crate::xaml;

pub(super) const MARKUP: &str = r#"
  <Grid ColumnSpacing="16">
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="200"/>
      <ColumnDefinition Width="*"/>
    </Grid.ColumnDefinitions>
    <ListView x:Name="HistorySections" AutomationProperties.Name="History sections"/>
    <Grid x:Name="HistoryPanel" Grid.Column="1" RowSpacing="12">
      <Grid.RowDefinitions>
        <RowDefinition Height="Auto"/><RowDefinition Height="*"/><RowDefinition Height="Auto"/>
      </Grid.RowDefinitions>
      <AutoSuggestBox x:Name="HistorySearch" PlaceholderText="Search history" QueryIcon="Find"/>
      <Grid Grid.Row="1">
        <ListView x:Name="HistoryList" SelectionMode="Single"/>
        <TextBlock x:Name="HistoryEmpty" Text="No history" HorizontalAlignment="Center"
                   VerticalAlignment="Center" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
      </Grid>
      <Grid Grid.Row="2" ColumnSpacing="8">
        <Grid.ColumnDefinitions>
          <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/>
          <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
        </Grid.ColumnDefinitions>
        <Button x:Name="HistoryOpen" Content="Open in a new tab"/>
        <Button x:Name="HistoryDelete" Grid.Column="1" Content="Delete page from history"/>
        <ComboBox x:Name="HistoryRange" Grid.Column="3" MinWidth="170"/>
        <Button x:Name="HistoryClear" Grid.Column="4" Content="Clear"/>
      </Grid>
    </Grid>
    {other_devices}
  </Grid>"#;

/// The sections down the side.
const SECTIONS: [Category; 2] = [
    Category {
        label: "History",
        glyph: "\u{E81C}",
        panel: "HistoryPanel",
    },
    Category {
        label: "Tabs from other devices",
        glyph: "\u{E772}",
        panel: "OtherDevicesPanel",
    },
];

const SHOWN: usize = 300;

/// Time ranges for "Clear", newest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Range {
    LastHour,
    LastDay,
    LastWeek,
    LastFourWeeks,
    AllTime,
}

const RANGES: [(Range, &str); 5] = [
    (Range::LastHour, "Last hour"),
    (Range::LastDay, "Last 24 hours"),
    (Range::LastWeek, "Last 7 days"),
    (Range::LastFourWeeks, "Last 4 weeks"),
    (Range::AllTime, "All time"),
];

impl Range {
    /// Where the range starts, for a clear at `now_ms`.
    fn start(self, now_ms: i64) -> i64 {
        const HOUR: i64 = 60 * 60 * 1000;
        match self {
            Self::LastHour => now_ms - HOUR,
            Self::LastDay => now_ms - 24 * HOUR,
            Self::LastWeek => now_ms - 7 * 24 * HOUR,
            Self::LastFourWeeks => now_ms - 28 * 24 * HOUR,
            Self::AllTime => 0,
        }
        .max(0)
    }
}

struct Page {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    search: AutoSuggestBox,
    list: ListView,
    empty: UIElement,
    range: ComboBox,
    shown: RefCell<Vec<HistoryEntry>>,
}

/// A button's handler.
type Action = fn(&Page);

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Wired> {
    let page = Rc::new(Page {
        browser: Rc::downgrade(browser),
        window: Rc::downgrade(window),
        search: xaml::find(root, "HistorySearch")?,
        list: xaml::find(root, "HistoryList")?,
        empty: xaml::find(root, "HistoryEmpty")?,
        range: xaml::find(root, "HistoryRange")?,
        shown: RefCell::new(Vec::new()),
    });
    let ranges = page.range.cast::<ItemsControl>()?.Items()?;
    for (_, label) in RANGES {
        ranges.Append(&xaml::boxed(label)?)?;
    }
    page.range.cast::<Selector>()?.SetSelectedIndex(0)?;
    page.render();

    let p = Rc::downgrade(&page);
    page.search
        .TextChanged(move |_, _| {
            if let Some(p) = p.upgrade() {
                p.render();
            }
        })?
        .forget();
    let buttons: [(&str, Action); 3] = [
        ("HistoryOpen", Page::open),
        ("HistoryDelete", Page::delete),
        ("HistoryClear", Page::clear),
    ];
    for (name, action) in buttons {
        let button: Button = xaml::find(root, name)?;
        let p = Rc::downgrade(&page);
        on_click(&button, move || {
            if let Some(p) = p.upgrade() {
                action(&p);
            }
        })?;
    }
    side_list(root, "HistorySections", &SECTIONS)?;
    let devices = super::other_devices::wire(root, browser, window)?;
    let (p, d) = (Rc::downgrade(&page), Rc::downgrade(&devices));
    let synced: Rc<dyn Fn(&Changed)> = Rc::new(move |changed| {
        if changed.history
            && let Some(p) = p.upgrade()
        {
            p.render();
        }
        if changed.sessions
            && let Some(d) = d.upgrade()
        {
            d.render();
        }
    });
    browser.sync().on_applied(&synced);
    let d = Rc::downgrade(&devices);
    let signed_in: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(d) = d.upgrade() {
            d.render();
        }
    });
    browser.sync().on_change(&signed_in);
    Ok(Wired {
        _alive: vec![page, devices, Rc::new(synced), Rc::new(signed_in)],
        on_close: None,
    })
}

impl Page {
    fn render(&self) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let query = self.search.Text().unwrap_or_default();
        let entries = browser.core(|p| {
            let mut history = p.history();
            if query.trim().is_empty() {
                history
                    .visits_between(0, i64::MAX, SHOWN)
                    .map(|visits| newest_per_page(visits.into_iter().map(|(entry, _)| entry)))
            } else {
                history.search(&query, SHOWN)
            }
        });
        let entries = entries.unwrap_or_else(|e| {
            log::warn!("history: {e}");
            Vec::new()
        });
        let Ok(items) = self.list.cast::<ItemsControl>().and_then(|l| l.Items()) else {
            return;
        };
        let _ = items.Clear();
        let now = now_ms();
        // Only entries that got a row, so a row's index always finds its own entry.
        let mut shown = Vec::with_capacity(entries.len());
        for entry in entries {
            match row(&entry, now).and_then(|row| items.Append(&row)) {
                Ok(()) => shown.push(entry),
                Err(e) => log::warn!("history row: {e}"),
            }
        }
        let _ = xaml::set_visible(&self.empty, shown.is_empty());
        *self.shown.borrow_mut() = shown;
    }

    fn selected(&self) -> Option<HistoryEntry> {
        let index = super::selected_index(&self.list)?;
        self.shown.borrow().get(index).cloned()
    }

    fn open(&self) {
        let (Some(entry), Some(window)) = (self.selected(), self.window.upgrade()) else {
            return;
        };
        if let Err(e) = window.open_url_tab(entry.url.as_str(), true) {
            log::warn!("open {}: {e}", entry.url);
        }
    }

    fn delete(&self) {
        let (Some(entry), Some(browser)) = (self.selected(), self.browser.upgrade()) else {
            return;
        };
        if let Err(e) = browser.core(|p| p.history().delete_url(&entry.url)) {
            log::warn!("delete {}: {e}", entry.url);
        }
        self.render();
    }

    fn clear(&self) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let index = super::selected_index(&self.range);
        let Some((range, _)) = index.and_then(|i| RANGES.get(i)) else {
            return;
        };
        let now = now_ms();
        if let Err(e) = browser.core(|p| p.history().delete_range(range.start(now), now + 1)) {
            log::warn!("clear history: {e}");
        }
        self.render();
    }
}

/// Visits newest first, one line per page.
fn newest_per_page(entries: impl Iterator<Item = HistoryEntry>) -> Vec<HistoryEntry> {
    let mut seen = std::collections::HashSet::new();
    entries.filter(|e| seen.insert(e.url.clone())).collect()
}

fn row(entry: &HistoryEntry, now_ms: i64) -> Result<IInspectable> {
    let title = if entry.title.is_empty() {
        entry.url.as_str()
    } else {
        &entry.title
    };
    let element: UIElement = xaml::load(&format!(
        r#"<Grid {{ns}} ColumnSpacing="12" Padding="0,6">
             <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
             <StackPanel>
               <TextBlock Text="{title}" TextTrimming="CharacterEllipsis"/>
               <TextBlock Text="{url}" TextTrimming="CharacterEllipsis" Style="{{StaticResource CaptionTextBlockStyle}}"
                          Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
             </StackPanel>
             <TextBlock Grid.Column="1" Text="{when}" VerticalAlignment="Center"
                        Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
           </Grid>"#,
        title = xaml::escape(title),
        url = xaml::escape(entry.url.as_str()),
        when = xaml::escape(&ago(now_ms, entry.last_visit_ms)),
    ))?;
    element.cast()
}

/// "just now", "5 minutes ago", "3 hours ago", "2 days ago".
pub(super) fn ago(now_ms: i64, then_ms: i64) -> String {
    let minutes = (now_ms - then_ms).max(0) / 60_000;
    let (n, unit) = match minutes {
        0 => return "just now".to_owned(),
        m if m < 60 => (m, "minute"),
        m if m < 24 * 60 => (m / 60, "hour"),
        m => (m / (24 * 60), "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times() {
        let now = 10 * 24 * 3_600_000;
        assert_eq!(ago(now, now - 30_000), "just now");
        assert_eq!(ago(now, now - 60_000), "1 minute ago");
        assert_eq!(ago(now, now - 5 * 3_600_000), "5 hours ago");
        assert_eq!(ago(now, now - 2 * 24 * 3_600_000), "2 days ago");
        assert_eq!(ago(now, now + 1_000), "just now");
    }

    #[test]
    fn ranges_start_before_now() {
        let now = 30 * 24 * 3_600_000;
        assert_eq!(Range::LastHour.start(now), now - 3_600_000);
        assert_eq!(Range::LastFourWeeks.start(now), 2 * 24 * 3_600_000);
        assert_eq!(Range::AllTime.start(now), 0);
        assert_eq!(Range::LastWeek.start(1000), 0);
    }
}
