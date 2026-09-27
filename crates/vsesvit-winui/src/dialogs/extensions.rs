//! Extensions: install from a Chrome Web Store or addons.mozilla.org link or id, a `.crx` /
//! `.xpi` file or an unpacked folder, with progress; and the installed list with each
//! extension's version, provenance, an on/off switch and a remove button.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use vsesvit_core::extensions::{InstallSource, InstalledExtension};
use windows_core::{Interface, Result};

use super::{Wired, on_click};
use crate::bindings::*;
use crate::browser::Browser;
use crate::extensions::{Progress, verification_label};
use crate::popup::{best_icon, icon_markup};
use crate::window::BrowserWindow;
use crate::{exec, pickers, xaml};

pub(super) const MARKUP: &str = r#"
  <StackPanel Width="640" Spacing="12">
    <TextBlock Text="Add an extension" Style="{StaticResource BodyStrongTextBlockStyle}"/>
    <Grid ColumnSpacing="8">
      <Grid.ColumnDefinitions>
        <ColumnDefinition/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
      </Grid.ColumnDefinitions>
      <TextBox x:Name="InstallSource"
               PlaceholderText="Chrome Web Store or addons.mozilla.org link, extension ID, or file path"/>
      <Button x:Name="InstallButton" Grid.Column="1" Content="Install" Style="{StaticResource AccentButtonStyle}"/>
      <Button x:Name="InstallFile" Grid.Column="2" Content="File..."
              ToolTipService.ToolTip="Install a .crx or .xpi file"/>
      <Button x:Name="InstallFolder" Grid.Column="3" Content="Folder..."
              ToolTipService.ToolTip="Load an unpacked extension folder"/>
    </Grid>
    <ProgressBar x:Name="InstallProgress" Visibility="Collapsed"/>
    <TextBlock x:Name="InstallStatus" TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    <TextBlock Text="Installed" Style="{StaticResource BodyStrongTextBlockStyle}"/>
    <ListView x:Name="ExtensionsList" MaxHeight="340" SelectionMode="None"/>
    <TextBlock x:Name="ExtensionsEmpty" Text="No extensions installed"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
  </StackPanel>"#;

struct Manager {
    browser: Weak<Browser>,
    window: Weak<BrowserWindow>,
    source: TextBox,
    install_buttons: Vec<Control>,
    progress: ProgressBar,
    status: TextBlock,
    list: ListView,
    empty: UIElement,
    busy: Cell<bool>,
}

pub(super) fn wire(
    root: &FrameworkElement,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
) -> Result<Wired> {
    let manager = Rc::new(Manager {
        browser: Rc::downgrade(browser),
        window: Rc::downgrade(window),
        source: xaml::find(root, "InstallSource")?,
        install_buttons: vec![
            xaml::find(root, "InstallButton")?,
            xaml::find(root, "InstallFile")?,
            xaml::find(root, "InstallFolder")?,
        ],
        progress: xaml::find(root, "InstallProgress")?,
        status: xaml::find(root, "InstallStatus")?,
        list: xaml::find(root, "ExtensionsList")?,
        empty: xaml::find(root, "ExtensionsEmpty")?,
        busy: Cell::new(false),
    });
    manager.render();

    let m = Rc::downgrade(&manager);
    on_click(&manager.install_buttons[0], move || {
        let Some(m) = m.upgrade() else { return };
        let text = m.source.Text().unwrap_or_default();
        match InstallSource::parse(&text) {
            Ok(source) => m.install(source),
            Err(e) => {
                let _ = m.status.SetText(&e.to_string());
            }
        }
    })?;
    let m = Rc::downgrade(&manager);
    on_click(&manager.install_buttons[1], move || {
        if let Some(m) = m.upgrade() {
            m.pick(false);
        }
    })?;
    let m = Rc::downgrade(&manager);
    on_click(&manager.install_buttons[2], move || {
        if let Some(m) = m.upgrade() {
            m.pick(true);
        }
    })?;

    let m = Rc::downgrade(&manager);
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(m) = m.upgrade() {
            m.render();
        }
    });
    browser.extensions.subscribe(&refresh);
    Ok(Wired {
        _alive: vec![manager, Rc::new(refresh)],
        on_close: None,
    })
}

impl Manager {
    /// Installs `source`: download and verification on a worker thread, then commit and engine
    /// load on the UI thread, with progress shown meanwhile.
    fn install(self: &Rc<Self>, source: InstallSource) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        if self.busy.replace(true) {
            return;
        }
        self.set_installing(true);
        let _ = self.status.SetText("Starting");
        let me = self.clone();
        exec::spawn(async move {
            let shown = me.clone();
            let progress = move |p: Progress| shown.show_progress(&p);
            let result = browser.install_extension(source, &progress).await;
            let text = match result {
                Ok(ext) => match browser.extensions.engine_error(&ext.id) {
                    Some(e) => format!(
                        "Installed {} {}, but WebView2 did not load it: {e}",
                        ext.manifest.name, ext.version
                    ),
                    None => format!("Installed {} {}.", ext.manifest.name, ext.version),
                },
                Err(e) => format!("Not installed: {e}"),
            };
            me.set_installing(false);
            let _ = me.status.SetText(&text);
            me.busy.set(false);
            me.render();
        });
    }

    fn pick(self: &Rc<Self>, folder: bool) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let owner = match window.window_id() {
            Ok(id) => id,
            Err(e) => {
                log::warn!("picker owner: {e}");
                return;
            }
        };
        let me = self.clone();
        exec::spawn(async move {
            let picked = if folder {
                pickers::pick_folder(owner).await
            } else {
                pickers::pick_file(owner, &[".crx", ".xpi"]).await
            };
            match picked {
                Ok(Some(path)) => match InstallSource::from_path(&path) {
                    Ok(source) => {
                        let _ = me.source.SetText(&path.to_string_lossy());
                        me.install(source);
                    }
                    Err(e) => {
                        let _ = me.status.SetText(&format!("{}: {e}", path.display()));
                    }
                },
                Ok(None) => {}
                Err(e) => {
                    let _ = me
                        .status
                        .SetText(&format!("Could not open the picker: {e}"));
                }
            }
        });
    }

    fn set_installing(&self, installing: bool) {
        for button in &self.install_buttons {
            let _ = button.SetIsEnabled(!installing);
        }
        let _ = xaml::set_visible(&self.progress, installing);
        let _ = self.progress.SetIsIndeterminate(true);
    }

    fn show_progress(&self, progress: &Progress) {
        let _ = self.status.SetText(&progress.text);
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

    fn render(self: &Rc<Self>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let installed = browser.core(|p| p.extensions().list()).unwrap_or_else(|e| {
            log::warn!("listing extensions: {e}");
            Vec::new()
        });
        let Ok(items) = self.list.cast::<ItemsControl>().and_then(|l| l.Items()) else {
            return;
        };
        let _ = items.Clear();
        for ext in &installed {
            let error = browser.extensions.engine_error(&ext.id);
            match self.row(ext, error.as_deref()) {
                Ok(row) => {
                    let _ = items.Append(&row);
                }
                Err(e) => log::warn!("extension row: {e}"),
            }
        }
        let _ = xaml::set_visible(&self.empty, installed.is_empty());
    }

    fn row(
        self: &Rc<Self>,
        ext: &InstalledExtension,
        engine_error: Option<&str>,
    ) -> Result<UIElement> {
        let icon = best_icon(&ext.manifest.icons)
            .or_else(|| {
                ext.manifest
                    .action
                    .as_ref()
                    .and_then(|a| best_icon(&a.default_icon))
            })
            .map(|i| i.resolve(&ext.dir));
        let details = format!(
            "{} \u{00B7} {} \u{00B7} {}",
            ext.version,
            verification_label(&ext.verification),
            ext.id.as_str()
        );
        let error =
            engine_error.map_or(String::new(), |e| format!("WebView2 did not load it: {e}"));
        let root: FrameworkElement = xaml::load(&format!(
            r#"<Grid {{ns}} ColumnSpacing="12" Padding="0,6">
                 <Grid.ColumnDefinitions>
                   <ColumnDefinition Width="32"/><ColumnDefinition Width="*"/>
                   <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
                 </Grid.ColumnDefinitions>
                 <Grid VerticalAlignment="Center">{icon}</Grid>
                 <StackPanel Grid.Column="1" VerticalAlignment="Center">
                   <TextBlock Text="{name}" Style="{{StaticResource BodyStrongTextBlockStyle}}" TextTrimming="CharacterEllipsis"/>
                   <TextBlock Text="{details}" Style="{{StaticResource CaptionTextBlockStyle}}"
                              Foreground="{{ThemeResource TextFillColorSecondaryBrush}}" TextTrimming="CharacterEllipsis"/>
                   <TextBlock Text="{error}" Style="{{StaticResource CaptionTextBlockStyle}}" TextWrapping="Wrap"
                              Foreground="{{ThemeResource SystemFillColorCriticalBrush}}" Visibility="{error_visibility}"/>
                 </StackPanel>
                 <ToggleSwitch x:Name="Enabled" Grid.Column="2" OnContent="On" OffContent="Off" MinWidth="0"
                               AutomationProperties.Name="Enabled"/>
                 <Button x:Name="Remove" Grid.Column="3" Content="Remove"/>
               </Grid>"#,
            icon = icon_markup(icon.as_deref(), 32),
            name = xaml::escape(&ext.manifest.name),
            details = xaml::escape(&details),
            error = xaml::escape(&error),
            error_visibility = if error.is_empty() {
                "Collapsed"
            } else {
                "Visible"
            },
        ))?;
        let toggle: ToggleSwitch = xaml::find(&root, "Enabled")?;
        toggle.SetIsOn(ext.enabled)?;
        let id = ext.id.clone();
        let me = Rc::downgrade(self);
        let source = toggle.clone();
        toggle
            .Toggled(move |_, _| {
                let (Some(me), Ok(on)) = (me.upgrade(), source.IsOn()) else {
                    return;
                };
                let Some(browser) = me.browser.upgrade() else {
                    return;
                };
                let id = id.clone();
                exec::spawn(async move {
                    if let Err(e) = browser.set_extension_enabled(&id, on).await {
                        let _ = me
                            .status
                            .SetText(&format!("Could not change {}: {e}", id.as_str()));
                    }
                });
            })?
            .forget();
        let remove: Button = xaml::find(&root, "Remove")?;
        let id = ext.id.clone();
        let name = ext.manifest.name.clone();
        let me = Rc::downgrade(self);
        on_click(&remove, move || {
            let Some(me) = me.upgrade() else { return };
            let Some(browser) = me.browser.upgrade() else {
                return;
            };
            let (id, name) = (id.clone(), name.clone());
            exec::spawn(async move {
                let text = match browser.uninstall_extension(&id).await {
                    Ok(()) => format!("Removed {name}."),
                    Err(e) => format!("Could not remove {name}: {e}"),
                };
                let _ = me.status.SetText(&text);
                me.render();
            });
        })?;
        root.cast()
    }
}
