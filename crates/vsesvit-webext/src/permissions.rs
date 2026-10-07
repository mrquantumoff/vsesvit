//! `chrome.permissions` on the wire, and what the shell shows when an extension asks for more.
//! Which permissions an extension holds, may ask for and how Chrome words them is core's
//! (`vsesvit_core::extensions::permissions`).

use std::path::PathBuf;

use serde_json::{Value, json};
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::extensions::permissions::{PermissionMessage, PermissionSet, PermissionsError};

/// A `permissions.request` the user decides on, worded as Chrome's prompt: the heading is
/// `request_heading(name)`, then `REQUEST_LEAD` and the warnings, then Allow and Deny.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub extension: ExtensionId,
    pub name: String,
    pub icon: Option<PathBuf>,
    pub warnings: Vec<PermissionMessage>,
}

/// `chrome.permissions.Permissions` from the shim: `permissions` and `origins`, both optional.
pub fn parse(value: &Value) -> Result<PermissionSet, String> {
    let strings = |key: &str| -> Result<Vec<String>, String> {
        match value.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => items.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("Error in invocation of permissions: '{key}' must be an array of strings"))).collect(),
            Some(_) => Err(format!("Error in invocation of permissions: '{key}' must be an array of strings")),
        }
    };
    PermissionSet::from_request(&strings("permissions")?, &strings("origins")?).map_err(|e: PermissionsError| e.to_string())
}

pub fn to_json(set: &PermissionSet) -> Value {
    json!({ "permissions": set.apis, "origins": set.origins.iter().map(|o| o.as_str()).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_round_trip_as_chrome_spells_them() {
        let set = parse(&json!({ "permissions": ["tabs"], "origins": ["https://a.example/*"] })).unwrap();
        assert_eq!(to_json(&set), json!({ "permissions": ["tabs"], "origins": ["https://a.example/*"] }));
        assert_eq!(to_json(&parse(&json!({})).unwrap()), json!({ "permissions": [], "origins": [] }));
        assert!(parse(&json!({ "permissions": "tabs" })).is_err());
        assert!(parse(&json!({ "origins": ["not a pattern"] })).unwrap_err().starts_with("Invalid value for origin pattern not a pattern"));
    }
}
