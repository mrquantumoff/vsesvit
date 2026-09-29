//! The welcome: pages that choose the search engine, bring bookmarks over from other browsers,
//! add recommended extensions and make Vsesvit the default browser. It opens by itself on the
//! launch that created the profile (`onboarding::should_show`) and from the menu at any time;
//! closing it by any means marks the profile onboarded.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use vsesvit_core::import::{self, Found, Source};
use vsesvit_core::onboarding::{self, RECOMMENDED_EXTENSIONS, Recommended};
use vsesvit_core::search::SearchEngineId;
use windows_core::{Interface, Result};

use super::bookmarks::import_bookmarks;
use super::{Wired, default_browser, on_click};
use crate::bindings::*;
use crate::browser::Browser;
use crate::extensions::Progress;
use crate::window::BrowserWindow;
use crate::{anim, exec, pickers, xaml};

/// A fixed size, so the dialog does not jump between pages.
pub(super) const MARKUP: &str = r#"
  <Grid Width="600" Height="440" RowSpacing="20">
    <Grid.RowDefinitions><RowDefinition Height="*"/><RowDefinition Height="Auto"/></Grid.RowDefinitions>
    <ContentControl x:Name="WelcomePage" IsTabStop="False"
                    HorizontalContentAlignment="Stretch" VerticalContentAlignment="Stretch"/>
    <Grid Grid.Row="1" ColumnSpacing="8">
      <Grid.ColumnDefinitions>
        <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/>
      </Grid.ColumnDefinitions>
      <Button x:Name="WelcomeSkip" Content="Skip setup" Background="Transparent" BorderThickness="0"/>
      <StackPanel x:Name="WelcomeSteps" Grid.Column="1" Orientation="Horizontal" Spacing="6"
                  VerticalAlignment="Center"/>
      <StackPanel Grid.Column="2" Orientation="Horizontal" Spacing="8" HorizontalAlignment="Right">
        <Button x:Name="WelcomeBack" Content="Back" MinWidth="88"/>
        <Button x:Name="WelcomeNext" Content="Next" MinWidth="88" Style="{StaticResource AccentButtonStyle}"/>
      </StackPanel>
    </Grid>
  </Grid>"#;

/// The welcome's pages, declared in the order they come (`PAGES`, and each page's index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Hello,
    Search,
    Import,
    Extensions,
    DefaultBrowser,
    Done,
}

pub(crate) const PAGES: [Page; 6] = [
    Page::Hello,
    Page::Search,
    Page::Import,
    Page::Extensions,
    Page::DefaultBrowser,
    Page::Done,
];

impl Page {
    /// The `x:Name` of the page's root, which scripted runs look for.
    pub fn name(self) -> &'static str {
        match self {
            Page::Hello => "WelcomeHello",
            Page::Search => "WelcomeSearch",
            Page::Import => "WelcomeImport",
            Page::Extensions => "WelcomeExtensions",
            Page::DefaultBrowser => "WelcomeDefaultBrowser",
            Page::Done => "WelcomeDone",
        }
    }

    fn next_label(self) -> &'static str {
        match self {
            Page::Hello => "Get started",
            Page::Done => "Start browsing",
            _ => "Next",
        }
    }

    fn body(self) -> String {
        match self {
            Page::Hello => HELLO.to_owned(),
            Page::Search => SEARCH.to_owned(),
            Page::Import => IMPORT.to_owned(),
            Page::Extensions => EXTENSIONS.to_owned(),
            Page::DefaultBrowser => format!(
                r#"<TextBlock Text="Make Vsesvit your default browser" Style="{{StaticResource SubtitleTextBlockStyle}}"/>
  <TextBlock TextWrapping="Wrap" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"
             Text="Links you open from other apps, mail and documents then open here. Windows asks you to choose the default browser yourself, in Windows Settings."/>
  {}"#,
                default_browser::MARKUP
            ),
            Page::Done => DONE.to_owned(),
        }
    }

    /// The page's root: its body in a column that slides and fades in.
    fn markup(self) -> String {
        format!(
            r#"<StackPanel {{ns}} x:Name="{name}" Spacing="12">
  {motion}
  {body}
</StackPanel>"#,
            name = self.name(),
            motion = anim::implicit("StackPanel", anim::PAGE),
            body = self.body(),
        )
    }
}

const HELLO: &str = r#"
  <FontIcon Glyph="&#xE774;" FontSize="48" HorizontalAlignment="Left" Margin="0,24,0,8"
            Foreground="{ThemeResource AccentTextFillColorPrimaryBrush}"/>
  <TextBlock Text="Welcome to Vsesvit" Style="{StaticResource TitleTextBlockStyle}"/>
  <TextBlock TextWrapping="Wrap" Style="{StaticResource BodyLargeTextBlockStyle}"
             Text="A fast, private browser that puts you in charge of the web."/>
  <TextBlock TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}" Margin="0,12,0,0"
             Text="A few quick choices get you started: your search engine, your bookmarks from other browsers, a few extensions worth having, and whether Vsesvit opens your links. Everything here can be changed later in Settings."/>"#;

const SEARCH: &str = r#"
  <TextBlock Text="Choose your search engine" Style="{StaticResource SubtitleTextBlockStyle}"/>
  <TextBlock TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"
             Text="What you type in the address bar that is not a web address is searched with it."/>
  <ScrollViewer MaxHeight="300" VerticalScrollBarVisibility="Auto">
    <StackPanel x:Name="WelcomeEngines" Spacing="2" AutomationProperties.Name="Search engines"/>
  </ScrollViewer>"#;

const IMPORT: &str = r#"
  <TextBlock Text="Bring your bookmarks" Style="{StaticResource SubtitleTextBlockStyle}"/>
  <TextBlock TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"
             Text="Each browser's bookmarks go into a folder of their own on the bookmarks bar."/>
  <StackPanel x:Name="WelcomeBrowsers" Spacing="2" AutomationProperties.Name="Browsers to import from"/>
  <TextBlock x:Name="WelcomeNoBrowsers" Text="No other browsers found on this PC." Visibility="Collapsed"/>
  <StackPanel Orientation="Horizontal" Spacing="8">
    <Button x:Name="WelcomeImport" Content="Import"/>
    <Button x:Name="WelcomeImportFile" Content="From a bookmarks file…"/>
    <ProgressRing x:Name="WelcomeImporting" Width="20" Height="20" IsActive="False" Visibility="Collapsed"/>
  </StackPanel>
  <TextBlock x:Name="WelcomeImportStatus" TextWrapping="Wrap"
             Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>"#;

const EXTENSIONS: &str = r#"
  <TextBlock Text="Add extensions worth having" Style="{StaticResource SubtitleTextBlockStyle}"/>
  <TextBlock TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"
             Text="Each comes from the Chrome Web Store, checked against its publisher's signature. Add any now, or later from Extensions."/>
  <StackPanel x:Name="WelcomeExtensionRows" Spacing="4"/>"#;

const DONE: &str = r#"
  <FontIcon Glyph="&#xE930;" FontSize="48" HorizontalAlignment="Left" Margin="0,24,0,8"
            Foreground="{ThemeResource AccentTextFillColorPrimaryBrush}"/>
  <TextBlock Text="You're all set" Style="{StaticResource TitleTextBlockStyle}"/>
  <TextBlock TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"
             Text="To see these pages again, choose Welcome in the menu."/>"#;

struct Welcome {
    browser: Weak<Browser>,
    dialog: ContentDialog,
    host: ContentControl,
    steps: Panel,
    skip: UIElement,
    back: UIElement,
    next: ContentControl,
    pages: Vec<FrameworkElement>,
    at: Cell<usize>,
    engines: Vec<(SearchEngineId, IToggleButton)>,
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Wired> {
    let pages = PAGES
        .iter()
        .map(|page| {
            let element: FrameworkElement = xaml::load(&page.markup())?;
            anim::rest_on_load(&element)?;
            Ok(element)
        })
        .collect::<Result<Vec<_>>>()?;
    let engines = fill_engines(&pages[Page::Search as usize], browser)?;
    let importer = Importer::wire(&pages[Page::Import as usize], browser, window)?;
    let extensions = extension_rows(&pages[Page::Extensions as usize], browser)?;
    let default_browser = default_browser::wire(&pages[Page::DefaultBrowser as usize])?;

    let welcome = Rc::new(Welcome {
        browser: Rc::downgrade(browser),
        dialog: root.cast()?,
        host: xaml::find(root, "WelcomePage")?,
        steps: xaml::find(root, "WelcomeSteps")?,
        skip: xaml::find(root, "WelcomeSkip")?,
        back: xaml::find(root, "WelcomeBack")?,
        next: xaml::find(root, "WelcomeNext")?,
        pages,
        at: Cell::new(0),
        engines,
    });
    welcome.show(0, true)?;
    let w = Rc::downgrade(&welcome);
    on_click(&welcome.next, move || {
        if let Some(w) = w.upgrade() {
            w.next();
        }
    })?;
    let w = Rc::downgrade(&welcome);
    on_click(&welcome.back, move || {
        if let Some(w) = w.upgrade()
            && let Some(to) = w.at.get().checked_sub(1)
        {
            let _ = w.show(to, false);
        }
    })?;
    let dialog = welcome.dialog.clone();
    on_click(&welcome.skip, move || {
        let _ = dialog.Hide();
    })?;

    let b = Rc::downgrade(browser);
    Ok(Wired {
        _alive: vec![welcome, importer, Rc::new(extensions), default_browser],
        on_close: Some(Box::new(move || {
            if let Some(b) = b.upgrade()
                && let Err(e) = b.core(onboarding::finish)
            {
                log::warn!("marking the welcome done: {e}");
            }
        })),
    })
}

impl Welcome {
    fn next(&self) {
        let at = self.at.get();
        if PAGES[at] == Page::Search {
            self.apply_engine();
        }
        if at + 1 < PAGES.len() {
            let _ = self.show(at + 1, true);
        } else {
            let _ = self.dialog.Hide();
        }
    }

    /// Shows page `to`, coming in from the side it lies on.
    fn show(&self, to: usize, forward: bool) -> Result<()> {
        let page = &self.pages[to];
        let dx = if forward { anim::SLIDE } else { -anim::SLIDE };
        anim::prepare_entrance(&page.cast()?, dx)?;
        self.host.SetContent(page)?;
        self.at.set(to);
        let first = to == 0;
        let last = to + 1 == PAGES.len();
        xaml::set_visible(&self.back, !first)?;
        xaml::set_visible(&self.skip, !last)?;
        self.next
            .SetContent(&xaml::boxed(PAGES[to].next_label())?)?;
        self.show_steps(to)
    }

    /// A dot per page, the current one long and in the accent color.
    fn show_steps(&self, at: usize) -> Result<()> {
        let children = self.steps.Children()?;
        children.Clear()?;
        for index in 0..PAGES.len() {
            let (width, brush) = if index == at {
                (18, "AccentFillColorDefaultBrush")
            } else {
                (6, "ControlStrongFillColorDefaultBrush")
            };
            let dot: UIElement = xaml::load(&format!(
                r#"<Border {{ns}} Width="{width}" Height="6" CornerRadius="3" Background="{{ThemeResource {brush}}}"/>"#
            ))?;
            children.Append(&dot)?;
        }
        let label = format!("Step {} of {}", at + 1, PAGES.len());
        AutomationProperties::SetName(&self.steps, &label)
    }

    fn apply_engine(&self) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let chosen = self
            .engines
            .iter()
            .find(|(_, radio)| is_checked(radio))
            .map(|(id, _)| id.clone());
        let Some(chosen) = chosen else { return };
        let current = browser.core(|p| p.search_engines().default_engine().ok().map(|e| e.id));
        if current.as_ref() != Some(&chosen)
            && let Err(e) = browser.core(|p| p.search_engines().set_default(&chosen))
        {
            log::warn!("default search engine: {e}");
        }
    }
}

fn is_checked(toggle: &IToggleButton) -> bool {
    toggle.IsChecked().unwrap_or(false)
}

/// A radio button per search engine, the current default chosen.
fn fill_engines(
    page: &FrameworkElement,
    browser: &Browser,
) -> Result<Vec<(SearchEngineId, IToggleButton)>> {
    let (engines, default) = browser.core(|p| {
        let engines = p.search_engines().list().unwrap_or_default();
        let default = p.search_engines().default_engine().ok().map(|e| e.id);
        (engines, default)
    });
    let list: Panel = xaml::find(page, "WelcomeEngines")?;
    let children = list.Children()?;
    let mut radios = Vec::new();
    for engine in engines {
        let radio: IToggleButton = xaml::load(&format!(
            r#"<RadioButton {{ns}} GroupName="WelcomeEngine" Content="{}"/>"#,
            xaml::escape(&engine.name)
        ))?;
        radio.SetIsChecked(Some(default.as_ref() == Some(&engine.id)))?;
        children.Append(&radio.cast::<UIElement>()?)?;
        radios.push((engine.id, radio));
    }
    Ok(radios)
}

/// The import page: a box per browser found, imported one after another off the UI thread.
struct Importer {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    found: Vec<(Found, IToggleButton)>,
    buttons: [Control; 2],
    busy: UIElement,
    status: TextBlock,
    running: Cell<bool>,
}

impl Importer {
    fn wire(
        page: &FrameworkElement,
        browser: &Rc<Browser>,
        window: &Rc<BrowserWindow>,
    ) -> Result<Rc<Self>> {
        let list: Panel = xaml::find(page, "WelcomeBrowsers")?;
        let children = list.Children()?;
        let mut found = Vec::new();
        for browser in import::installed_browsers() {
            let check: IToggleButton = xaml::load(&format!(
                r#"<CheckBox {{ns}} Content="{}" IsChecked="True"/>"#,
                xaml::escape(&browser.name)
            ))?;
            children.Append(&check.cast::<UIElement>()?)?;
            found.push((browser, check));
        }
        let none: UIElement = xaml::find(page, "WelcomeNoBrowsers")?;
        xaml::set_visible(&none, found.is_empty())?;
        let this = Rc::new(Self {
            browser: Rc::downgrade(browser),
            window: Rc::downgrade(window),
            buttons: [
                xaml::find(page, "WelcomeImport")?,
                xaml::find(page, "WelcomeImportFile")?,
            ],
            busy: xaml::find(page, "WelcomeImporting")?,
            status: xaml::find(page, "WelcomeImportStatus")?,
            running: Cell::new(false),
            found,
        });
        xaml::set_visible(&this.buttons[0], !this.found.is_empty())?;
        let me = Rc::downgrade(&this);
        on_click(&this.buttons[0], move || {
            if let Some(me) = me.upgrade() {
                let chosen = me
                    .found
                    .iter()
                    .filter(|(_, check)| is_checked(check))
                    .map(|(found, _)| {
                        (
                            found.folder_title(),
                            found.name.clone(),
                            found.source.clone(),
                        )
                    })
                    .collect();
                me.run(chosen);
            }
        })?;
        let me = Rc::downgrade(&this);
        on_click(&this.buttons[1], move || {
            if let Some(me) = me.upgrade() {
                me.pick_file();
            }
        })?;
        Ok(this)
    }

    fn pick_file(self: &Rc<Self>) {
        let Some(owner) = self.window.upgrade().and_then(|w| w.window_id().ok()) else {
            return;
        };
        let me = self.clone();
        exec::spawn(async move {
            match pickers::pick_file(owner, &[".html", ".htm", ".json"]).await {
                Ok(Some(path)) => {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    let folder = import::FILE_FOLDER_TITLE.to_owned();
                    me.run(vec![(folder, name, Source::File(path))]);
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = me
                        .status
                        .SetText(&format!("Could not open the picker: {e}"));
                }
            }
        });
    }

    /// Imports each `(folder, from, source)` in turn; what happened to each shows as it ends.
    fn run(self: &Rc<Self>, sources: Vec<(String, String, Source)>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        if sources.is_empty() || self.running.replace(true) {
            return;
        }
        self.set_busy(true);
        let me = self.clone();
        exec::spawn(async move {
            let mut done: Vec<String> = Vec::new();
            for (folder, from, source) in sources {
                let mut shown = done.clone();
                shown.push(format!("Importing from {from}\u{2026}"));
                let _ = me.status.SetText(&shown.join("\n"));
                done.push(import_bookmarks(&browser, &folder, &from, source).await);
                if let Some((_, check)) = me.found.iter().find(|(f, _)| f.name == from) {
                    let _ = check.SetIsChecked(Some(false));
                }
            }
            let _ = me.status.SetText(&done.join("\n"));
            me.set_busy(false);
            me.running.set(false);
        });
    }

    fn set_busy(&self, busy: bool) {
        for button in &self.buttons {
            let _ = button.SetIsEnabled(!busy);
        }
        let _ = xaml::set_visible(&self.busy, busy);
        if let Ok(ring) = self.busy.cast::<ProgressRing>() {
            let _ = ring.SetIsActive(busy);
        }
    }
}

/// One recommended extension's row: what it is, and a button that installs it with progress.
struct ExtensionRow {
    recommended: Recommended,
    button: Button,
    progress: ProgressBar,
    status: TextBlock,
}

fn extension_rows(page: &FrameworkElement, browser: &Rc<Browser>) -> Result<Vec<Rc<ExtensionRow>>> {
    let list: Panel = xaml::find(page, "WelcomeExtensionRows")?;
    let children = list.Children()?;
    let mut rows = Vec::new();
    for recommended in RECOMMENDED_EXTENSIONS {
        let root: FrameworkElement = xaml::load(&format!(
            r#"<Grid {{ns}} ColumnSpacing="12" Padding="12,10" CornerRadius="6"
                    Background="{{ThemeResource CardBackgroundFillColorDefaultBrush}}">
  <Grid.ColumnDefinitions><ColumnDefinition/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
  <StackPanel Spacing="2" VerticalAlignment="Center">
    <TextBlock Text="{name}" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
    <TextBlock Text="{blurb}" TextWrapping="Wrap" Style="{{StaticResource CaptionTextBlockStyle}}"
               Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
    <ProgressBar x:Name="Progress" Margin="0,6,0,0" Visibility="Collapsed"/>
    <TextBlock x:Name="Status" TextWrapping="Wrap" Style="{{StaticResource CaptionTextBlockStyle}}" Visibility="Collapsed"/>
  </StackPanel>
  <Button x:Name="Install" Grid.Column="1" MinWidth="96" VerticalAlignment="Center"
          AutomationProperties.Name="Install {name}"/>
</Grid>"#,
            name = xaml::escape(recommended.name),
            blurb = xaml::escape(recommended.blurb),
        ))?;
        let row = Rc::new(ExtensionRow {
            recommended: *recommended,
            button: xaml::find(&root, "Install")?,
            progress: xaml::find(&root, "Progress")?,
            status: xaml::find(&root, "Status")?,
        });
        let installed = browser
            .core(|p| p.extensions().get(&recommended.id()))
            .is_ok_and(|e| e.is_some());
        row.set_installed(installed);
        let (r, b) = (Rc::downgrade(&row), Rc::downgrade(browser));
        on_click(&row.button, move || {
            if let (Some(r), Some(b)) = (r.upgrade(), b.upgrade()) {
                r.install(b);
            }
        })?;
        children.Append(&root.cast::<UIElement>()?)?;
        rows.push(row);
    }
    Ok(rows)
}

impl ExtensionRow {
    fn set_installed(&self, installed: bool) {
        if installed {
            self.set_button("Installed", false);
        } else {
            self.set_button("Install", true);
        }
    }

    fn set_button(&self, label: &str, enabled: bool) {
        let _ = xaml::boxed(label)
            .and_then(|label| self.button.cast::<ContentControl>()?.SetContent(&label));
        let _ = self
            .button
            .cast::<Control>()
            .and_then(|b| b.SetIsEnabled(enabled));
    }

    /// Installs through the same pipeline as the Extensions dialog. A failure stays in the row
    /// and the button offers to try again; the rest of the welcome goes on meanwhile.
    fn install(self: &Rc<Self>, browser: Rc<Browser>) {
        self.set_button("Installing\u{2026}", false);
        let _ = self.progress.SetIsIndeterminate(true);
        let _ = xaml::set_visible(&self.progress, true);
        self.show_status("Starting");
        let me = self.clone();
        exec::spawn(async move {
            let shown = me.clone();
            let progress = move |p: Progress| shown.show_progress(&p);
            let result = browser
                .install_extension(me.recommended.install_source(), &progress)
                .await;
            let _ = xaml::set_visible(&me.progress, false);
            match result {
                Ok(ext) => {
                    me.set_installed(true);
                    match browser.extensions.engine_error(&ext.id) {
                        Some(e) => me.show_status(&format!("WebView2 did not load it: {e}")),
                        None => {
                            let _ = xaml::set_visible(&me.status, false);
                        }
                    }
                }
                Err(e) => {
                    me.show_status(&format!("Not installed: {e}"));
                    me.set_button("Try again", true);
                }
            }
        });
    }

    fn show_status(&self, text: &str) {
        let _ = self.status.SetText(text);
        let _ = xaml::set_visible(&self.status, true);
    }

    fn show_progress(&self, progress: &Progress) {
        self.show_status(&progress.text);
        match progress.fraction {
            Some(fraction) => {
                let _ = self.progress.SetIsIndeterminate(false);
                if let Ok(range) = self.progress.cast::<RangeBase>() {
                    let _ = range.SetMaximum(1.0);
                    let _ = range.SetValue(fraction);
                }
            }
            None => {
                let _ = self.progress.SetIsIndeterminate(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PAGES;

    #[test]
    fn pages_come_in_declared_order() {
        for (index, page) in PAGES.iter().enumerate() {
            assert_eq!(*page as usize, index, "{page:?}");
        }
    }
}
