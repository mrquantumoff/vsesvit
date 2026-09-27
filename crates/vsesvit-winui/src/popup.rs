//! Extension action buttons and their popups.
//!
//! WebView2 runs the extensions but draws no browser UI for them, so the shell draws each
//! action button and hosts the popup: a `Flyout` holding a `WebView2` in the shared environment
//! (and so the same profile), navigated to `chrome-extension://<id>/<popup>`. There the page has
//! the full `chrome.*` API of an extension page.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
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
    pub extension_id: String,
    pub title: String,
    /// Path of the popup page inside the extension.
    pub popup: Option<String>,
    pub icon: Option<PathBuf>,
}

impl ExtensionAction {
    /// Reads the action of an unpacked folder loaded with `--load-extension`. Installed
    /// extensions get theirs from vsesvit-core's parsed manifest instead.
    pub fn from_unpacked(extension_id: &str, dir: &Path) -> std::result::Result<Self, String> {
        let manifest = std::fs::read_to_string(dir.join("manifest.json"))
            .map_err(|e| format!("manifest.json: {e}"))?;
        parse_action(&manifest, extension_id, dir)
    }

    pub fn popup_url(&self) -> Option<String> {
        let popup = self.popup.as_deref()?.trim_start_matches('/');
        Some(format!("chrome-extension://{}/{popup}", self.extension_id))
    }
}

fn parse_action(
    manifest: &str,
    extension_id: &str,
    dir: &Path,
) -> std::result::Result<ExtensionAction, String> {
    let manifest: Value =
        serde_json::from_str(manifest).map_err(|e| format!("manifest.json: {e}"))?;
    let action = ["action", "browser_action", "page_action"]
        .iter()
        .find_map(|key| manifest.get(*key))
        .cloned()
        .unwrap_or(Value::Null);
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let title = text(action.get("default_title"))
        .or_else(|| text(manifest.get("name")))
        .unwrap_or_else(|| extension_id.to_owned());
    let popup = text(action.get("default_popup"));
    let icon = best_icon(action.get("default_icon")).or_else(|| best_icon(manifest.get("icons")));
    Ok(ExtensionAction {
        extension_id: extension_id.to_owned(),
        title,
        popup,
        icon: icon.map(|relative| dir.join(relative.trim_start_matches('/'))),
    })
}

/// A single path, or the size closest to 32 px from a `{"16": ..., "32": ...}` map.
fn best_icon(icons: Option<&Value>) -> Option<String> {
    match icons? {
        Value::String(path) => Some(path.clone()),
        Value::Object(sizes) => sizes
            .iter()
            .filter_map(|(size, path)| Some((size.parse::<u32>().ok()?, path.as_str()?)))
            .min_by_key(|(size, _)| size.abs_diff(32))
            .map(|(_, path)| path.to_owned()),
        _ => None,
    }
}

fn file_uri(path: &Path) -> String {
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

pub(crate) fn action_button(
    action: &ExtensionAction,
    environment: CoreWebView2Environment,
) -> Result<Button> {
    let content = match &action.icon {
        Some(icon) => format!(
            r#"<Image Width="16" Height="16" Source="{}"/>"#,
            xaml::escape(&file_uri(icon))
        ),
        None => r#"<FontIcon Glyph="&#xEA86;" FontSize="16"/>"#.to_owned(),
    };
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

/// Shows the action's popup under `anchor`.
pub(crate) fn open(
    anchor: &FrameworkElement,
    environment: CoreWebView2Environment,
    action: &ExtensionAction,
    activation: Activation,
) -> Result<()> {
    let Some(url) = action.popup_url() else {
        log::info!("extension {} has no popup", action.extension_id);
        return Ok(());
    };
    let flyout: Flyout = xaml::load(FLYOUT_XAML)?;
    let host: Panel = flyout.Content()?.cast()?;
    let view = WebView2::new()?;
    let element = view.cast::<FrameworkElement>()?;
    element.SetWidth(360.0)?;
    element.SetHeight(240.0)?;
    host.Children()?.Append(&view.cast::<UIElement>()?)?;
    let flyout = flyout.cast::<FlyoutBase>()?;
    let options = FlyoutShowOptions::new()?;
    options.SetShowMode(match activation {
        Activation::Focus => FlyoutShowMode::Standard,
        Activation::Keep => FlyoutShowMode::Transient,
    })?;
    flyout.ShowAtWithOptions(anchor, &options)?;
    let closing_view = view.clone();
    flyout
        .Closed(move |_, _| {
            let _ = closing_view.Close();
        })?
        .forget();
    exec::spawn(async move {
        if let Err(e) = load(&flyout, &view, &environment, &url).await {
            log::error!("popup {url}: {e}");
            let _ = flyout.Hide();
        }
    });
    Ok(())
}

async fn load(
    flyout: &FlyoutBase,
    view: &WebView2,
    environment: &CoreWebView2Environment,
    url: &str,
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
    let json = core.ExecuteScriptAsync(script).ok()?.await.ok()?;
    json.to_string_lossy()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mv3_action_with_popup_and_icon_map() {
        let manifest = r#"{"name":"Probe","action":{"default_title":"Probe title",
            "default_popup":"popup.html","default_icon":{"16":"i16.png","32":"i32.png","128":"i128.png"}}}"#;
        let action = parse_action(manifest, "abc", Path::new(r"C:\ext")).unwrap();
        assert_eq!(action.title, "Probe title");
        assert_eq!(
            action.popup_url().as_deref(),
            Some("chrome-extension://abc/popup.html")
        );
        assert_eq!(action.icon, Some(PathBuf::from(r"C:\ext\i32.png")));
    }

    #[test]
    fn mv2_browser_action_falls_back_to_name_and_icons() {
        let manifest = r#"{"name":"Old","browser_action":{"default_popup":"/p/index.html"},"icons":{"48":"i48.png"}}"#;
        let action = parse_action(manifest, "id", Path::new(r"C:\e")).unwrap();
        assert_eq!(action.title, "Old");
        assert_eq!(
            action.popup_url().as_deref(),
            Some("chrome-extension://id/p/index.html")
        );
        assert_eq!(action.icon, Some(PathBuf::from(r"C:\e\i48.png")));
    }

    #[test]
    fn no_action_means_no_popup() {
        let action = parse_action(r#"{"name":"X"}"#, "id", Path::new("d")).unwrap();
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
