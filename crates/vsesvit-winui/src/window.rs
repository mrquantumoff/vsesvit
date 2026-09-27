//! One browser window: the tab strip in the title bar, the toolbar, the bookmarks bar and the
//! page grid that hosts every tab's web view.
//!
//! Order of tabs lives in the `TabView`'s items (the user can reorder them); `tabs` only owns
//! the `Tab` values. No `RefCell` borrow is held across a XAML call, because XAML raises events
//! such as `SelectionChanged` synchronously from inside those calls.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::bookmarks_bar::{self, BarItem, Disposition, OpenLink};
use crate::browser::{Browser, ClosedTab};
use crate::dialogs::{self, Dialog};
use crate::popup::{self, Activation, ExtensionAction};
use crate::shortcuts::{BINDINGS, Command, Mods};
use crate::tab::{Initial, Tab, TabId};
use crate::updates::{Action, Banner, Severity};
use crate::{capture, exec, omnibox, platform, xaml};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Show {
    Activate,
    /// Shown without taking focus from the user's current window (scripted runs).
    NoActivate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    Run(Command),
    Show(Dialog),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    End,
    After(TabId),
}

const WINDOW_XAML: &str = r#"
<Grid {ns}>
  <Grid.Resources>
    <Style x:Key="ToolbarButton" TargetType="Button" BasedOn="{StaticResource DefaultButtonStyle}">
      <Setter Property="Background" Value="Transparent"/>
      <Setter Property="BorderThickness" Value="0"/>
      <Setter Property="Padding" Value="0"/>
      <Setter Property="Width" Value="36"/>
      <Setter Property="Height" Value="32"/>
    </Style>
    <Style x:Key="ToolbarToggle" TargetType="ToggleButton" BasedOn="{StaticResource DefaultToggleButtonStyle}">
      <Setter Property="Background" Value="Transparent"/>
      <Setter Property="BorderThickness" Value="0"/>
      <Setter Property="Padding" Value="0"/>
      <Setter Property="Width" Value="36"/>
      <Setter Property="Height" Value="32"/>
    </Style>
    <StaticResource x:Key="ButtonBackgroundPointerOver" ResourceKey="SubtleFillColorSecondaryBrush"/>
    <StaticResource x:Key="ButtonBackgroundPressed" ResourceKey="SubtleFillColorTertiaryBrush"/>
    <StaticResource x:Key="ButtonBackgroundDisabled" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ButtonBorderBrushPointerOver" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ButtonBorderBrushPressed" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ButtonBorderBrushDisabled" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundPointerOver" ResourceKey="SubtleFillColorSecondaryBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundPressed" ResourceKey="SubtleFillColorTertiaryBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundChecked" ResourceKey="SubtleFillColorTransparentBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundCheckedPointerOver" ResourceKey="SubtleFillColorSecondaryBrush"/>
    <StaticResource x:Key="ToggleButtonBackgroundCheckedPressed" ResourceKey="SubtleFillColorTertiaryBrush"/>
    <StaticResource x:Key="ToggleButtonForegroundChecked" ResourceKey="AccentTextFillColorPrimaryBrush"/>
    <StaticResource x:Key="ToggleButtonForegroundCheckedPointerOver" ResourceKey="AccentTextFillColorPrimaryBrush"/>
    <StaticResource x:Key="ToggleButtonForegroundCheckedPressed" ResourceKey="AccentTextFillColorSecondaryBrush"/>
  </Grid.Resources>
  <Grid.RowDefinitions>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="Auto"/>
    <RowDefinition Height="*"/>
  </Grid.RowDefinitions>

  <TabView x:Name="Tabs" TabWidthMode="Equal" IsAddTabButtonVisible="True"
           CanReorderTabs="True" CanDragTabs="True" AllowDropTabs="True" VerticalAlignment="Bottom">
    <TabView.TabStripHeader>
      <Grid Width="8"/>
    </TabView.TabStripHeader>
    <TabView.TabStripFooter>
      <Grid x:Name="DragRegion" Background="Transparent" MinWidth="188"/>
    </TabView.TabStripFooter>
  </TabView>

  <Grid x:Name="Toolbar" Grid.Row="1" Padding="6,4,6,4" ColumnSpacing="2"
        Background="{ThemeResource SolidBackgroundFillColorTertiaryBrush}">
    <Grid.ColumnDefinitions>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="*"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
      <ColumnDefinition Width="Auto"/>
    </Grid.ColumnDefinitions>
    <Button x:Name="Back" Style="{StaticResource ToolbarButton}" IsEnabled="False"
            ToolTipService.ToolTip="Back (Alt+Left)" AutomationProperties.Name="Back">
      <FontIcon Glyph="&#xE72B;" FontSize="16"/>
    </Button>
    <Button x:Name="Forward" Grid.Column="1" Style="{StaticResource ToolbarButton}" IsEnabled="False"
            ToolTipService.ToolTip="Forward (Alt+Right)" AutomationProperties.Name="Forward">
      <FontIcon Glyph="&#xE72A;" FontSize="16"/>
    </Button>
    <Button x:Name="Reload" Grid.Column="2" Style="{StaticResource ToolbarButton}"
            ToolTipService.ToolTip="Refresh (Ctrl+R)" AutomationProperties.Name="Refresh">
      <FontIcon x:Name="ReloadGlyph" Glyph="&#xE72C;" FontSize="16"/>
    </Button>
    <AutoSuggestBox x:Name="Address" Grid.Column="3" Margin="6,0" VerticalAlignment="Center"
                    PlaceholderText="Search or enter web address" UpdateTextOnSelect="False"
                    AutomationProperties.Name="Address and search bar"/>
    <ToggleButton x:Name="Star" Grid.Column="4" Style="{StaticResource ToolbarToggle}"
                  ToolTipService.ToolTip="Bookmark this page (Ctrl+D)" AutomationProperties.Name="Bookmark this page">
      <FontIcon x:Name="StarGlyph" Glyph="&#xE734;" FontSize="16"/>
    </ToggleButton>
    <StackPanel x:Name="ExtensionActions" Grid.Column="5" Orientation="Horizontal" Spacing="2"/>
    <Button x:Name="More" Grid.Column="6" Style="{StaticResource ToolbarButton}"
            ToolTipService.ToolTip="Settings and more" AutomationProperties.Name="Settings and more">
      <FontIcon Glyph="&#xE712;" FontSize="16"/>
      <Button.Flyout>
        <MenuFlyout Placement="BottomEdgeAlignedRight">
          <MenuFlyoutItem x:Name="MenuNewTab" Text="New tab" KeyboardAcceleratorTextOverride="Ctrl+T">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xECCD;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuNewWindow" Text="New window" KeyboardAcceleratorTextOverride="Ctrl+N">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE78B;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutSeparator/>
          <MenuFlyoutItem x:Name="MenuBookmarks" Text="Bookmarks" KeyboardAcceleratorTextOverride="Ctrl+Shift+O">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE728;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuHistory" Text="History" KeyboardAcceleratorTextOverride="Ctrl+H">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE81C;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuExtensions" Text="Extensions">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xEA86;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutSeparator/>
          <MenuFlyoutItem x:Name="MenuSettings" Text="Settings">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE713;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
          <MenuFlyoutItem x:Name="MenuAbout" Text="About Vsesvit">
            <MenuFlyoutItem.Icon><FontIcon Glyph="&#xE946;"/></MenuFlyoutItem.Icon>
          </MenuFlyoutItem>
        </MenuFlyout>
      </Button.Flyout>
    </Button>
  </Grid>

  <Grid x:Name="BookmarksBar" Grid.Row="2" Height="32" Padding="8,0,8,4"
        Background="{ThemeResource SolidBackgroundFillColorTertiaryBrush}">
    <StackPanel x:Name="BookmarkItems" Orientation="Horizontal" Spacing="2" VerticalAlignment="Center"/>
    <TextBlock x:Name="BookmarksHint" Margin="6,0" VerticalAlignment="Center"
               Style="{StaticResource CaptionTextBlockStyle}"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"
               Text="For quick access, place your bookmarks here on the bookmarks bar."/>
  </Grid>

  <InfoBar x:Name="UpdateBar" Grid.Row="3" IsOpen="False" CornerRadius="0" BorderThickness="0,1,0,0">
    <InfoBar.ActionButton>
      <Button x:Name="UpdateAction"/>
    </InfoBar.ActionButton>
  </InfoBar>

  <Grid x:Name="Pages" Grid.Row="4"
        Background="{ThemeResource SolidBackgroundFillColorTertiaryBrush}"
        BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}" BorderThickness="0,1,0,0"/>
</Grid>"#;

/// Named parts of `WINDOW_XAML`.
struct Chrome {
    root: FrameworkElement,
    tab_view: TabView,
    drag_region: UIElement,
    toolbar: UIElement,
    back: Control,
    forward: Control,
    reload: Button,
    reload_glyph: FontIcon,
    address: AutoSuggestBox,
    star: ToggleButton,
    star_glyph: FontIcon,
    extension_actions: Panel,
    bookmarks_bar: UIElement,
    bookmark_items: Panel,
    bookmarks_hint: UIElement,
    update_bar: InfoBar,
    update_action: Button,
    pages: Panel,
}

impl Chrome {
    fn load() -> Result<Self> {
        let root: FrameworkElement = xaml::load(WINDOW_XAML)?;
        Ok(Self {
            tab_view: xaml::find(&root, "Tabs")?,
            drag_region: xaml::find(&root, "DragRegion")?,
            toolbar: xaml::find(&root, "Toolbar")?,
            back: xaml::find(&root, "Back")?,
            forward: xaml::find(&root, "Forward")?,
            reload: xaml::find(&root, "Reload")?,
            reload_glyph: xaml::find(&root, "ReloadGlyph")?,
            address: xaml::find(&root, "Address")?,
            star: xaml::find(&root, "Star")?,
            star_glyph: xaml::find(&root, "StarGlyph")?,
            extension_actions: xaml::find(&root, "ExtensionActions")?,
            bookmarks_bar: xaml::find(&root, "BookmarksBar")?,
            bookmark_items: xaml::find(&root, "BookmarkItems")?,
            bookmarks_hint: xaml::find(&root, "BookmarksHint")?,
            update_bar: xaml::find(&root, "UpdateBar")?,
            update_action: xaml::find(&root, "UpdateAction")?,
            pages: xaml::find(&root, "Pages")?,
            root,
        })
    }
}

pub(crate) struct BrowserWindow {
    browser: Weak<Browser>,
    window: Window,
    ui: Chrome,
    tabs: RefCell<Vec<Rc<Tab>>>,
    /// The user typed into the address box since it last showed the page URL.
    address_edited: Cell<bool>,
    /// The tab the toolbar currently shows.
    shown_tab: Cell<Option<TabId>>,
    fullscreen: Cell<bool>,
    bookmarks_bar_wanted: Cell<bool>,
    bar_items: RefCell<Vec<BarItem>>,
    dialog_open: Cell<bool>,
    /// What the update bar shows; the user may have closed it since.
    update_banner: RefCell<Option<Banner>>,
    closed: Cell<bool>,
    me: Weak<BrowserWindow>,
}

impl BrowserWindow {
    pub fn create(browser: &Rc<Browser>, show: Show) -> Result<Rc<Self>> {
        let ui = Chrome::load()?;
        let window = Window::new()?;
        window.SetTitle("Vsesvit")?;
        window.SetContent(&ui.root)?;
        window.SetExtendsContentIntoTitleBar(true)?;
        window.SetTitleBar(&ui.drag_region)?;
        let window2 = window.cast::<IWindow2>()?;
        window2.SetSystemBackdrop(&MicaBackdrop::new()?.cast::<SystemBackdrop>()?)?;

        let this = Rc::new_cyclic(|me| Self {
            browser: Rc::downgrade(browser),
            window,
            ui,
            tabs: RefCell::new(Vec::new()),
            address_edited: Cell::new(false),
            shown_tab: Cell::new(None),
            fullscreen: Cell::new(false),
            bookmarks_bar_wanted: Cell::new(true),
            bar_items: RefCell::new(Vec::new()),
            dialog_open: Cell::new(false),
            update_banner: RefCell::new(None),
            closed: Cell::new(false),
            me: me.clone(),
        });
        this.wire()?;
        this.install_accelerators()?;
        this.size_for_screen()?;
        match show {
            Show::Activate => this.window.Activate()?,
            Show::NoActivate => window2.AppWindow()?.ShowWithActivation(false)?,
        }
        Ok(this)
    }

    fn size_for_screen(&self) -> Result<()> {
        let hwnd = platform::window_handle(&self.window)?;
        let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.0;
        let size = SizeInt32 {
            width: (1280.0 * scale) as i32,
            height: (860.0 * scale) as i32,
        };
        self.window.cast::<IWindow2>()?.AppWindow()?.Resize(size)
    }

    pub fn browser(&self) -> Option<Rc<Browser>> {
        self.browser.upgrade()
    }

    pub fn xaml_root(&self) -> Result<XamlRoot> {
        self.ui.root.cast::<UIElement>()?.XamlRoot()
    }

    fn me(&self) -> Rc<Self> {
        self.me
            .upgrade()
            .expect("a window method runs while the window is alive")
    }

    // ---- tabs ----

    pub fn open_start_tabs(&self, urls: &[String]) -> Result<()> {
        let mut targets: Vec<Initial> = urls
            .iter()
            .filter_map(|text| omnibox::navigation_target(text))
            .map(Initial::Url)
            .collect();
        if targets.is_empty() {
            targets.push(Initial::Blank);
        }
        for (index, initial) in targets.into_iter().enumerate() {
            self.open_tab(initial, Placement::End, index == 0)?;
        }
        Ok(())
    }

    pub fn open_url_tab(&self, url: &str, foreground: bool) -> Result<Rc<Tab>> {
        self.open_tab(Initial::Url(url.to_owned()), Placement::End, foreground)
    }

    pub fn open_blank_tab(&self) -> Result<Rc<Tab>> {
        let tab = self.open_tab(Initial::Blank, Placement::End, true)?;
        self.focus_address();
        Ok(tab)
    }

    /// A page's new-window request: a tab right after its opener.
    pub fn open_tab_from(&self, opener: TabId, initial: Initial, background: bool) {
        if let Err(e) = self.open_tab(initial, Placement::After(opener), !background) {
            log::error!("open tab: {e}");
        }
    }

    fn open_tab(
        &self,
        initial: Initial,
        placement: Placement,
        foreground: bool,
    ) -> Result<Rc<Tab>> {
        let browser = self.browser().ok_or_else(windows_core::Error::empty)?;
        let tab = Tab::new(browser.next_tab_id(), self.me.clone())?;
        xaml::set_visible(tab.view(), false)?;
        self.ui
            .pages
            .Children()?
            .Append(&tab.view().cast::<UIElement>()?)?;
        self.tabs.borrow_mut().push(tab.clone());

        let items = self.ui.tab_view.TabItems()?;
        let index = match placement {
            Placement::End => items.Size()?,
            Placement::After(opener) => self.index_of(opener).map_or(items.Size()?, |i| i + 1),
        };
        items.InsertAt(index, &tab.item().cast::<IInspectable>()?)?;
        if foreground {
            self.ui.tab_view.SetSelectedItem(tab.item())?;
        }
        self.sync_selection();
        exec::spawn(tab.clone().start(
            browser.engine().environment().clone(),
            browser.page_script(),
            initial,
        ));
        browser.session_changed();
        Ok(tab)
    }

    pub fn close_tab(&self, id: TabId) {
        let Some(tab) = self.tab(id) else { return };
        if let Err(e) = self.remove_tab(&tab) {
            log::warn!("close tab {id}: {e}");
        }
        if let Some(browser) = self.browser() {
            let state = tab.state();
            browser.remember_closed(ClosedTab {
                url: state.url,
                title: state.title,
            });
            browser.session_changed();
        }
        let last = self.tabs.borrow().is_empty();
        if last {
            let _ = self.window.Close();
        }
    }

    fn remove_tab(&self, tab: &Rc<Tab>) -> Result<()> {
        let items = self.ui.tab_view.TabItems()?;
        if let Some(index) = self.index_of(tab.id) {
            let count = items.Size()?;
            let selected = self.ui.tab_view.SelectedIndex()?;
            if selected == index as i32 && count > 1 {
                let next = if index + 1 < count {
                    index + 1
                } else {
                    index - 1
                };
                self.ui.tab_view.SetSelectedIndex(next as i32)?;
            }
            items.RemoveAt(index)?;
        }
        let children = self.ui.pages.Children()?;
        let view = tab.view().cast::<UIElement>()?;
        let mut index = 0;
        if children.IndexOf(&view, &mut index)? {
            children.RemoveAt(index)?;
        }
        tab.close();
        self.tabs.borrow_mut().retain(|t| t.id != tab.id);
        self.sync_selection();
        Ok(())
    }

    pub fn tab(&self, id: TabId) -> Option<Rc<Tab>> {
        self.tabs.borrow().iter().find(|t| t.id == id).cloned()
    }

    /// Tabs in strip order.
    pub fn tabs_in_order(&self) -> Vec<Rc<Tab>> {
        let Ok(items) = self.ui.tab_view.TabItems() else {
            return Vec::new();
        };
        let tabs = self.tabs.borrow().clone();
        let mut ordered = Vec::with_capacity(tabs.len());
        for index in 0..items.Size().unwrap_or(0) {
            let Ok(item) = items.GetAt(index) else {
                continue;
            };
            if let Some(tab) = tabs.iter().find(|t| xaml::same_object(t.item(), &item)) {
                ordered.push(tab.clone());
            }
        }
        ordered
    }

    fn index_of(&self, id: TabId) -> Option<u32> {
        self.tabs_in_order()
            .iter()
            .position(|t| t.id == id)
            .map(|i| i as u32)
    }

    pub fn active_tab(&self) -> Option<Rc<Tab>> {
        let selected = self.ui.tab_view.SelectedItem().ok()?;
        self.tabs
            .borrow()
            .iter()
            .find(|t| xaml::same_object(t.item(), &selected))
            .cloned()
    }

    fn select_index(&self, index: usize) {
        let _ = self.ui.tab_view.SetSelectedIndex(index as i32);
        self.sync_selection();
    }

    fn select_relative(&self, step: isize) {
        let count = self.tabs.borrow().len() as isize;
        if count == 0 {
            return;
        }
        let current = self.ui.tab_view.SelectedIndex().unwrap_or(0) as isize;
        self.select_index((current + step).rem_euclid(count) as usize);
    }

    /// Shows the selected tab's web view, hides the rest, and refreshes the toolbar.
    fn sync_selection(&self) {
        let active = self.active_tab();
        let tabs = self.tabs.borrow().clone();
        for tab in tabs {
            let visible = active.as_ref().is_some_and(|a| a.id == tab.id);
            if xaml::is_visible(tab.view()) != visible {
                let _ = xaml::set_visible(tab.view(), visible);
            }
        }
        if self.fullscreen.get() && !active.as_ref().is_some_and(|t| t.state().fullscreen) {
            self.set_fullscreen(false);
        }
        let active_id = active.map(|t| t.id);
        if self.shown_tab.replace(active_id) != active_id {
            self.address_edited.set(false);
        }
        self.refresh_chrome();
    }

    /// Called by a tab whenever its state changed.
    pub fn tab_updated(&self, tab: &Tab) {
        if self.active_tab().is_some_and(|a| a.id == tab.id) {
            self.refresh_chrome();
        }
    }

    pub fn tab_fullscreen_changed(&self, tab: &Tab, fullscreen: bool) {
        if self.active_tab().is_some_and(|a| a.id == tab.id) {
            self.set_fullscreen(fullscreen);
        }
    }

    fn refresh_chrome(&self) {
        let state = self.active_tab().map(|t| t.state()).unwrap_or_default();
        let _ = self.ui.back.SetIsEnabled(state.can_go_back);
        let _ = self.ui.forward.SetIsEnabled(state.can_go_forward);
        let (glyph, tip) = if state.loading {
            ("\u{E711}", "Stop")
        } else {
            ("\u{E72C}", "Refresh (Ctrl+R)")
        };
        let _ = self.ui.reload_glyph.SetGlyph(glyph);
        let _ = xaml::boxed(tip).and_then(|tip| ToolTipService::SetToolTip(&self.ui.reload, &tip));
        if !self.address_edited.get() {
            let shown = omnibox::display_url(&state.url);
            if self.ui.address.Text().is_ok_and(|t| t != shown) {
                let _ = self.ui.address.SetText(shown);
            }
        }
        self.show_star(state.starred);
        let title = if state.title.is_empty() || state.url.is_empty() {
            "Vsesvit".to_owned()
        } else {
            format!("{} - Vsesvit", state.title)
        };
        let _ = self.window.SetTitle(&title);
    }

    fn show_star(&self, starred: bool) {
        let _ = self.ui.star.SetIsChecked(Some(starred));
        let _ = self
            .ui
            .star_glyph
            .SetGlyph(if starred { "\u{E735}" } else { "\u{E734}" });
    }

    pub fn address_text(&self) -> String {
        self.ui.address.Text().unwrap_or_default()
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.borrow().len()
    }

    // ---- commands ----

    pub fn run(&self, command: Command) {
        let active = self.active_tab();
        match command {
            Command::NewTab => {
                if let Err(e) = self.open_blank_tab() {
                    log::error!("new tab: {e}");
                }
            }
            Command::NewWindow => {
                if let Some(browser) = self.browser()
                    && let Err(e) = browser.open_window(&[], Show::Activate)
                {
                    log::error!("new window: {e}");
                }
            }
            Command::CloseTab => {
                if let Some(tab) = active {
                    self.close_tab(tab.id);
                }
            }
            Command::ReopenClosedTab => {
                if let Some(closed) = self.browser().and_then(|b| b.take_closed()) {
                    log::info!("reopening {} ({})", closed.url, closed.title);
                    if let Err(e) = self.open_url_tab(&closed.url, true) {
                        log::error!("reopen tab: {e}");
                    }
                }
            }
            Command::FocusAddress => self.focus_address(),
            Command::Reload => {
                if let Some(tab) = active {
                    tab.reload();
                }
            }
            Command::Back => {
                if let Some(tab) = active {
                    tab.go_back();
                }
            }
            Command::Forward => {
                if let Some(tab) = active {
                    tab.go_forward();
                }
            }
            Command::NextTab => self.select_relative(1),
            Command::PreviousTab => self.select_relative(-1),
            Command::SelectTab(index) => {
                if usize::from(index) < self.tab_count() {
                    self.select_index(usize::from(index));
                }
            }
            Command::SelectLastTab => {
                if let Some(last) = self.tab_count().checked_sub(1) {
                    self.select_index(last);
                }
            }
            Command::Bookmark => self.star_clicked(),
            Command::Find => {
                if let (Some(tab), Some(browser)) = (active, self.browser()) {
                    match browser.engine().find_options("") {
                        Ok(options) => exec::spawn(async move {
                            if let Err(e) = tab.find(options).await {
                                log::warn!("find: {e}");
                            }
                        }),
                        Err(e) => log::warn!("find: {e}"),
                    }
                }
            }
            Command::ToggleBookmarksBar => {
                if let Some(browser) = self.browser() {
                    browser.set_bookmarks_bar_visible(!browser.bookmarks_bar_visible());
                }
            }
            Command::ShowBookmarks => self.show_dialog(Dialog::Bookmarks),
            Command::ShowHistory => self.show_dialog(Dialog::History),
        }
    }

    pub fn show_dialog(&self, dialog: Dialog) {
        if self.dialog_open.replace(true) {
            return;
        }
        let me = self.me();
        exec::spawn(async move {
            if let Err(e) = dialogs::show(&me, dialog).await {
                log::error!("{dialog:?} dialog: {e}");
            }
            me.dialog_open.set(false);
        });
    }

    /// Programmatic focus in an inactive window would activate it, which scripted runs and
    /// background events must never do.
    fn is_foreground(&self) -> bool {
        platform::window_handle(&self.window)
            .is_ok_and(|hwnd| unsafe { GetForegroundWindow() } == hwnd)
    }

    fn focus_address(&self) {
        if !self.is_foreground() {
            return;
        }
        let address = &self.ui.address;
        let _ = address
            .cast::<UIElement>()
            .and_then(|a| a.Focus(FocusState::Programmatic));
        if let Ok(root) = address.cast::<DependencyObject>()
            && let Some(text_box) = xaml::find_descendant::<TextBox>(&root)
        {
            let _ = text_box.SelectAll();
        }
    }

    fn star_clicked(&self) {
        let (Some(tab), Some(browser)) = (self.active_tab(), self.browser()) else {
            return;
        };
        let state = tab.state();
        if state.url.is_empty() {
            self.show_star(false);
            return;
        }
        let starred = browser.star_clicked(&state.url, &state.title);
        tab.set_starred(starred);
        self.show_star(starred);
    }

    /// Enter in the address box.
    pub fn address_submitted(&self, text: &str) {
        self.address_edited.set(false);
        let Some(browser) = self.browser() else {
            return;
        };
        let Some(url) = browser.omnibox_submitted(text) else {
            return;
        };
        match self.active_tab() {
            Some(tab) => {
                tab.navigate(&url);
                if self.is_foreground() {
                    tab.focus_page();
                }
            }
            None => {
                if let Err(e) = self.open_url_tab(&url, true) {
                    log::error!("open {url}: {e}");
                }
            }
        }
        self.refresh_chrome();
    }

    fn address_edited_by_user(&self) {
        self.address_edited.set(true);
        let Some(browser) = self.browser() else {
            return;
        };
        let text = self.address_text();
        let suggestions = browser.omnibox_text_changed(&text);
        let items: Vec<Option<IInspectable>> =
            suggestions.iter().map(|s| xaml::boxed(s).ok()).collect();
        let source = windows_collections::IVector::<IInspectable>::from(items);
        let _ = self
            .ui
            .address
            .cast::<ItemsControl>()
            .and_then(|list| list.SetItemsSource(&source));
    }

    // ---- bars ----

    pub fn set_bookmarks_bar_visible(&self, visible: bool) {
        self.bookmarks_bar_wanted.set(visible);
        let _ = xaml::set_visible(&self.ui.bookmarks_bar, visible && !self.fullscreen.get());
    }

    /// Replaces the bookmarks bar's buttons.
    pub fn set_bookmarks_bar(&self, items: &[BarItem]) {
        let Ok(children) = self.ui.bookmark_items.Children() else {
            return;
        };
        let _ = children.Clear();
        let window = self.me.clone();
        let open: OpenLink =
            Rc::new(move |url, disposition| with(&window, |w| w.open_link(url, disposition)));
        for item in items {
            match bookmarks_bar::button(item, &open) {
                Ok(button) => {
                    let _ = children.Append(&button);
                }
                Err(e) => log::warn!("bookmarks bar item: {e}"),
            }
        }
        let _ = xaml::set_visible(&self.ui.bookmarks_hint, items.is_empty());
        *self.bar_items.borrow_mut() = items.to_vec();
    }

    pub fn bookmarks_bar_items(&self) -> Vec<BarItem> {
        self.bar_items.borrow().clone()
    }

    fn open_link(&self, url: &str, disposition: Disposition) {
        let result = match (disposition, self.active_tab()) {
            (Disposition::CurrentTab, Some(tab)) => {
                tab.navigate(url);
                Ok(())
            }
            (Disposition::CurrentTab, None) => self.open_url_tab(url, true).map(drop),
            (Disposition::BackgroundTab, _) => self.open_url_tab(url, false).map(drop),
        };
        if let Err(e) = result {
            log::error!("open {url}: {e}");
        }
    }

    /// Shows `banner` in the update bar, or hides the bar. A bar the user closed opens again
    /// only for news (a new title), not for progress.
    pub fn show_update(&self, banner: Option<&Banner>) {
        let news =
            self.update_banner.borrow().as_ref().map(|b| &b.title) != banner.map(|b| &b.title);
        *self.update_banner.borrow_mut() = banner.cloned();
        let bar = &self.ui.update_bar;
        let Some(banner) = banner else {
            let _ = bar.SetIsOpen(false);
            return;
        };
        let severity = match banner.severity {
            Severity::Informational => InfoBarSeverity::Informational,
            Severity::Error => InfoBarSeverity::Error,
        };
        let _ = bar.SetSeverity(severity);
        let _ = bar.SetTitle(&banner.title);
        let _ = bar.SetMessage(&banner.message);
        let action = &self.ui.update_action;
        if let Some(label) = banner.action.map(Action::label) {
            let _ = xaml::boxed(label)
                .and_then(|label| action.cast::<IContentControl>()?.SetContent(&label));
        }
        let _ = xaml::set_visible(action, banner.action.is_some());
        if news {
            let _ = bar.SetIsOpen(true);
        }
    }

    fn update_clicked(&self) {
        let action = self.update_banner.borrow().as_ref().and_then(|b| b.action);
        if let (Some(action), Some(browser)) = (action, self.browser()) {
            browser.update_action(action);
        }
    }

    /// Replaces the extension action buttons in the toolbar.
    pub fn set_extension_actions(&self, actions: &[ExtensionAction]) {
        let Ok(children) = self.ui.extension_actions.Children() else {
            return;
        };
        let _ = children.Clear();
        let Some(browser) = self.browser() else {
            return;
        };
        for action in actions {
            match popup::action_button(action, browser.engine().environment().clone()) {
                Ok(button) => {
                    let _ = button.cast::<UIElement>().and_then(|b| children.Append(&b));
                }
                Err(e) => log::warn!("extension action {}: {e}", action.extension_id),
            }
        }
    }

    /// Opens the popup of the `index`-th extension action, as a click would.
    pub fn open_extension_popup(&self, index: u32, activation: Activation) -> Result<()> {
        let button = self.ui.extension_actions.Children()?.GetAt(index)?;
        let browser = self.browser().ok_or_else(windows_core::Error::empty)?;
        let action = browser
            .extension_actions()
            .into_iter()
            .nth(index as usize)
            .ok_or_else(windows_core::Error::empty)?;
        popup::open(
            &button.cast()?,
            browser.engine().environment().clone(),
            &action,
            activation,
        )
    }

    fn set_fullscreen(&self, on: bool) {
        if self.fullscreen.replace(on) == on {
            return;
        }
        let chrome_visible = !on;
        let _ = xaml::set_visible(&self.ui.tab_view, chrome_visible);
        let _ = xaml::set_visible(&self.ui.toolbar, chrome_visible);
        let _ = xaml::set_visible(
            &self.ui.bookmarks_bar,
            chrome_visible && self.bookmarks_bar_wanted.get(),
        );
        let _ = xaml::set_visible(&self.ui.update_bar, chrome_visible);
        let kind = if on {
            AppWindowPresenterKind::FullScreen
        } else {
            AppWindowPresenterKind::Overlapped
        };
        let presenter = self
            .window
            .cast::<IWindow2>()
            .and_then(|w| w.AppWindow())
            .and_then(|w| w.SetPresenterByKind(kind));
        if let Err(e) = presenter {
            log::warn!("fullscreen: {e}");
        }
    }

    // ---- engine access and capture ----

    /// The WebView2 profile, once any tab's engine view exists.
    pub async fn engine_profile(&self) -> Option<CoreWebView2Profile> {
        let ready = exec::wait_for(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(50),
            || self.tabs.borrow().iter().find_map(|t| t.core().cloned()),
        )
        .await?;
        ready
            .cast::<ICoreWebView2_13>()
            .and_then(|c| c.Profile())
            .ok()
    }

    /// The whole window, captured without activating it.
    pub async fn capture(&self) -> Result<capture::WindowShot> {
        capture::window_png(platform::window_handle(&self.window)?).await
    }

    // ---- wiring ----

    fn wire(&self) -> Result<()> {
        let me = || self.me.clone();
        let ui = &self.ui;

        let w = me();
        ui.tab_view
            .AddTabButtonClick(move |_, _| {
                if let Some(w) = w.upgrade() {
                    w.run(Command::NewTab);
                }
            })?
            .forget();
        let w = me();
        ui.tab_view
            .TabCloseRequested(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                let Ok(item) = args.Tab() else { return };
                let id = w
                    .tabs
                    .borrow()
                    .iter()
                    .find(|t| xaml::same_object(t.item(), &item))
                    .map(|t| t.id);
                if let Some(id) = id {
                    w.close_tab(id);
                }
            })?
            .forget();
        let w = me();
        ui.tab_view
            .SelectionChanged(move |_, _| {
                if let Some(w) = w.upgrade() {
                    w.sync_selection();
                }
            })?
            .forget();

        let w = me();
        click(&ui.back, move || with(&w, |w| w.run(Command::Back)))?;
        let w = me();
        click(&ui.forward, move || with(&w, |w| w.run(Command::Forward)))?;
        let w = me();
        click(&ui.reload, move || {
            with(&w, |w| {
                if let Some(tab) = w.active_tab() {
                    tab.reload_or_stop();
                }
            });
        })?;
        let w = me();
        click(&ui.star, move || with(&w, |w| w.star_clicked()))?;
        let w = me();
        click(&ui.update_action, move || with(&w, |w| w.update_clicked()))?;

        let w = me();
        ui.address
            .TextChanged(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                if args
                    .Reason()
                    .is_ok_and(|r| r == AutoSuggestionBoxTextChangeReason::UserInput)
                {
                    w.address_edited_by_user();
                }
            })?
            .forget();
        let w = me();
        ui.address
            .QuerySubmitted(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                let chosen = args
                    .ChosenSuggestion()
                    .ok()
                    .and_then(|c| {
                        c.cast::<windows_reference::IReference<windows_core::HSTRING>>()
                            .ok()
                    })
                    .and_then(|c| c.Value().ok())
                    .map(|c| c.to_string_lossy());
                let text = chosen.unwrap_or_else(|| args.QueryText().unwrap_or_default());
                w.address_submitted(&text);
            })?
            .forget();
        let w = me();
        ui.address
            .cast::<UIElement>()?
            .GotFocus(move |_, _| {
                let Some(w) = w.upgrade() else { return };
                if let Ok(root) = w.ui.address.cast::<DependencyObject>()
                    && let Some(text_box) = xaml::find_descendant::<TextBox>(&root)
                {
                    let _ = text_box.SelectAll();
                }
            })?
            .forget();

        let menu = [
            ("MenuNewTab", MenuAction::Run(Command::NewTab)),
            ("MenuNewWindow", MenuAction::Run(Command::NewWindow)),
            ("MenuBookmarks", MenuAction::Show(Dialog::Bookmarks)),
            ("MenuHistory", MenuAction::Show(Dialog::History)),
            ("MenuExtensions", MenuAction::Show(Dialog::Extensions)),
            ("MenuSettings", MenuAction::Show(Dialog::Settings)),
            ("MenuAbout", MenuAction::Show(Dialog::About)),
        ];
        for (name, action) in menu {
            let item: MenuFlyoutItem = xaml::find(&ui.root, name)?;
            let w = me();
            item.Click(move |_, _| {
                if let Some(w) = w.upgrade() {
                    match action {
                        MenuAction::Run(command) => w.run(command),
                        MenuAction::Show(dialog) => w.show_dialog(dialog),
                    }
                }
            })?
            .forget();
        }

        let w = me();
        self.window
            .Closed(move |_, _| {
                if let Some(w) = w.upgrade() {
                    w.on_closed();
                }
            })?
            .forget();
        Ok(())
    }

    fn install_accelerators(&self) -> Result<()> {
        let root = self.ui.root.cast::<UIElement>()?;
        root.SetKeyboardAcceleratorPlacementMode(KeyboardAcceleratorPlacementMode::Hidden)?;
        let accelerators = root.KeyboardAccelerators()?;
        for binding in BINDINGS {
            let accelerator = KeyboardAccelerator::new()?;
            accelerator.SetKey(VirtualKey(i32::from(binding.vk)))?;
            accelerator.SetModifiers(virtual_key_modifiers(binding.mods))?;
            let w = self.me.clone();
            let command = binding.command;
            accelerator
                .Invoked(move |_, args| {
                    if let Some(args) = args.as_ref() {
                        let _ = args.SetHandled(true);
                    }
                    if let Some(w) = w.upgrade() {
                        w.run(command);
                    }
                })?
                .forget();
            accelerators.Append(&accelerator)?;
        }
        Ok(())
    }

    fn on_closed(&self) {
        if self.closed.replace(true) {
            return;
        }
        for tab in self.tabs.take() {
            tab.close();
        }
        if let Some(browser) = self.browser() {
            browser.window_closed(self);
        }
    }
}

fn virtual_key_modifiers(mods: Mods) -> VirtualKeyModifiers {
    let mut bits = 0;
    if mods.bits() & Mods::CTRL.bits() != 0 {
        bits |= VirtualKeyModifiers::Control.0;
    }
    if mods.bits() & Mods::SHIFT.bits() != 0 {
        bits |= VirtualKeyModifiers::Shift.0;
    }
    if mods.bits() & Mods::ALT.bits() != 0 {
        bits |= VirtualKeyModifiers::Menu.0;
    }
    VirtualKeyModifiers(bits)
}

fn with(window: &Weak<BrowserWindow>, f: impl FnOnce(&BrowserWindow)) {
    if let Some(window) = window.upgrade() {
        f(&window);
    }
}

fn click(button: &impl Interface, handler: impl Fn() + 'static) -> Result<()> {
    button
        .cast::<ButtonBase>()?
        .Click(move |_, _| handler())?
        .forget();
    Ok(())
}
