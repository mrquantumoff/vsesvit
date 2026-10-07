//! Profiles in the Windows shell: the avatar every list of profiles shows, the flyout that names
//! and colours a profile, Manage profiles, the picker a launch shows before any profile opens,
//! and starting another profile's process. Core's `profiles` keeps the list.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};

use vsesvit_core::Url;
use vsesvit_core::profiles::{
    self, NAME_MAX, ProfileColor, ProfileEntry, ProfileId, ProfilesDir, Registry,
};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::{Wired, on_click};
use crate::window::{self, Backdrop, BrowserWindow};
use crate::{app, platform, xaml};

/// The profile's sync account picture in a circle, else a coloured circle with its initial.
pub(crate) fn avatar_markup(
    name: &str,
    color: ProfileColor,
    picture: Option<&Path>,
    size: u32,
) -> String {
    let radius = size / 2;
    if let Some(uri) = picture.and_then(|p| Url::from_file_path(p).ok()) {
        return format!(
            r#"<Border x:Name="ProfilePicture" Width="{size}" Height="{size}" CornerRadius="{radius}">
  <Border.Background><ImageBrush ImageSource="{uri}" Stretch="UniformToFill"/></Border.Background>
</Border>"#,
            uri = xaml::escape(uri.as_str()),
        );
    }
    format!(
        r#"<Border Width="{size}" Height="{size}" CornerRadius="{radius}" Background="{css}">
  <TextBlock Text="{initial}" Foreground="White" FontWeight="SemiBold" FontSize="{font}"
             HorizontalAlignment="Center" VerticalAlignment="Center"/>
</Border>"#,
        css = color.css(),
        initial = xaml::escape(&profiles::avatar_initial(name)),
        font = size * 11 / 20,
    )
}

/// [`avatar_markup`] for a listed profile.
pub(crate) fn entry_avatar_markup(dir: &ProfilesDir, entry: &ProfileEntry, size: u32) -> String {
    avatar_markup(
        &entry.name,
        entry.color,
        dir.picture(entry).as_deref(),
        size,
    )
}

pub(crate) fn avatar(
    name: &str,
    color: ProfileColor,
    picture: Option<&Path>,
    size: u32,
) -> Result<UIElement> {
    xaml::load(&avatar_markup(name, color, picture, size).replacen("<Border", "<Border {ns}", 1))
}

/// Starts `root`'s profile, or brings its windows forward when its process already runs: the
/// new process finds the profile locked and forwards its (empty) command line, and with it the
/// right to take the foreground this process has from the user's click.
pub(crate) fn launch(root: &Path) -> std::io::Result<()> {
    let child = std::process::Command::new(std::env::current_exe()?)
        .arg("--profile-dir")
        .arg(root)
        .spawn()?;
    let _ = unsafe { AllowSetForegroundWindow(child.id()) };
    Ok(())
}

/// Has the process running `root`'s profile look at the profile list again, through the
/// command line a second process on the profile forwards to it.
pub(crate) fn notify(root: &Path) {
    let spawned = std::env::current_exe().and_then(|program| {
        std::process::Command::new(program)
            .arg("--profile-dir")
            .arg(root)
            .spawn()
    });
    if let Err(e) = spawned {
        log::warn!(
            "telling the profile at {} to look at the profile list: {e}",
            root.display()
        );
    }
}

/// Asks for a profile's name and colour in a flyout under `anchor`; `done` gets them when the
/// user accepts.
pub(crate) fn ask(
    anchor: &FrameworkElement,
    heading: &str,
    accept: &str,
    name: &str,
    color: ProfileColor,
    done: impl Fn(String, ProfileColor) + 'static,
) -> Result<Flyout> {
    let swatch = |(i, choice): (usize, &ProfileColor)| {
        format!(
            r#"<ToggleButton x:Name="ProfileColor{i}" Width="36" Height="36" Padding="0" CornerRadius="18"
                ToolTipService.ToolTip="{label}" AutomationProperties.Name="{label}">
  <Border Width="24" Height="24" CornerRadius="12" Background="{css}"/>
</ToggleButton>"#,
            label = choice.label(),
            css = choice.css(),
        )
    };
    let swatches: Vec<String> = ProfileColor::ALL.iter().enumerate().map(swatch).collect();
    let flyout: Flyout = xaml::load(&format!(
        r#"<Flyout {{ns}} Placement="BottomEdgeAlignedRight">
  <StackPanel Spacing="12" Width="300">
    <TextBlock Text="{heading}" Style="{{StaticResource SubtitleTextBlockStyle}}"/>
    <TextBox x:Name="ProfileName" Header="Name" Text="{name}" MaxLength="{NAME_MAX}"/>
    <StackPanel Spacing="4">
      <StackPanel Orientation="Horizontal" Spacing="4">{first}</StackPanel>
      <StackPanel Orientation="Horizontal" Spacing="4">{rest}</StackPanel>
    </StackPanel>
    <Button x:Name="ProfileAccept" Content="{accept}" HorizontalAlignment="Right"
            Style="{{StaticResource AccentButtonStyle}}"/>
  </StackPanel>
</Flyout>"#,
        heading = xaml::escape(heading),
        name = xaml::escape(name),
        accept = xaml::escape(accept),
        first = swatches[..5].concat(),
        rest = swatches[5..].concat(),
    ))?;
    let content: FrameworkElement = flyout.Content()?.cast()?;
    let entry: TextBox = xaml::find(&content, "ProfileName")?;
    let accept: Button = xaml::find(&content, "ProfileAccept")?;
    accept
        .cast::<Control>()?
        .SetIsEnabled(!name.trim().is_empty())?;
    let accept_control = accept.cast::<Control>()?;
    entry
        .TextChanged(move |source, _| {
            let text = source
                .as_ref()
                .and_then(|s| s.cast::<TextBox>().ok())
                .and_then(|t| t.Text().ok())
                .unwrap_or_default();
            let _ = accept_control.SetIsEnabled(!text.to_string().trim().is_empty());
        })?
        .forget();
    let chosen = Rc::new(Cell::new(color));
    let toggles: Rc<Vec<IToggleButton>> = Rc::new(
        (0..ProfileColor::ALL.len())
            .map(|i| xaml::find::<IToggleButton>(&content, &format!("ProfileColor{i}")))
            .collect::<Result<_>>()?,
    );
    for (i, choice) in ProfileColor::ALL.into_iter().enumerate() {
        toggles[i].SetIsChecked(Some(choice == color))?;
        let (toggle, toggles, chosen) = (toggles[i].clone(), toggles.clone(), chosen.clone());
        on_click(&toggle, move || {
            chosen.set(choice);
            for (j, toggle) in toggles.iter().enumerate() {
                let _ = toggle.SetIsChecked(Some(i == j));
            }
        })?;
    }
    let shown = flyout.clone();
    on_click(&accept, move || {
        let name = entry.Text().map(|t| t.to_string()).unwrap_or_default();
        let _ = shown.cast::<FlyoutBase>().and_then(|f| f.Hide());
        done(name.trim().to_owned(), chosen.get());
    })?;
    flyout.cast::<FlyoutBase>()?.ShowAt(anchor)?;
    Ok(flyout)
}

/// Asks to remove a profile, in a flyout under `anchor`.
fn ask_remove(anchor: &FrameworkElement, name: &str, done: impl Fn() + 'static) -> Result<()> {
    let flyout: Flyout = xaml::load(&format!(
        r#"<Flyout {{ns}} Placement="Bottom">
  <StackPanel Spacing="12" Width="320">
    <TextBlock Text="Remove {name}?" Style="{{StaticResource SubtitleTextBlockStyle}}" TextWrapping="Wrap"/>
    <TextBlock TextWrapping="Wrap" Text="Its bookmarks, history, settings, extensions and sync sign-in are deleted from this device, and its windows close. What it synced stays on the sync server."/>
    <Button x:Name="ProfileRemove" Content="Remove" HorizontalAlignment="Right"
            Style="{{StaticResource AccentButtonStyle}}"/>
  </StackPanel>
</Flyout>"#,
        name = xaml::escape(name),
    ))?;
    let content: FrameworkElement = flyout.Content()?.cast()?;
    let shown = flyout.clone();
    on_click(
        &xaml::find::<Button>(&content, "ProfileRemove")?,
        move || {
            let _ = shown.cast::<FlyoutBase>().and_then(|f| f.Hide());
            done();
        },
    )?;
    flyout.cast::<FlyoutBase>()?.ShowAt(anchor)
}

/// Manage profiles, the dialog's content: every profile, to open, rename and recolour, or
/// remove, and Add profile.
pub(crate) const MANAGE_MARKUP: &str = r#"
  <StackPanel Spacing="12" Width="460">
    <TextBlock TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"
               Text="Each profile has its own bookmarks, history, settings, extensions and sync account, and opens in windows of its own."/>
    <StackPanel x:Name="ProfileRows" Spacing="4"/>
    <Button x:Name="AddProfile" Content="Add profile"/>
  </StackPanel>"#;

pub(crate) fn wire_manage(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Wired> {
    browser.profile_used();
    let manage = Rc::new_cyclic(|me: &Weak<Manage>| {
        let me = me.clone();
        Manage {
            browser: Rc::downgrade(browser),
            window: Rc::downgrade(window),
            rows: RefCell::new(None),
            watch: Rc::new(move || {
                if let Some(manage) = me.upgrade() {
                    manage.fill();
                }
            }),
        }
    });
    *manage.rows.borrow_mut() = Some(xaml::find(root, "ProfileRows")?);
    browser.watch_profiles(&manage.watch);
    let add: Button = xaml::find(root, "AddProfile")?;
    let (w, anchor) = (Rc::downgrade(window), add.cast::<FrameworkElement>()?);
    on_click(&add, move || {
        if let Some(window) = w.upgrade() {
            add_profile(&window, &anchor);
        }
    })?;
    manage.fill();
    Ok(Wired {
        _alive: vec![manage],
        on_close: None,
    })
}

/// "Add profile": names the new profile, then opens it in a window of its own, as Chrome does.
pub(crate) fn add_profile(window: &Rc<BrowserWindow>, anchor: &FrameworkElement) {
    let Some(browser) = window.browser() else {
        return;
    };
    let registry = browser.profiles();
    let w = Rc::downgrade(window);
    let shown = ask(
        anchor,
        "Add profile",
        "Add",
        &registry.next_name(),
        registry.next_color(),
        move |name, color| {
            if let Some(window) = w.upgrade()
                && let Some(browser) = window.browser()
                && let Err(e) = browser.add_profile(&name, color)
            {
                window.show_failure("Could not add the profile", &e);
            }
        },
    );
    if let Err(e) = shown {
        log::warn!("the add profile flyout: {e}");
    }
}

pub(crate) struct Manage {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    rows: RefCell<Option<Panel>>,
    /// Kept for [`Browser::watch_profiles`], which holds it weakly.
    watch: Rc<dyn Fn()>,
}

impl Manage {
    fn fill(self: &Rc<Self>) {
        if let Err(e) = self.try_fill() {
            log::warn!("Manage profiles: {e}");
        }
    }

    fn try_fill(self: &Rc<Self>) -> Result<()> {
        let (Some(browser), Some(rows)) = (self.browser.upgrade(), self.rows.borrow().clone())
        else {
            return Ok(());
        };
        let children = rows.Children()?;
        children.Clear()?;
        let registry = browser.profiles();
        let Some(home) = browser.home() else {
            return Ok(());
        };
        for (i, entry) in registry.profiles().iter().enumerate() {
            let this = entry.id == home.id;
            let row: FrameworkElement = xaml::load(&format!(
                r#"<Grid {{ns}} ColumnSpacing="12" Padding="12,8" CornerRadius="4"
       Background="{{ThemeResource CardBackgroundFillColorDefaultBrush}}">
  <Grid.ColumnDefinitions>
    <ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/>
    <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
  </Grid.ColumnDefinitions>
  {avatar}
  <StackPanel Grid.Column="1" VerticalAlignment="Center">
    <TextBlock x:Name="ProfileRowName{i}" Text="{name}" TextTrimming="CharacterEllipsis"/>
    <TextBlock Text="This profile" Visibility="{this}" Style="{{StaticResource CaptionTextBlockStyle}}"
               Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
  </StackPanel>
  <Button x:Name="ProfileOpen" Grid.Column="2" Visibility="{others}" Background="Transparent" BorderThickness="0"
          ToolTipService.ToolTip="Open" AutomationProperties.Name="Open {name}">
    <FontIcon Glyph="&#xE8A7;" FontSize="14"/>
  </Button>
  <Button x:Name="ProfileEdit" Grid.Column="3" Background="Transparent" BorderThickness="0"
          ToolTipService.ToolTip="Edit" AutomationProperties.Name="Edit {name}">
    <FontIcon Glyph="&#xE70F;" FontSize="14"/>
  </Button>
  <Button x:Name="ProfileRemove" Grid.Column="4" Background="Transparent" BorderThickness="0"
          IsEnabled="{removable}" ToolTipService.ToolTip="Remove" AutomationProperties.Name="Remove {name}">
    <FontIcon Glyph="&#xE74D;" FontSize="14"/>
  </Button>
</Grid>"#,
                avatar = entry_avatar_markup(&home.dir, entry, 32),
                name = xaml::escape(&entry.name),
                this = if this { "Visible" } else { "Collapsed" },
                others = if this { "Collapsed" } else { "Visible" },
                removable = registry.can_remove(&entry.id),
            ))?;
            self.wire_row(&row, &entry.id, &entry.name, entry.color)?;
            children.Append(&row.cast::<UIElement>()?)?;
        }
        Ok(())
    }

    fn wire_row(
        self: &Rc<Self>,
        row: &FrameworkElement,
        id: &ProfileId,
        name: &str,
        color: ProfileColor,
    ) -> Result<()> {
        let (me, open_id) = (Rc::downgrade(self), id.clone());
        on_click(&xaml::find::<Button>(row, "ProfileOpen")?, move || {
            if let Some(browser) = me.upgrade().and_then(|m| m.browser.upgrade()) {
                browser.open_profile(&open_id);
            }
        })?;

        let edit: FrameworkElement = xaml::find(row, "ProfileEdit")?;
        let (me, edit_id, anchor, old) = (
            Rc::downgrade(self),
            id.clone(),
            edit.clone(),
            name.to_owned(),
        );
        on_click(&edit, move || {
            let (me, id) = (me.clone(), edit_id.clone());
            let shown = ask(
                &anchor,
                "Edit profile",
                "Save",
                &old,
                color,
                move |name, color| {
                    let Some(manage) = me.upgrade() else { return };
                    let result = manage
                        .browser
                        .upgrade()
                        .map(|b| b.edit_profile(&id, &name, color));
                    if let (Some(Err(e)), Some(window)) = (result, manage.window.upgrade()) {
                        window.show_failure("Could not change the profile", &e);
                    }
                },
            );
            if let Err(e) = shown {
                log::warn!("the edit profile flyout: {e}");
            }
        })?;

        let remove: FrameworkElement = xaml::find(row, "ProfileRemove")?;
        let (me, remove_id, anchor, name) = (
            Rc::downgrade(self),
            id.clone(),
            remove.clone(),
            name.to_owned(),
        );
        on_click(&remove, move || {
            let (me, id) = (me.clone(), remove_id.clone());
            let shown = ask_remove(&anchor, &name, move || {
                let Some(manage) = me.upgrade() else { return };
                let result = manage.browser.upgrade().map(|b| b.remove_profile(&id));
                if let (Some(Err(e)), Some(window)) = (result, manage.window.upgrade()) {
                    window.show_failure("Could not remove the profile", &e);
                }
            });
            if let Err(e) = shown {
                log::warn!("the remove profile flyout: {e}");
            }
        })?;
        Ok(())
    }
}

/// The picker's markup: Chrome's "Who's using Chrome?".
const PICKER_XAML: &str = r#"<Grid {ns}>
  <Grid.RowDefinitions><RowDefinition Height="32"/><RowDefinition Height="*"/></Grid.RowDefinitions>
  <Grid x:Name="PickerTitleBar" Background="Transparent"/>
  <ScrollViewer Grid.Row="1" VerticalScrollBarVisibility="Auto">
    <StackPanel Spacing="28" Padding="24" VerticalAlignment="Center" HorizontalAlignment="Center">
      <TextBlock Text="Who's using Vsesvit?" HorizontalAlignment="Center"
                 Style="{StaticResource TitleTextBlockStyle}"/>
      <StackPanel x:Name="PickerCards" Orientation="Horizontal" Spacing="12" HorizontalAlignment="Center"/>
      <CheckBox x:Name="PickerShow" Content="Show on startup" HorizontalAlignment="Center"/>
    </StackPanel>
  </ScrollViewer>
</Grid>"#;

thread_local! {
    /// The picker's window, while it is open.
    static PICKER: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// Shows the picker a launch that names no profile opens when there are several and none
/// runs. Choosing one starts it and ends this process, which never opens a profile.
pub(crate) fn show_picker(dir: ProfilesDir) -> Result<()> {
    let registry = dir.load();
    let root: FrameworkElement = xaml::load(PICKER_XAML)?;
    let window = Window::new()?;
    window.SetTitle("Vsesvit")?;
    window.SetContent(&root)?;
    window.SetExtendsContentIntoTitleBar(true)?;
    window.SetTitleBar(&xaml::find::<UIElement>(&root, "PickerTitleBar")?)?;
    let hwnd = platform::window_handle(&window)?;
    platform::set_window_icon(hwnd);
    window::set_backdrop(&window, Backdrop::Mica);

    let cards = xaml::find::<Panel>(&root, "PickerCards")?.Children()?;
    for entry in registry.profiles() {
        let card = card(&entry_avatar_markup(&dir, entry, 72), &entry.name)?;
        let (dir, id) = (dir.clone(), entry.id.clone());
        on_click(&card, move || pick(&dir.root(&id)))?;
        cards.Append(&card.cast::<UIElement>()?)?;
    }
    let plus = r#"<Border Width="72" Height="72" CornerRadius="36"
        Background="{ThemeResource SubtleFillColorSecondaryBrush}">
  <FontIcon Glyph="&#xE710;" FontSize="28"/>
</Border>"#;
    let add = card(plus, "Add")?;
    let (picker_dir, anchor) = (dir.clone(), add.cast::<FrameworkElement>()?);
    on_click(&add, move || {
        let registry = picker_dir.load();
        let dir = picker_dir.clone();
        let shown = ask(
            &anchor,
            "Add profile",
            "Add",
            &registry.next_name(),
            registry.next_color(),
            move |name, color| match dir.add(&name, color) {
                Ok((id, _)) => pick(&dir.root(&id)),
                Err(e) => log::warn!("adding a profile: {e}"),
            },
        );
        if let Err(e) = shown {
            log::warn!("the add profile flyout: {e}");
        }
    })?;
    cards.Append(&add.cast::<UIElement>()?)?;

    let toggle: IToggleButton = xaml::find(&root, "PickerShow")?;
    toggle.SetIsChecked(Some(registry.show_picker()))?;
    on_click(&toggle.clone(), move || {
        let on = toggle.IsChecked().unwrap_or(false);
        if let Err(e) = dir.set_show_picker(on) {
            log::warn!("the profile list: {e}");
        }
    })?;

    let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.0;
    let pixels = |view: f64| (view * scale) as i32;
    window.cast::<IWindow2>()?.AppWindow()?.Resize(SizeInt32 {
        width: pixels(780.0),
        height: pixels(520.0),
    })?;
    window
        .Closed(|_, _| {
            PICKER.with_borrow_mut(Option::take);
            app::exit(0);
        })?
        .forget();
    window.Activate()?;
    PICKER.with_borrow_mut(|p| *p = Some(window));
    Ok(())
}

fn card(picture: &str, name: &str) -> Result<Button> {
    xaml::load(&format!(
        r#"<Button {{ns}} Background="Transparent" BorderThickness="0" Padding="12" Width="132"
        ToolTipService.ToolTip="{name}" AutomationProperties.Name="{name}">
  <StackPanel Spacing="8">
    {picture}
    <TextBlock Text="{name}" HorizontalAlignment="Center" TextTrimming="CharacterEllipsis" MaxWidth="108"/>
  </StackPanel>
</Button>"#,
        name = xaml::escape(name),
    ))
}

fn pick(root: &Path) {
    if let Err(e) = launch(root) {
        log::error!("starting the profile at {}: {e}", root.display());
        platform::message_box(
            &format!("Vsesvit could not open the profile.\n\n{e}"),
            MB_OK | MB_ICONERROR,
        );
        return;
    }
    if let Some(window) = PICKER.with_borrow(Clone::clone) {
        let _ = window.Close();
    }
}

/// Fills the profile menu: every profile, the current one checked, then adding and managing
/// them, as in Chrome's profile menu.
pub(crate) fn fill_menu(
    menu: &MenuFlyout,
    registry: &Registry,
    current: &ProfileId,
    window: &Rc<BrowserWindow>,
    anchor: &FrameworkElement,
) -> Result<()> {
    let items = menu.Items()?;
    items.Clear()?;
    for entry in registry.profiles() {
        let item = ToggleMenuFlyoutItem::new()?;
        item.cast::<MenuFlyoutItem>()?.SetText(&entry.name)?;
        item.SetIsChecked(&entry.id == current)?;
        let (w, id) = (Rc::downgrade(window), entry.id.clone());
        item.cast::<MenuFlyoutItem>()?
            .Click(move |_, _| {
                if let Some(browser) = w.upgrade().and_then(|w| w.browser()) {
                    browser.open_profile(&id);
                }
            })?
            .forget();
        items.Append(&item.cast::<MenuFlyoutItemBase>()?)?;
    }
    items.Append(&MenuFlyoutSeparator::new()?.cast::<MenuFlyoutItemBase>()?)?;
    let add = MenuFlyoutItem::new()?;
    add.SetText("Add profile")?;
    let (w, anchor) = (Rc::downgrade(window), anchor.clone());
    add.Click(move |_, _| {
        if let Some(window) = w.upgrade() {
            add_profile(&window, &anchor);
        }
    })?
    .forget();
    items.Append(&add.cast::<MenuFlyoutItemBase>()?)?;
    let manage = MenuFlyoutItem::new()?;
    manage.SetText("Manage profiles")?;
    let w = Rc::downgrade(window);
    manage
        .Click(move |_, _| {
            if let Some(window) = w.upgrade() {
                window.show_dialog(crate::dialogs::Dialog::Profiles);
            }
        })?
        .forget();
    items.Append(&manage.cast::<MenuFlyoutItemBase>()?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_avatar_is_its_colour_and_initial() {
        let markup = avatar_markup("<work>", ProfileColor::Teal, None, 24);
        assert!(markup.contains(r##"Background="#2190a4""##), "{markup}");
        assert!(markup.contains(r#"Text="W""#), "{markup}");
        assert!(markup.contains(r#"CornerRadius="12""#), "{markup}");
    }

    #[test]
    fn an_avatar_with_a_picture_is_the_picture_in_a_circle() {
        let markup = avatar_markup(
            "Work",
            ProfileColor::Teal,
            Some(Path::new(r"C:\Profiles\A & B\Account Picture 1.png")),
            72,
        );
        assert!(
            markup.contains(
                r#"ImageSource="file:///C:/Profiles/A%20&amp;%20B/Account%20Picture%201.png""#
            ),
            "{markup}"
        );
        assert!(markup.contains(r#"CornerRadius="36""#), "{markup}");
        assert!(!markup.contains("TextBlock"), "{markup}");
    }
}
