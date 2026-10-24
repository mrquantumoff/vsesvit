//! Owned by W1.3.
//! The string rules of the cef crate (API G3, G4, G5) in one place, so no handler gets them wrong.

use cef::{CefString, CefStringList, CefStringUserfree};

/// A string CEF will own and free, for a field of a struct out-parameter (`WindowInfo` in
/// `on_before_popup`): `CefString::from(&str)` is written back there as an empty string (G3).
pub fn owned(text: &str) -> CefString {
    let mut raw: cef::sys::cef_string_utf16_t = unsafe { std::mem::zeroed() };
    // SAFETY: `text` is valid UTF-8 for its length; CEF allocates `raw` and sets its destructor.
    unsafe { cef::sys::cef_string_utf8_to_utf16(text.as_ptr().cast(), text.len(), &mut raw) };
    CefString::from(raw)
}

/// An empty buffer for a required string out-parameter. `CefString::default()` is a null pointer,
/// which CEF silently ignores (G4).
pub fn out() -> CefString {
    CefString::from("")
}

/// The text of a string CEF lends a callback. Copies it: a clone would dangle after the call (G5).
pub fn text(s: Option<&CefString>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}

/// The text of a string CEF returns to us (`frame.url()`, `CefStringUserfree`).
pub fn userfree(s: &CefStringUserfree) -> String {
    CefString::from(s).to_string()
}

/// The items of a list CEF lends a callback. Takes the list: a cloned list yields nothing (G5).
pub fn take_list(list: Option<&mut CefStringList>) -> Vec<String> {
    list.map(|l| std::mem::take(l).into_iter().collect()).unwrap_or_default()
}

/// `text`, or `None` when empty: CEF passes "no value" as an empty string.
pub fn non_empty(s: Option<&CefString>) -> Option<String> {
    Some(text(s)).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lent_strings_are_copied() {
        let lent = CefString::from("https://example.com/");
        let copy = text(Some(&lent));
        drop(lent);
        assert_eq!(copy, "https://example.com/");
        assert_eq!(text(None), "");
        assert_eq!(non_empty(Some(&CefString::from(""))), None);
        assert_eq!(non_empty(Some(&CefString::from("x"))).as_deref(), Some("x"));
    }

    #[test]
    fn out_params_get_a_buffer_not_a_null() {
        // `CefString::default()` converts to a null pointer, `out()` to an allocated empty string.
        let null: *const cef::sys::cef_string_t = (&CefString::default()).into();
        assert!(null.is_null());
        let buffer = out();
        let raw: *const cef::sys::cef_string_t = (&buffer).into();
        assert!(!raw.is_null());
        assert_eq!(buffer.to_string(), "");
    }

    #[test]
    fn lent_lists_are_taken() {
        let mut list = CefStringList::new();
        list.append("a");
        list.append("b");
        assert_eq!(take_list(Some(&mut list)), ["a", "b"]);
        assert!(take_list(None).is_empty());
    }

    #[test]
    fn owned_strings_carry_their_text() {
        assert_eq!(owned("org.vsesvit.Vsesvit").to_string(), "org.vsesvit.Vsesvit");
    }
}
