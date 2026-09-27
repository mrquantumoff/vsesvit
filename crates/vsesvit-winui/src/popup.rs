//! Extension action buttons and their popups.
//!
//! WebView2 runs the extensions but draws no browser UI for them, so the shell draws each
//! action button and hosts the popup: a `Flyout` holding a `WebView2` in the shared environment
//! (and so the same profile), navigated to `chrome-extension://<engine id>/<popup>`. There the
//! page has the full `chrome.*` API of an extension page.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use vsesvit_core::extensions::InstalledExtension;
use vsesvit_core::extensions::manifest::RelPath;
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::{exec, xaml};

/// Chrome's popup size limits.
const MIN_SIZE: f64 = 25.0;
const MAX_WIDTH: f64 = 800.0;
const MAX_HEIGHT: f64 = 600.0;

/// Whether showing a popup may take keyboard focus (and with it, window activation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activation {
    /// A click: the popup gets focus like any flyout.
    Focus,
    /// Scripted runs: the popup appears without taking focus from anyone.
    Keep,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtensionAction {
    /// The id WebView2 knows the extension by, which is its `chrome-extension://` host.
    pub extension_id: String,
    pub title: String,
    /// Path of the popup page inside the extension.
    pub popup: Option<String>,
    pub icon: Option<PathBuf>,
}

impl ExtensionAction {
    /// The toolbar button of an installed extension, once the engine has loaded it. Disabled
    /// extensions and extensions the engine has not loaded have none.
    pub fn from_installed(ext: &InstalledExtension) -> Option<Self> {
        let engine_id = ext.engine_id.clone().filter(|_| ext.enabled)?;
        let manifest = &ext.manifest;
        let action = manifest.action.as_ref();
        let title = action
            .and_then(|a| a.default_title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| manifest.name.clone());
        let icon = action
            .and_then(|a| best_icon(&a.default_icon))
            .or_else(|| best_icon(&manifest.icons))
            .map(|icon| icon.resolve(&ext.dir));
        Some(Self {
            extension_id: engine_id,
            title,
            popup: action
                .and_then(|a| a.default_popup.as_ref())
                .map(|p| p.as_str().to_owned()),
            icon,
        })
    }

    pub fn popup_url(&self) -> Option<String> {
        let popup = self.popup.as_deref()?.trim_start_matches('/');
        Some(format!("chrome-extension://{}/{popup}", self.extension_id))
    }
}

/// The icon size closest to 32 px.
pub(crate) fn best_icon(icons: &BTreeMap<u32, RelPath>) -> Option<&RelPath> {
    icons
        .iter()
        .min_by_key(|(size, _)| size.abs_diff(32))
        .map(|(_, path)| path)
}

pub(crate) fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file:///");
    for c in path.to_string_lossy().chars() {
        match c {
            '\\' => uri.push('/'),
            ' ' => uri.push_str("%20"),
            '#' => uri.push_str("%23"),
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3F"),
            c => uri.push(c),
        }
    }
    uri
}

/// An `Image` of the file at `icon`, or the generic extension glyph.
pub(crate) fn icon_markup(icon: Option<&Path>, size: u32) -> String {
    match icon {
        Some(icon) => format!(
            r#"<Image Width="{size}" Height="{size}" Source="{}"/>"#,
            xaml::escape(&file_uri(icon))
        ),
        None => format!(r#"<FontIcon Glyph="&#xEA86;" FontSize="{size}"/>"#),
    }
}

pub(crate) fn action_button(
    action: &ExtensionAction,
    environment: CoreWebView2Environment,
) -> Result<Button> {
    let content = icon_markup(action.icon.as_deref(), 16);
    let title = xaml::escape(&action.title);
    let button: Button = xaml::load(&format!(
        r#"<Button {{ns}} Background="Transparent" BorderThickness="0" Padding="0" Width="36" Height="32"
                   ToolTipService.ToolTip="{title}" AutomationProperties.Name="{title}">{content}</Button>"#
    ))?;
    let action = action.clone();
    let anchor = button.cast::<FrameworkElement>()?;
    button
        .cast::<ButtonBase>()?
        .Click(move |_, _| {
            if let Err(e) = open(&anchor, environment.clone(), &action, Activation::Focus) {
                log::error!("popup of {}: {e}", action.extension_id);
            }
        })?
        .forget();
    Ok(button)
}

const FLYOUT_XAML: &str = r#"
<Flyout {ns} Placement="BottomEdgeAlignedRight" ShouldConstrainToRootBounds="True">
  <Flyout.FlyoutPresenterStyle>
    <Style TargetType="FlyoutPresenter">
      <Setter Property="Padding" Value="0"/>
      <Setter Property="MaxWidth" Value="820"/>
      <Setter Property="MaxHeight" Value="640"/>
    </Style>
  </Flyout.FlyoutPresenterStyle>
  <Grid/>
</Flyout>"#;

/// An open popup.
pub(crate) struct Popup {
    flyout: FlyoutBase,
    core: Rc<RefCell<Option<CoreWebView2>>>,
}

impl Popup {
    /// The popup document's title, once its page has loaded.
    pub fn title(&self) -> Option<String> {
        self.core.borrow().as_ref()?.DocumentTitle().ok()
    }

    pub fn hide(&self) {
        let _ = self.flyout.Hide();
    }
}

/// Shows the action's popup under `anchor`.
pub(crate) fn open(
    anchor: &FrameworkElement,
    environment: CoreWebView2Environment,
    action: &ExtensionAction,
    activation: Activation,
) -> Result<Popup> {
    let flyout: Flyout = xaml::load(FLYOUT_XAML)?;
    let flyout_base = flyout.cast::<FlyoutBase>()?;
    let popup = Popup {
        flyout: flyout_base.clone(),
        core: Rc::new(RefCell::new(None)),
    };
    let Some(url) = action.popup_url() else {
        log::info!("extension {} has no popup", action.extension_id);
        return Ok(popup);
    };
    let host: Panel = flyout.Content()?.cast()?;
    let view = WebView2::new()?;
    let element = view.cast::<FrameworkElement>()?;
    element.SetWidth(360.0)?;
    element.SetHeight(240.0)?;
    host.Children()?.Append(&view.cast::<UIElement>()?)?;
    let options = FlyoutShowOptions::new()?;
    options.SetShowMode(match activation {
        Activation::Focus => FlyoutShowMode::Standard,
        Activation::Keep => FlyoutShowMode::Transient,
    })?;
    flyout_base.ShowAtWithOptions(anchor, &options)?;
    let closing_view = view.clone();
    flyout_base
        .Closed(move |_, _| {
            let _ = closing_view.Close();
        })?
        .forget();
    let slot = popup.core.clone();
    exec::spawn(async move {
        if let Err(e) = load(&flyout_base, &view, &environment, &url, &slot).await {
            log::error!("popup {url}: {e}");
            let _ = flyout_base.Hide();
        }
    });
    Ok(popup)
}

async fn load(
    flyout: &FlyoutBase,
    view: &WebView2,
    environment: &CoreWebView2Environment,
    url: &str,
    slot: &RefCell<Option<CoreWebView2>>,
) -> Result<()> {
    view.cast::<IWebView22>()?
        .EnsureCoreWebView2WithEnvironmentAsync(environment)?
        .await?;
    let core = view.CoreWebView2()?;
    let hide = flyout.clone();
    core.WindowCloseRequested(move |_, _| {
        let _ = hide.Hide();
    })?
    .forget();
    let fit_view = view.clone();
    core.NavigationCompleted(move |sender, _| {
        if let Some(core) = sender.as_ref() {
            exec::spawn(fit_to_content(fit_view.clone(), core.clone()));
        }
    })?
    .forget();
    *slot.borrow_mut() = Some(core.clone());
    log::info!("popup: {url}");
    core.Navigate(url)
}

/// Sizes the popup to its document, within Chrome's 25x25..800x600 limits.
async fn fit_to_content(view: WebView2, core: CoreWebView2) {
    const WIDTH: &str = "(() => { const d = document.documentElement; const old = d.style.width; \
        d.style.width = 'max-content'; const w = d.getBoundingClientRect().width; \
        d.style.width = old; return Math.ceil(w); })()";
    const HEIGHT: &str = "Math.ceil(document.documentElement.getBoundingClientRect().height)";
    let Ok(element) = view.cast::<FrameworkElement>() else {
        return;
    };
    if let Some(width) = measure(&core, WIDTH).await {
        let _ = element.SetWidth(width.clamp(MIN_SIZE, MAX_WIDTH));
        exec::sleep(Duration::from_millis(60)).await;
    }
    if let Some(height) = measure(&core, HEIGHT).await {
        let _ = element.SetHeight(height.clamp(MIN_SIZE, MAX_HEIGHT));
    }
}

async fn measure(core: &CoreWebView2, script: &str) -> Option<f64> {
    let json = exec::timeout(Duration::from_secs(5), async {
        core.ExecuteScriptAsync(script).ok()?.await.ok()
    })
    .await
    .flatten()?;
    json.to_string_lossy()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

#[cfg(test)]
mod tests {
    use vsesvit_core::extensions::manifest::Manifest;
    use vsesvit_core::extensions::{ExtensionId, InstallSource, Verification};

    use super::*;

    fn installed(manifest: &str, engine_id: Option<&str>, enabled: bool) -> InstalledExtension {
        let manifest = Manifest::parse(manifest, &|_| None).unwrap();
        InstalledExtension {
            id: ExtensionId::parse("abcdefghijklmnopabcdefghijklmnop").unwrap(),
            version: manifest.version.clone(),
            dir: PathBuf::from(r"C:\ext"),
            manifest,
            enabled,
            source: InstallSource::Unpacked {
                dir: PathBuf::from(r"C:\ext"),
            },
            verification: Verification::Unpacked,
            engine_id: engine_id.map(str::to_owned),
        }
    }

    #[test]
    fn mv3_action_with_popup_and_icon_map() {
        let manifest = r#"{"manifest_version":3,"name":"Probe","version":"1.0","action":{"default_title":"Probe title",
            "default_popup":"popup.html","default_icon":{"16":"i16.png","32":"i32.png","128":"i128.png"}}}"#;
        let action =
            ExtensionAction::from_installed(&installed(manifest, Some("engineid"), true)).unwrap();
        assert_eq!(action.title, "Probe title");
        assert_eq!(
            action.popup_url().as_deref(),
            Some("chrome-extension://engineid/popup.html")
        );
        assert_eq!(action.icon, Some(PathBuf::from(r"C:\ext\i32.png")));
    }

    #[test]
    fn falls_back_to_name_and_manifest_icons() {
        let manifest = r#"{"manifest_version":2,"name":"Old","version":"1","browser_action":{"default_popup":"p/index.html"},"icons":{"48":"i48.png"}}"#;
        let action =
            ExtensionAction::from_installed(&installed(manifest, Some("id"), true)).unwrap();
        assert_eq!(action.title, "Old");
        assert_eq!(
            action.popup_url().as_deref(),
            Some("chrome-extension://id/p/index.html")
        );
        assert_eq!(action.icon, Some(PathBuf::from(r"C:\ext\i48.png")));
    }

    #[test]
    fn no_button_until_loaded_or_while_disabled() {
        let manifest = r#"{"manifest_version":3,"name":"X","version":"1"}"#;
        assert!(ExtensionAction::from_installed(&installed(manifest, None, true)).is_none());
        assert!(ExtensionAction::from_installed(&installed(manifest, Some("id"), false)).is_none());
        let action =
            ExtensionAction::from_installed(&installed(manifest, Some("id"), true)).unwrap();
        assert_eq!(action.popup_url(), None);
        assert_eq!(action.icon, None);
    }

    #[test]
    fn file_uris_escape_reserved_characters() {
        assert_eq!(
            file_uri(Path::new(r"C:\a b\#1%.png")),
            "file:///C:/a%20b/%231%25.png"
        );
    }
}
