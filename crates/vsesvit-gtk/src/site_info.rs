//! The connection popover that the security icon at the start of the address bar opens, as
//! in Chrome: whether the connection is secure and to which host, and for HTTPS the
//! certificate WebKit checked (who it was issued to and by, when it is valid, the names it
//! covers, its SHA-256 fingerprint) with the chain up to the root. WebKit hands over the
//! certificates as `GTlsCertificate`s; core parses their DER. Tracking protection's switch for
//! the site ([`crate::trackers::site_info_section`]) and what the site may use
//! ([`crate::permissions::site_info_section`]) come between the summary and the certificate.

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::Url;
use vsesvit_core::certificate::{self, Certificate, Name};
use webkit::prelude::*;

use crate::address_bar::Security;
use crate::tab::Tab;

/// Chains longer than this are cut; real ones have three or four certificates.
const MAX_CHAIN: usize = 10;
const WIDTH: i32 = 380;

/// What the popover says about the selected tab's page.
pub(crate) struct Connection {
    security: Security,
    host: String,
    /// For HTTPS, what WebKit reports about the certificate.
    tls: Option<Tls>,
}

struct Tls {
    /// The server's certificate first, then each issuer; a certificate core cannot parse
    /// keeps its place with the reason.
    chain: Vec<Result<Certificate, String>>,
    problems: Vec<&'static str>,
}

impl Connection {
    pub(crate) fn of(tab: &Tab) -> Self {
        let uri = tab.committed_uri();
        let url = uri.as_deref().and_then(|uri| Url::parse(uri).ok());
        let host = url.as_ref().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| uri.clone().unwrap_or_default());
        let tls = tab
            .web_view()
            .tls_info()
            .filter(|_| url.as_ref().is_some_and(|u| u.scheme() == "https"))
            .map(|(leaf, flags)| Tls { chain: chain(leaf), problems: problems(flags) });
        Connection { security: tab.security(), host, tls }
    }
}

fn chain(leaf: gio::TlsCertificate) -> Vec<Result<Certificate, String>> {
    std::iter::successors(Some(leaf), gio::TlsCertificate::issuer)
        .take(MAX_CHAIN)
        .map(|cert| {
            let der = cert.certificate().ok_or_else(|| "no DER data".to_owned())?;
            certificate::parse_der(&der).map_err(|e| e.to_string())
        })
        .collect()
}

/// What each verification flag means, in the words the popover uses.
const PROBLEMS: &[(gio::TlsCertificateFlags, &str)] = &[
    (gio::TlsCertificateFlags::UNKNOWN_CA, "It is not issued by an authority this device trusts."),
    (gio::TlsCertificateFlags::BAD_IDENTITY, "It is not valid for this site's name."),
    (gio::TlsCertificateFlags::NOT_ACTIVATED, "It is not valid yet."),
    (gio::TlsCertificateFlags::EXPIRED, "It has expired."),
    (gio::TlsCertificateFlags::REVOKED, "It has been revoked."),
    (gio::TlsCertificateFlags::INSECURE, "It uses an insecure algorithm."),
    (gio::TlsCertificateFlags::GENERIC_ERROR, "It could not be verified."),
];

fn problems(flags: gio::TlsCertificateFlags) -> Vec<&'static str> {
    PROBLEMS.iter().filter(|(flag, _)| flags.contains(*flag)).map(|(_, text)| *text).collect()
}

/// The headline, its icon and the explanation under it.
fn summary(connection: &Connection) -> (&'static str, &'static str, &'static str) {
    let certificate_bad = connection.tls.as_ref().is_some_and(|tls| !tls.problems.is_empty());
    match connection.security {
        Security::Secure if !certificate_bad => (
            "Connection is secure",
            "channel-secure-symbolic",
            "What you send to this site, such as passwords or card numbers, is private.",
        ),
        Security::Secure | Security::Insecure => (
            "Connection is not secure",
            "channel-insecure-symbolic",
            "Do not enter passwords or card numbers on this site: others on the network could read or change them.",
        ),
        Security::Internal => ("This is a local or built-in page", "dialog-information-symbolic", "It was not loaded over the network."),
        Security::NotApplicable => ("No connection details", "dialog-information-symbolic", "This page was not loaded from a site."),
    }
}

/// A local date with the month by name, which reads the same in every locale.
fn date(unix: i64) -> String {
    glib::DateTime::from_unix_local(unix)
        .ok()
        .and_then(|t| t.format("%-d %b %Y").ok())
        .map_or_else(|| unix.to_string(), String::from)
}

/// The rows under a name: its common name and organization, as Chrome's viewer shows them.
fn name_rows(name: &Name) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    if let Some(cn) = &name.common_name {
        rows.push(("Common name", cn.clone()));
    }
    if let Some(o) = &name.organization {
        rows.push(("Organization", o.clone()));
    }
    if let Some(ou) = &name.organizational_unit {
        rows.push(("Unit", ou.clone()));
    }
    if rows.is_empty() {
        rows.push(("Name", name.display()));
    }
    rows
}

/// The certificate section's groups of rows, in order.
fn certificate_groups(cert: &Certificate) -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    vec![
        ("Issued to", name_rows(&cert.subject)),
        ("Issued by", name_rows(&cert.issuer)),
        ("Validity", vec![("Valid from", date(cert.not_before_unix)), ("Valid until", date(cert.not_after_unix))]),
        (
            "Details",
            vec![
                ("Public key", cert.public_key.clone()),
                ("Signature", cert.signature_algorithm.clone()),
                ("Serial number", cert.serial_hex.clone()),
            ],
        ),
    ]
}

/// `trackers` and `permissions` are the page's tracking protection and Permissions sections,
/// when it has them.
pub(crate) fn popover(connection: &Connection, trackers: Option<&gtk::ListBox>, permissions: Option<&gtk::Box>) -> gtk::Popover {
    let (title, icon, explanation) = summary(connection);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .width_request(WIDTH)
        .build();
    let headline = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    headline.append(&gtk::Image::from_icon_name(icon));
    headline.append(&gtk::Label::builder().label(title).xalign(0.0).css_classes(["heading"]).build());
    content.append(&headline);
    if !connection.host.is_empty() {
        content.append(&text(&connection.host, &["dim-label"]));
    }
    content.append(&text(explanation, &[]));
    if let Some(trackers) = trackers {
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(trackers);
    }
    if let Some(permissions) = permissions {
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(permissions);
    }
    if let Some(tls) = &connection.tls {
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&tls_section(tls));
    }
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(680)
        .child(&content)
        .build();
    gtk::Popover::builder().child(&scroller).css_classes(["site-info"]).build()
}

fn tls_section(tls: &Tls) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.append(&gtk::Label::builder().label("Certificate").xalign(0.0).css_classes(["heading"]).build());
    if tls.problems.is_empty() {
        section.append(&text("Valid: issued by a trusted authority for this site.", &["success"]));
    }
    for problem in &tls.problems {
        section.append(&text(problem, &["error"]));
    }
    match tls.chain.first() {
        Some(Ok(leaf)) => {
            let grid = gtk::Grid::builder().row_spacing(4).column_spacing(12).build();
            let mut row = 0;
            for (group, rows) in certificate_groups(leaf) {
                grid.attach(&gtk::Label::builder().label(group).xalign(0.0).css_classes(["caption-heading"]).margin_top(4).build(), 0, row, 2, 1);
                row += 1;
                for (label, value) in rows {
                    grid.attach(&gtk::Label::builder().label(label).xalign(0.0).valign(gtk::Align::Start).css_classes(["dim-label"]).build(), 0, row, 1, 1);
                    grid.attach(&value_label(&value, false), 1, row, 1, 1);
                    row += 1;
                }
            }
            section.append(&grid);
            section.append(&gtk::Label::builder().label("SHA-256 fingerprint").xalign(0.0).css_classes(["caption-heading"]).build());
            section.append(&value_label(&leaf.sha256_hex, true));
            section.append(&names_expander(&leaf.dns_names));
        }
        Some(Err(e)) => section.append(&text(&format!("The certificate could not be read: {e}"), &["error"])),
        None => {}
    }
    if tls.chain.len() > 1 {
        section.append(&gtk::Label::builder().label("Certificate chain").xalign(0.0).css_classes(["caption-heading"]).build());
        for (depth, cert) in tls.chain.iter().enumerate() {
            let subject = cert.as_ref().map_or_else(|e| format!("(unreadable: {e})"), |c| c.subject.display());
            let label = value_label(&subject, false);
            label.set_margin_start(i32::try_from(depth).unwrap_or(0) * 12);
            section.append(&label);
        }
    }
    section
}

/// The names the certificate covers, collapsed as Chrome's viewer lists them.
fn names_expander(names: &[String]) -> gtk::Expander {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    for name in names {
        list.append(&value_label(name, false));
    }
    gtk::Expander::builder()
        .label(format!("Subject alternative names ({})", names.len()))
        .child(&list)
        .sensitive(!names.is_empty())
        .build()
}

fn text(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).wrap(true).max_width_chars(48).css_classes(classes.to_vec()).build()
}

fn value_label(value: &str, monospace: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(value)
        .xalign(0.0)
        .hexpand(true)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(44)
        .build();
    if monospace {
        label.add_css_class("monospace");
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(cn: Option<&str>, o: Option<&str>) -> Name {
        Name { common_name: cn.map(str::to_owned), organization: o.map(str::to_owned), ..Name::default() }
    }

    #[test]
    fn flags_become_the_problems_they_mean() {
        assert!(problems(gio::TlsCertificateFlags::empty()).is_empty());
        let both = problems(gio::TlsCertificateFlags::EXPIRED | gio::TlsCertificateFlags::UNKNOWN_CA);
        assert_eq!(both, ["It is not issued by an authority this device trusts.", "It has expired."]);
    }

    #[test]
    fn a_bad_certificate_is_never_called_secure() {
        let connection = |security, problems: Vec<&'static str>| Connection {
            security,
            host: "example.com".to_owned(),
            tls: Some(Tls { chain: Vec::new(), problems }),
        };
        assert_eq!(summary(&connection(Security::Secure, Vec::new())).0, "Connection is secure");
        assert_eq!(summary(&connection(Security::Secure, vec!["It has expired."])).0, "Connection is not secure");
        assert_eq!(summary(&connection(Security::Insecure, Vec::new())).0, "Connection is not secure");
    }

    #[test]
    fn names_show_their_common_name_and_organization() {
        assert_eq!(name_rows(&name(Some("example.com"), Some("Example Inc."))), [("Common name", "example.com".to_owned()), ("Organization", "Example Inc.".to_owned())]);
        assert_eq!(name_rows(&name(None, None)), [("Name", "Unknown".to_owned())]);
    }
}
