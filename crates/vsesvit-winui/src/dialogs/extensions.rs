//! Extensions: install from a Chrome Web Store, Edge Add-ons or addons.mozilla.org link or id, a `.crx` /
//! `.xpi` file or an unpacked folder, with progress and, before a store or package install goes
//! in, Chrome's install prompt in the page (a dialog cannot open over this one); Update, which
//! checks every store extension for a newer version now; and the installed list with each
//! extension's version, provenance, an on/off switch and a remove button, and Chrome's
//! Re-enable for an update that can do more than the user approved.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use vsesvit_core::extensions::manifest::Manifest;
use vsesvit_core::extensions::permissions::{self, INSTALL_LEAD, PermissionMessage, RE_ENABLE_LEAD};
use vsesvit_core::extensions::{InstallSource, InstalledExtension, StagedInstall};
use windows_core::{Interface, Result};

use super::{Wired, on_click};
use crate::bindings::*;
use crate::browser::Browser;
use crate::extensions::Progress;
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
               PlaceholderText="Chrome Web Store, Edge Add-ons or addons.mozilla.org link, extension ID, or file path"/>
      <Button x:Name="InstallButton" Grid.Column="1" Content="Install" Style="{StaticResource AccentButtonStyle}"/>
      <Button x:Name="InstallFile" Grid.Column="2" Content="File..."
              ToolTipService.ToolTip="Install a .crx or .xpi file"/>
      <Button x:Name="InstallFolder" Grid.Column="3" Content="Folder..."
              ToolTipService.ToolTip="Load an unpacked extension folder"/>
    </Grid>
    <ProgressBar x:Name="InstallProgress" Visibility="Collapsed"/>
    <StackPanel x:Name="InstallPrompt" Visibility="Collapsed" Spacing="8" Padding="16" CornerRadius="8"
                Background="{ThemeResource CardBackgroundFillColorDefaultBrush}"
                BorderBrush="{ThemeResource CardStrokeColorDefaultBrush}" BorderThickness="1">
      <TextBlock x:Name="PromptHeading" Style="{StaticResource BodyStrongTextBlockStyle}" TextWrapping="Wrap"/>
      <TextBlock x:Name="PromptText" TextWrapping="Wrap"/>
      <StackPanel Orientation="Horizontal" Spacing="8" HorizontalAlignment="Right">
        <Button x:Name="PromptCancel" Content="Cancel"/>
        <Button x:Name="PromptAdd" Content="Add extension" Style="{StaticResource AccentButtonStyle}"/>
      </StackPanel>
    </StackPanel>
    <TextBlock x:Name="InstallStatus" TextWrapping="Wrap" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
    <Grid>
      <TextBlock Text="Installed" Style="{StaticResource BodyStrongTextBlockStyle}" VerticalAlignment="Center"/>
      <Button x:Name="UpdateExtensions" Content="Update" HorizontalAlignment="Right"
              ToolTipService.ToolTip="Update every extension from its store now"/>
    </Grid>
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
    prompt: UIElement,
    prompt_heading: TextBlock,
    prompt_text: TextBlock,
    /// The verified install the prompt asks about, committed on Add, dropped on Cancel.
    pending: RefCell<Option<StagedInstall>>,
    update: Control,
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
        prompt: xaml::find(root, "InstallPrompt")?,
        prompt_heading: xaml::find(root, "PromptHeading")?,
        prompt_text: xaml::find(root, "PromptText")?,
        pending: RefCell::new(None),
        update: xaml::find(root, "UpdateExtensions")?,
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
    on_click(&manager.update, move || {
        if let Some(m) = m.upgrade() {
            m.update();
        }
    })?;

    let m = Rc::downgrade(&manager);
    on_click(&xaml::find::<Button>(root, "PromptAdd")?, move || {
        if let Some(m) = m.upgrade() {
            m.decide(true);
        }
    })?;
    let m = Rc::downgrade(&manager);
    on_click(&xaml::find::<Button>(root, "PromptCancel")?, move || {
        if let Some(m) = m.upgrade() {
            m.decide(false);
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
    /// Installs `source`: download and verification on a worker thread, then, once the user
    /// adds it where Chrome would ask, commit and engine load on the UI thread, with progress
    /// shown meanwhile.
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
            match browser.stage_extension(source, &progress).await {
                Ok(staged) if staged.needs_approval() => me.ask(staged),
                Ok(staged) => me.commit(staged).await,
                Err(e) => me.finish(&format!("Not installed: {e}")),
            }
        });
    }

    /// Shows Chrome's install prompt for `staged` in the page.
    fn ask(&self, staged: StagedInstall) {
        let manifest = staged.manifest();
        let _ = self.prompt_heading.SetText(&permissions::install_heading(&manifest.name));
        let text = prompt_text(manifest);
        let _ = self.prompt_text.SetText(&text);
        let _ = xaml::set_visible(&self.prompt_text, !text.is_empty());
        let _ = self.status.SetText("");
        let _ = xaml::set_visible(&self.progress, false);
        let _ = xaml::set_visible(&self.prompt, true);
        self.pending.replace(Some(staged));
    }

    /// The prompt's Add or Cancel.
    fn decide(self: &Rc<Self>, add: bool) {
        let Some(staged) = self.pending.take() else { return };
        let _ = xaml::set_visible(&self.prompt, false);
        if !add {
            self.finish("");
            return;
        }
        let _ = xaml::set_visible(&self.progress, true);
        let me = self.clone();
        exec::spawn(async move { me.commit(staged).await });
    }

    async fn commit(self: &Rc<Self>, staged: StagedInstall) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let text = match browser.commit_extension(staged).await {
            Ok(ext) => match browser.extensions.engine_error(&ext.id) {
                Some(e) => format!(
                    "Installed {} {}, but WebView2 did not load it: {e}",
                    ext.manifest.name, ext.version
                ),
                None => format!("Installed {} {}.", ext.manifest.name, ext.version),
            },
            Err(e) => format!("Not installed: {e}"),
        };
        self.finish(&text);
    }

    fn finish(self: &Rc<Self>, text: &str) {
        self.set_installing(false);
        let _ = self.status.SetText(text);
        self.busy.set(false);
        self.render();
    }

    /// Checks every store extension for a newer version, then says what changed.
    fn update(self: &Rc<Self>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        let _ = self.update.SetIsEnabled(false);
        let _ = self.status.SetText("Checking for updates\u{2026}");
        let me = self.clone();
        exec::spawn(async move {
            let text = match browser.update_extensions().await {
                Ok(report) => report.summary(),
                Err(e) => format!("Could not check for updates: {e}"),
            };
            let _ = me.status.SetText(&text);
            let _ = me.update.SetIsEnabled(true);
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
        let _ = super::set_progress(&self.progress, progress.fraction);
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
            ext.verification.label(),
            ext.id.as_str()
        );
        let error =
            engine_error.map_or(String::new(), |e| format!("WebView2 did not load it: {e}"));
        let notice = re_enable_text(ext);
        let root: FrameworkElement = xaml::load(&format!(
            r#"<Grid {{ns}} ColumnSpacing="12" Padding="0,6">
                 <Grid.ColumnDefinitions>
                   <ColumnDefinition Width="32"/><ColumnDefinition Width="*"/>
                   <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
                 </Grid.ColumnDefinitions>
                 <Grid VerticalAlignment="Center">{icon}</Grid>
                 <StackPanel Grid.Column="1" VerticalAlignment="Center">
                   <TextBlock Text="{name}" Style="{{StaticResource BodyStrongTextBlockStyle}}" TextTrimming="CharacterEllipsis"/>
                   <TextBlock Text="{details}" Style="{{StaticResource CaptionTextBlockStyle}}"
                              Foreground="{{ThemeResource TextFillColorSecondaryBrush}}" TextTrimming="CharacterEllipsis"/>
                   <TextBlock Text="{error}" Style="{{StaticResource CaptionTextBlockStyle}}" TextWrapping="Wrap"
                              Foreground="{{ThemeResource SystemFillColorCriticalBrush}}" Visibility="{error_visibility}"/>
                   <TextBlock Text="{notice}" Style="{{StaticResource CaptionTextBlockStyle}}" TextWrapping="Wrap"
                              Foreground="{{ThemeResource SystemFillColorCautionBrush}}" Visibility="{notice_visibility}"/>
                 </StackPanel>
                 <Button x:Name="ReEnable" Grid.Column="2" Content="Re-enable" VerticalAlignment="Center"
                         Visibility="{notice_visibility}"/>
                 <ToggleSwitch x:Name="Enabled" Grid.Column="3" OnContent="On" OffContent="Off" MinWidth="0"
                               AutomationProperties.Name="Enabled"/>
                 <Button x:Name="Remove" Grid.Column="4" Content="Remove"/>
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
            notice = xaml::escape(&notice),
            notice_visibility = if notice.is_empty() {
                "Collapsed"
            } else {
                "Visible"
            },
        ))?;
        let toggle: ToggleSwitch = xaml::find(&root, "Enabled")?;
        toggle.SetIsOn(ext.enabled)?;
        // An update's new permissions keep it off whatever the switch says, until re-enabled.
        toggle.cast::<Control>()?.SetIsEnabled(notice.is_empty())?;
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
                        // The switch shows what core keeps, which a refused change left as it was.
                        me.render();
                    }
                });
            })?
            .forget();
        let re_enable: Button = xaml::find(&root, "ReEnable")?;
        let id = ext.id.clone();
        let name = ext.manifest.name.clone();
        let me = Rc::downgrade(self);
        on_click(&re_enable, move || {
            let Some(me) = me.upgrade() else { return };
            let Some(browser) = me.browser.upgrade() else {
                return;
            };
            let (id, name) = (id.clone(), name.clone());
            exec::spawn(async move {
                let text = match browser.approve_extension_permissions(&id).await {
                    Ok(()) => format!("Re-enabled {name}."),
                    Err(e) => format!("Could not re-enable {name}: {e}"),
                };
                let _ = me.status.SetText(&text);
                me.render();
            });
        })?;
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

/// What Chrome's install prompt says under its heading: "It can:" and the warnings, the sites
/// behind "a number of websites" indented below theirs. Empty when there is nothing to warn of.
pub(crate) fn prompt_text(manifest: &Manifest) -> String {
    let warnings = permissions::install_warnings(manifest);
    if warnings.is_empty() {
        return String::new();
    }
    warning_text(INSTALL_LEAD, &warnings)
}

/// Chrome's prompt for an extension an update turned off: `Enable “X”?`, "It can now:" and
/// what the update added, above Re-enable. Empty for any other.
fn re_enable_text(ext: &InstalledExtension) -> String {
    if ext.withheld.is_empty() {
        return String::new();
    }
    format!(
        "{}\n{}",
        permissions::re_enable_heading(&ext.manifest.name),
        warning_text(RE_ENABLE_LEAD, &ext.withheld)
    )
}

fn warning_text(lead: &str, warnings: &[PermissionMessage]) -> String {
    let mut lines = vec![lead.to_owned()];
    for warning in warnings {
        lines.push(format!("\u{2022} {}", warning.text));
        lines.extend(warning.details.iter().map(|d| format!("    \u{25E6} {d}")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_text_lists_chromes_warnings() {
        let manifest = Manifest::parse(
            r#"{ "manifest_version": 3, "name": "x", "version": "1", "permissions": ["bookmarks"],
                 "host_permissions": ["https://a.example/*", "https://b.example/*", "https://c.example/*", "https://d.example/*"] }"#,
            &|_| None,
        )
        .unwrap();
        assert_eq!(
            prompt_text(&manifest),
            "It can:\n\u{2022} Read and change your data on a number of websites\n    \u{25E6} a.example\n    \u{25E6} b.example\n    \u{25E6} c.example\n    \u{25E6} d.example\n\u{2022} Read and change your bookmarks"
        );
        let plain = Manifest::parse(r#"{ "manifest_version": 3, "name": "x", "version": "1", "permissions": ["storage"] }"#, &|_| None).unwrap();
        assert_eq!(prompt_text(&plain), "");
    }
}
