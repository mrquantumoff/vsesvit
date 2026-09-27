//! About: versions and the profile folder.

use windows_core::Result;

use super::Wired;
use crate::bindings::*;
use crate::browser::Browser;
use crate::xaml;

pub(super) const MARKUP: &str = r#"
  <StackPanel Spacing="6" MinWidth="360">
    <TextBlock x:Name="AboutVersion" Style="{StaticResource BodyStrongTextBlockStyle}"/>
    <TextBlock x:Name="AboutEngine" IsTextSelectionEnabled="True"/>
    <TextBlock x:Name="AboutProfile" TextWrapping="Wrap" IsTextSelectionEnabled="True"
               Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>
  </StackPanel>"#;

pub(super) fn fill(root: &FrameworkElement, browser: &Browser) -> Result<Wired> {
    let version: TextBlock = xaml::find(root, "AboutVersion")?;
    version.SetText(&format!("Vsesvit {}", env!("CARGO_PKG_VERSION")))?;
    let engine: TextBlock = xaml::find(root, "AboutEngine")?;
    engine.SetText(&format!(
        "Microsoft Edge WebView2 {}",
        browser.engine().browser_version()
    ))?;
    let profile: TextBlock = xaml::find(root, "AboutProfile")?;
    profile.SetText(&format!("Profile: {}", browser.profile_dir().display()))?;
    Ok(Wired::default())
}
