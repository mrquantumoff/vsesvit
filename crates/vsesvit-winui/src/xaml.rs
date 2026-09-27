//! Small helpers over the minimal XAML bindings.

use windows_core::{IInspectable, IUnknown, Interface, Result};
use windows_reference::IReference;

use crate::bindings::*;

pub(crate) const NAMESPACES: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

/// Parses markup whose root element carries `{ns}` in place of the XAML namespaces.
pub(crate) fn load<T: Interface>(markup: &str) -> Result<T> {
    XamlReader::Load(&markup.replacen("{ns}", NAMESPACES, 1))?.cast()
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
}
