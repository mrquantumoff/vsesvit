//! Extension action buttons and their popups.
//!
//! WebView2 runs the extensions but draws no browser UI for them, so the shell draws each
//! action button and hosts the popup: a `Flyout` holding a `WebView2` in the shared environment
//! (and so the same profile), navigated to `chrome-extension://<engine id>/<popup>`. There the
//! page has the full `chrome.*` API of an extension page. `js/popup.js` runs in every popup
//! document to close the gaps to Chrome's popup view: it answers tab queries about "the current
//! window" with the browser window's tabs and reports the size the popup should have.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::Serialize;

use vsesvit_core::Url;
use vsesvit_core::extensions::InstalledExtension;
use vsesvit_core::extensions::manifest::RelPath;
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::{exec, xaml};

/// Chrome's popup size limits; `js/popup.js` keeps to them too.
const MIN_SIZE: f64 = 25.0;
const MAX_WIDTH: f64 = 800.0;
const MAX_HEIGHT: f64 = 600.0;

const POPUP_SCRIPT: &str = include_str!("js/popup.js");

/// A tab of the browser window a popup opens from, as the popup's tab queries see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct OpenerTab {
    pub url: String,
    pub active: bool,
}

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
    /// The extension's id in core (`ExtensionId`), which the toolbar preference keys by.
    pub id: String,
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
            id: ext.id.as_str().to_owned(),
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

/// An `Image` of the file at `icon`, or the generic extension glyph.
pub(crate) fn icon_markup(icon: Option<&Path>, size: u32) -> String {
    match icon.and_then(|icon| Url::from_file_path(icon).ok()) {
        Some(uri) => format!(
            r#"<Image Width="{size}" Height="{size}" Source="{}"/>"#,
            xaml::escape(uri.as_str())
        ),
        None => format!(r#"<FontIcon Glyph="&#xEA86;" FontSize="{size}"/>"#),
    }
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
#[derive(Clone)]
pub(crate) struct Popup {
    flyout: FlyoutBase,
    core: Rc<RefCell<Option<CoreWebView2>>>,
}

impl Popup {
    /// The popup document's title, once its page has loaded.
    pub fn title(&self) -> Option<String> {
        self.core.borrow().as_ref()?.DocumentTitle().ok()
    }

    /// The popup document's address, once its page is loading.
    pub fn url(&self) -> Option<String> {
        self.core.borrow().as_ref()?.Source().ok()
    }

    pub fn is_open(&self) -> bool {
        self.flyout.IsOpen().unwrap_or(false)
    }

    pub fn hide(&self) {
        let _ = self.flyout.Hide();
    }
}

/// Shows the action's popup under `anchor`, for the browser window whose tabs are `opener`.
pub(crate) fn open(
    anchor: &FrameworkElement,
    environment: CoreWebView2Environment,
    action: &ExtensionAction,
    activation: Activation,
    opener: &[OpenerTab],
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
    let script = popup_script(opener);
    exec::spawn(async move {
        if let Err(e) = load(&flyout_base, &view, &environment, &url, &script, &slot).await {
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
    script: &str,
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
    let element = view.cast::<FrameworkElement>()?;
    core.WebMessageReceived(move |_, args| {
        let size = args
            .as_ref()
            .and_then(|a| a.TryGetWebMessageAsString().ok())
            .and_then(|m| parse_size(&m));
        if let Some((width, height)) = size {
            let _ = element.SetWidth(width);
            let _ = element.SetHeight(height);
        }
    })?
    .forget();
    core.AddScriptToExecuteOnDocumentCreatedAsync(script)?
        .await?;
    *slot.borrow_mut() = Some(core.clone());
    log::info!("popup: {url}");
    core.Navigate(url)
}

/// `js/popup.js`, called with the opener's tabs.
fn popup_script(opener: &[OpenerTab]) -> String {
    let opener = serde_json::to_string(opener).expect("strings and booleans serialize");
    format!("{}({opener});", POPUP_SCRIPT.trim_end())
}

/// The size a popup's `{"popupSize": [width, height]}` message asks for, within Chrome's limits.
fn parse_size(message: &str) -> Option<(f64, f64)> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    let [width, height] = value.get("popupSize")?.as_array()?.as_slice() else {
        return None;
    };
    let fit = |v: &serde_json::Value, max: f64| {
        v.as_f64()
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(MIN_SIZE, max))
    };
    Some((fit(width, MAX_WIDTH)?, fit(height, MAX_HEIGHT)?))
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
            withheld: Default::default(),
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
    fn size_messages_are_clamped_to_chromes_limits() {
        assert_eq!(
            parse_size(r#"{"popupSize":[600,430]}"#),
            Some((600.0, 430.0))
        );
        assert_eq!(parse_size(r#"{"popupSize":[1,9000]}"#), Some((25.0, 600.0)));
        assert_eq!(parse_size(r#"{"popupSize":[1]}"#), None);
        assert_eq!(parse_size(r#"{"popupSize":["a",1]}"#), None);
        assert_eq!(parse_size("not json"), None);
    }

    #[test]
    fn the_popup_script_is_called_with_the_opener_tabs() {
        let script = popup_script(&[OpenerTab {
            url: "https://e.test/\"x".into(),
            active: true,
        }]);
        assert!(
            script.ends_with(r#"})([{"url":"https://e.test/\"x","active":true}]);"#),
            "{script}"
        );
    }

    #[test]
    fn icon_markup_uses_escaped_file_urls() {
        let markup = |path: &str| icon_markup(Some(Path::new(path)), 16);
        assert!(markup(r"C:\a b\#1%.png").contains(r#"Source="file:///C:/a%20b/%231%25.png""#));
        assert!(markup(r"C:\a{b}`\i.png").contains(r#"Source="file:///C:/a%7Bb%7D%60/i.png""#));
        assert!(markup(r"\\srv\share\i.png").contains(r#"Source="file://srv/share/i.png""#));
        assert!(markup("rel.png").contains("FontIcon"));
    }
}
