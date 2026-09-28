//! Small helpers over the minimal XAML bindings.

use windows_core::{IInspectable, IUnknown, Interface, Result};
use windows_reference::IReference;

use crate::bindings::*;

pub(crate) const NAMESPACES: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

/// Parses markup whose root element carries `{ns}` in place of the XAML namespaces. Every
/// `{acrylic_menu}` becomes [`ACRYLIC_MENU`].
pub(crate) fn load<T: Interface>(markup: &str) -> Result<T> {
    let markup = markup
        .replacen("{ns}", NAMESPACES, 1)
        .replace("{acrylic_menu}", ACRYLIC_MENU);
    XamlReader::Load(&markup)?.cast()
}

/// Children of a `MenuFlyout` that give it its own acrylic backdrop, as Windows 11 menus have,
/// so the menu shows what is behind it even where it extends past the window.
const ACRYLIC_MENU: &str = r#"
    <MenuFlyout.SystemBackdrop><DesktopAcrylicBackdrop/></MenuFlyout.SystemBackdrop>
    <MenuFlyout.MenuFlyoutPresenterStyle>
      <Style TargetType="MenuFlyoutPresenter" BasedOn="{StaticResource DefaultMenuFlyoutPresenterStyle}">
        <Setter Property="Background" Value="Transparent"/>
      </Style>
    </MenuFlyout.MenuFlyoutPresenterStyle>"#;

/// An empty menu with the acrylic backdrop, opening below its anchor. It may extend past the
/// window (Windows keeps it on the screen), so a long menu is never cut at the window's edge.
pub(crate) fn acrylic_menu() -> Result<MenuFlyout> {
    load(
        r#"<MenuFlyout {ns} Placement="BottomEdgeAlignedLeft" ShouldConstrainToRootBounds="False">
             {acrylic_menu}</MenuFlyout>"#,
    )
}

/// An empty context menu with the acrylic backdrop, which opens where it was asked for.
pub(crate) fn context_menu() -> Result<MenuFlyout> {
    load(r#"<MenuFlyout {ns} ShouldConstrainToRootBounds="False">{acrylic_menu}</MenuFlyout>"#)
}

/// Decodes a PNG into an image source.
pub(crate) async fn png_image(png: &[u8]) -> Result<ImageSource> {
    let stream = InMemoryRandomAccessStream::new()?.cast::<IRandomAccessStream>()?;
    let writer = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
    writer.WriteBytes(png)?;
    writer.StoreAsync()?.await?;
    let bitmap = BitmapImage::new()?;
    bitmap
        .cast::<BitmapSource>()?
        .SetSourceAsync(&stream)?
        .await?;
    bitmap.cast()
}

/// In markup that has a `Glyph` icon and a collapsed `Favicon` image: shows the favicon in the
/// glyph's place once the PNG has decoded.
pub(crate) fn show_favicon(root: &FrameworkElement, png: Vec<u8>) -> Result<()> {
    let image: Image = find(root, "Favicon")?;
    let glyph: UIElement = find(root, "Glyph")?;
    crate::exec::spawn(async move {
        match png_image(&png).await {
            Ok(source) => {
                let _ = image.SetSource(&source);
                let _ = set_visible(&image, true);
                let _ = set_visible(&glyph, false);
            }
            Err(e) => log::debug!("favicon: {e}"),
        }
    });
    Ok(())
}

/// Every byte of a stream, from its start.
pub(crate) async fn read_all(stream: &IRandomAccessStream) -> Result<Vec<u8>> {
    let size = u32::try_from(stream.Size()?).map_err(|_| windows_core::Error::empty())?;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.await?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}

pub(crate) fn find<T: Interface>(scope: &FrameworkElement, name: &str) -> Result<T> {
    scope
        .FindName(name)?
        .cast()
        .inspect_err(|e| log::error!("XAML part {name}: {e}"))
}

pub(crate) fn boxed(text: &str) -> Result<IInspectable> {
    IReference::<windows_core::HSTRING>::from(text).cast()
}

pub(crate) fn set_visible(element: &impl Interface, visible: bool) -> Result<()> {
    let visibility = if visible {
        Visibility::Visible
    } else {
        Visibility::Collapsed
    };
    element.cast::<UIElement>()?.SetVisibility(visibility)
}

pub(crate) fn is_visible(element: &impl Interface) -> bool {
    element
        .cast::<UIElement>()
        .and_then(|e| e.Visibility())
        .is_ok_and(|v| v == Visibility::Visible)
}

/// COM identity: two references to the same object compare equal as `IUnknown`.
pub(crate) fn same_object(a: &impl Interface, b: &impl Interface) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Depth-first search of the visual tree for the first element of type `T`.
pub(crate) fn find_descendant<T: Interface>(root: &DependencyObject) -> Option<T> {
    let count = VisualTreeHelper::GetChildrenCount(root).ok()?;
    for index in 0..count {
        let Ok(child) = VisualTreeHelper::GetChild(root, index) else {
            continue;
        };
        if let Ok(found) = child.cast::<T>() {
            return Some(found);
        }
        if let Some(found) = find_descendant(&child) {
            return Some(found);
        }
    }
    None
}

/// Depth-first search of the visual tree for the element named `name` (`x:Name`). Unlike
/// `FindName`, this works for content moved out of the markup that declared it.
pub(crate) fn find_named<T: Interface>(root: &DependencyObject, name: &str) -> Option<T> {
    let count = VisualTreeHelper::GetChildrenCount(root).ok()?;
    for index in 0..count {
        let Ok(child) = VisualTreeHelper::GetChild(root, index) else {
            continue;
        };
        let named = child
            .cast::<FrameworkElement>()
            .and_then(|e| e.Name())
            .is_ok_and(|n| n == name);
        if named && let Ok(found) = child.cast::<T>() {
            return Some(found);
        }
        if let Some(found) = find_named(&child, name) {
            return Some(found);
        }
    }
    None
}

/// Escapes text for use inside a double-quoted XAML attribute or element content.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '{' if out.is_empty() => out.push_str("{}{"),
            // Not characters XML can hold, so the markup would not parse.
            '\u{FFFE}' | '\u{FFFF}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escapes_markup() {
        assert_eq!(escape(r#"a<b>&"c""#), "a&lt;b&gt;&amp;&quot;c&quot;");
    }

    #[test]
    fn leading_brace_is_not_a_markup_extension() {
        assert_eq!(escape("{Binding}"), "{}{Binding}");
        assert_eq!(escape("x{y}"), "x{y}");
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(escape("a\u{0}b\nc"), "abc");
    }

    #[test]
    fn characters_xml_cannot_hold_are_dropped() {
        assert_eq!(escape("a\u{FFFE}b\u{FFFF}c"), "abc");
        assert_eq!(
            escape("\u{FDD0}\u{1FFFF}\u{1F600}"),
            "\u{FDD0}\u{1FFFF}\u{1F600}"
        );
    }
}
