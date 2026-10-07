//! Chrome's two extension prompts: the install prompt ("Add “X”? It can: ...") before an
//! install from a store or a package is committed, and the one `permissions.request` shows.
//! The warnings are core's, in Chrome's words. The install prompt also says what the manifest
//! asks for that the Linux runtime lacks.

use adw::prelude::*;
use vsesvit_core::extensions::manifest::Manifest;
use vsesvit_core::extensions::permissions::{
    self, INSTALL_LEAD, PermissionMessage, REQUEST_LEAD,
};
use vsesvit_webext::permissions::Prompt;

use crate::extensions::unsupported_notice;

pub(crate) const ADD: &str = "add";
pub(crate) const ALLOW: &str = "allow";

/// The install prompt for `manifest`; its `ADD` response installs.
pub(crate) fn install_dialog(manifest: &Manifest) -> adw::AlertDialog {
    let warnings = permissions::install_warnings(manifest);
    let dialog = adw::AlertDialog::new(Some(&permissions::install_heading(&manifest.name)), None);
    let content = warning_list((!warnings.is_empty()).then_some(INSTALL_LEAD), &warnings);
    if let Some(notice) = unsupported_notice(manifest) {
        let unsupported = gtk::Label::builder()
            .label(format!("Not supported by the Linux runtime: {notice}. Features that depend on them will not work."))
            .wrap(true)
            .xalign(0.0)
            .css_classes(["warning", "caption"])
            .build();
        content.append(&unsupported);
    }
    dialog.set_extra_child(Some(&content));
    dialog.add_responses(&[("cancel", "_Cancel"), (ADD, "_Add Extension")]);
    dialog.set_response_appearance(ADD, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog
}

/// The `permissions.request` prompt; its `ALLOW` response grants.
pub(crate) fn request_dialog(prompt: &Prompt) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(&permissions::request_heading(&prompt.name)), None);
    dialog.set_extra_child(Some(&warning_list(Some(REQUEST_LEAD), &prompt.warnings)));
    dialog.add_responses(&[("deny", "_Deny"), (ALLOW, "_Allow")]);
    dialog.set_response_appearance(ALLOW, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("deny"));
    dialog.set_close_response("deny");
    dialog
}

/// The lead line, then each warning as a bullet with the sites behind it indented below.
fn warning_list(lead: Option<&str>, warnings: &[PermissionMessage]) -> gtk::Box {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let label = |text: String, margin: i32| {
        gtk::Label::builder().label(text).wrap(true).xalign(0.0).margin_start(margin).build()
    };
    if let Some(lead) = lead {
        list.append(&label(lead.to_owned(), 0));
    }
    for warning in warnings {
        list.append(&label(format!("• {}", warning.text), 6));
        for detail in &warning.details {
            list.append(&label(format!("◦ {detail}"), 24));
        }
    }
    list
}

/// The text the prompt shows, top to bottom, for checks.
pub(crate) fn shown_lines(dialog: &adw::AlertDialog) -> Vec<String> {
    let mut lines: Vec<String> = dialog.heading().into_iter().map(String::from).collect();
    let mut child = dialog.extra_child().and_then(|c| c.first_child());
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            lines.push(label.label().into());
        }
        child = widget.next_sibling();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gtk::test]
    fn the_install_prompt_lists_chromes_warnings_and_what_linux_lacks() {
        let manifest = Manifest::parse(
            r#"{ "manifest_version": 3, "name": "Reader", "version": "1",
                 "permissions": ["tabs", "webRequest"], "host_permissions": ["https://a.example/*"] }"#,
            &|_| None,
        )
        .unwrap();
        let dialog = install_dialog(&manifest);
        assert_eq!(
            shown_lines(&dialog),
            [
                "Add “Reader”?",
                "It can:",
                "• Read and change your data on a.example",
                "• Read your browsing history",
                "Not supported by the Linux runtime: webRequest. Features that depend on them will not work.",
            ]
        );
        assert_eq!(dialog.default_response().as_deref(), Some("cancel"));
    }

    #[gtk::test]
    fn the_request_prompt_lists_the_sites_behind_a_number_of_websites() {
        let warnings = permissions::warnings(&permissions::PermissionSet::from_manifest_list([
            "https://a.example/*",
            "https://b.example/*",
            "https://c.example/*",
            "https://d.example/*",
        ]));
        let prompt = Prompt { extension: vsesvit_core::extensions::ExtensionId::parse("x@y").unwrap(), name: "Sites".into(), icon: None, warnings };
        assert_eq!(
            shown_lines(&request_dialog(&prompt)),
            [
                "“Sites” has requested additional permissions.",
                "It could:",
                "• Read and change your data on a number of websites",
                "◦ a.example",
                "◦ b.example",
                "◦ c.example",
                "◦ d.example",
            ]
        );
    }
}
