//! The popup of the security icon at the start of the address bar: whether the connection is
//! secure, the host, the site's permissions (filled by the window), the TLS parameters and the
//! site's certificate chain, and Chrome's "Show certificate" button, which opens the Windows
//! certificate viewer.
//!
//! Each tab keeps the engine's last `Security.visibleSecurityStateChanged` report (the DevTools
//! protocol's view of the page's connection: the TLS parameters and the chain as base64 DER);
//! `vsesvit_core::certificate::parse_der` reads the certificates.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use vsesvit_core::certificate::{self, Certificate};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::xaml;

/// The engine's view of a page's connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// `secure`, `insecure`, `insecure-broken`, `neutral` or `info`.
    pub state: String,
    pub tls: Option<Tls>,
    /// The chain, leaf first, as DER.
    pub chain: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tls {
    pub protocol: String,
    pub key_exchange: String,
    pub cipher: String,
}

/// A `Security.visibleSecurityStateChanged` event's parameters.
pub(crate) fn parse_report(json: &str) -> Option<Report> {
    let value: Value = serde_json::from_str(json).ok()?;
    let visible = value.get("visibleSecurityState")?;
    let state = visible.get("securityState")?.as_str()?.to_owned();
    let certificate = visible.get("certificateSecurityState");
    let text = |key: &str| {
        certificate
            .and_then(|c| c.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let tls = certificate.map(|_| {
        let (exchange, group) = (text("keyExchange"), text("keyExchangeGroup"));
        let key_exchange = match (exchange.is_empty(), group.is_empty()) {
            (false, false) => format!("{exchange} with {group}"),
            (true, _) => group,
            (false, true) => exchange,
        };
        let (cipher, mac) = (text("cipher"), text("mac"));
        Tls {
            protocol: text("protocol"),
            key_exchange,
            cipher: if mac.is_empty() {
                cipher
            } else {
                format!("{cipher} with {mac}")
            },
        }
    });
    let chain = certificate
        .and_then(|c| c.get("certificate"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| STANDARD.decode(c.as_str()?).ok())
                .collect()
        })
        .unwrap_or_default();
    Some(Report { state, tls, chain })
}

/// The popup's headline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Headline {
    Secure,
    NotSecure,
    /// A page on this device or inside the browser.
    Local,
}

/// The URL whose origin a page has: the inner URL of a blob: or filesystem: URL, which the
/// page that made it shares, or `url` itself.
pub(crate) fn origin_url(url: &str) -> &str {
    ["blob:", "filesystem:"]
        .iter()
        .find(|prefix| {
            url.get(..prefix.len())
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case(prefix))
        })
        .map_or(url, |prefix| &url[prefix.len()..])
}

impl Headline {
    /// By the scheme of the page's origin, so a blob: page an http site made is not secure.
    pub fn of(url: &str, report: Option<&Report>) -> Self {
        match origin_url(url).split_once(':').map(|(scheme, _)| scheme) {
            Some("https") if report.is_none_or(|r| r.state == "secure") => Headline::Secure,
            Some("https" | "http") => Headline::NotSecure,
            _ => Headline::Local,
        }
    }

    fn text(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Headline::Secure => (
                "\u{E72E}",
                "Connection is secure",
                "Your information (for example, passwords or credit card numbers) is private \
                 when it is sent to this site.",
            ),
            Headline::NotSecure => (
                "\u{E7BA}",
                "Connection is not secure",
                "You should not enter any sensitive information on this site (for example, \
                 passwords or credit cards), because it could be stolen by attackers.",
            ),
            Headline::Local => (
                "\u{E8A5}",
                "This page is on your device or inside the browser",
                "It did not come over the network.",
            ),
        }
    }
}

/// A certificate of the chain, parsed, or why it could not be.
pub(crate) type Parsed = std::result::Result<Certificate, String>;

pub(crate) fn parse_chain(chain: &[Vec<u8>]) -> Vec<Parsed> {
    chain
        .iter()
        .map(|der| certificate::parse_der(der).map_err(|e| e.to_string()))
        .collect()
}

/// A date as the user's locale writes it, in their time zone.
fn local_date(unix: i64) -> String {
    // Windows.Foundation.DateTime counts 100 ns ticks since 1601.
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
    let time = DateTime {
        universal_time: unix
            .saturating_mul(10_000_000)
            .saturating_add(UNIX_EPOCH_TICKS),
    };
    DateTimeFormatter::CreateDateTimeFormatter("longdate")
        .and_then(|f| f.Format(time))
        .unwrap_or_else(|_| format!("{unix} (Unix time)"))
}

/// The popup's content for the page at `url` on `host`.
pub(crate) fn content(url: &str, host: &str, report: Option<&Report>) -> Result<FrameworkElement> {
    let headline = Headline::of(url, report);
    let (glyph, title, explanation) = headline.text();
    let mut body = String::new();
    if !host.is_empty() {
        body.push_str(&format!(
            r#"<TextBlock Text="{}" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}" TextWrapping="Wrap"/>"#,
            xaml::escape(host)
        ));
    }
    body.push_str(&format!(
        r#"<TextBlock Text="{}" TextWrapping="Wrap" Style="{{StaticResource CaptionTextBlockStyle}}"/>"#,
        xaml::escape(explanation)
    ));
    body.push_str(r#"<StackPanel x:Name="SitePermissions" Visibility="Collapsed"/>"#);
    let report = report.filter(|_| headline != Headline::Local);
    if let Some(tls) = report.and_then(|r| r.tls.as_ref()) {
        body.push_str(&format!(
            r#"<TextBlock x:Name="ConnectionTls" TextWrapping="Wrap" Style="{{StaticResource CaptionTextBlockStyle}}"
                 Text="The connection is encrypted and authenticated using {}, {} and {}."/>"#,
            xaml::escape(&tls.protocol),
            xaml::escape(&tls.key_exchange),
            xaml::escape(&tls.cipher)
        ));
    }
    let chain = report.map(|r| parse_chain(&r.chain)).unwrap_or_default();
    if let Some(leaf) = chain.first() {
        body.push_str(&certificate_markup(leaf, &chain));
    }
    xaml::load(&format!(
        r#"<ScrollViewer {{ns}} MaxHeight="560" Width="380" VerticalScrollBarVisibility="Auto">
             <StackPanel Spacing="8" Padding="0,0,12,0">
               <StackPanel Orientation="Horizontal" Spacing="10">
                 <FontIcon Glyph="{glyph}" FontSize="16"/>
                 <TextBlock x:Name="ConnectionTitle" Text="{title}" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
               </StackPanel>
               {body}
             </StackPanel>
           </ScrollViewer>"#
    ))
}

fn certificate_markup(leaf: &Parsed, chain: &[Parsed]) -> String {
    let leaf = match leaf {
        Ok(leaf) => leaf,
        Err(e) => {
            return format!(
                r#"<TextBlock Text="The certificate could not be read: {}" TextWrapping="Wrap"/>"#,
                xaml::escape(e)
            );
        }
    };
    // Values are escaped already: a name's lines are joined by a markup line break.
    let row = |label: &str, value: &str, row: usize| {
        format!(
            r#"<TextBlock Grid.Row="{row}" Text="{label}" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
               <TextBlock Grid.Row="{row}" Grid.Column="1" Text="{value}" TextWrapping="Wrap" IsTextSelectionEnabled="True"/>"#
        )
    };
    let name = |n: &certificate::Name| match (&n.common_name, &n.organization) {
        (Some(cn), Some(o)) => format!("{}&#10;{}", xaml::escape(cn), xaml::escape(o)),
        _ => xaml::escape(&n.display()),
    };
    let rows = [
        ("Issued to", name(&leaf.subject)),
        ("Issued by", name(&leaf.issuer)),
        (
            "Valid from",
            xaml::escape(&local_date(leaf.not_before_unix)),
        ),
        (
            "Valid until",
            xaml::escape(&local_date(leaf.not_after_unix)),
        ),
    ];
    let grid: String = rows
        .iter()
        .enumerate()
        .map(|(i, (label, value))| row(label, value, i))
        .collect();
    let names: String = leaf
        .dns_names
        .iter()
        .map(|n| {
            format!(
                r#"<TextBlock Text="{}" IsTextSelectionEnabled="True"/>"#,
                xaml::escape(n)
            )
        })
        .collect();
    let links: String = chain
        .iter()
        .enumerate()
        .map(|(depth, c)| {
            let subject = c.as_ref().map_or_else(
                |_| "Unreadable certificate".to_owned(),
                |c| c.subject.display(),
            );
            format!(
                r#"<TextBlock Text="{}" Margin="{},0,0,0"/>"#,
                xaml::escape(&subject),
                depth * 14
            )
        })
        .collect();
    format!(
        r#"<Border Height="1" Margin="0,4" Background="{{ThemeResource DividerStrokeColorDefaultBrush}}"/>
           <TextBlock Text="Certificate" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
           <Grid x:Name="ConnectionCertificate" ColumnSpacing="12" RowSpacing="4">
             <Grid.ColumnDefinitions><ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/></Grid.ColumnDefinitions>
             <Grid.RowDefinitions><RowDefinition/><RowDefinition/><RowDefinition/><RowDefinition/></Grid.RowDefinitions>
             {grid}
           </Grid>
           <Expander Header="Subject alternative names ({count})" HorizontalAlignment="Stretch"
                     HorizontalContentAlignment="Left">
             <StackPanel Spacing="2">{names}</StackPanel>
           </Expander>
           <TextBlock Text="SHA-256 fingerprint" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
           <TextBlock Text="{fingerprint}" FontFamily="Cascadia Mono, Consolas" FontSize="11" TextWrapping="Wrap"
                      IsTextSelectionEnabled="True"/>
           <TextBlock Text="Certificate chain" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
           <StackPanel x:Name="ConnectionChain" Spacing="2">{links}</StackPanel>
           <Button x:Name="ShowCertificate" Content="Show certificate" Margin="0,4,0,0"/>"#,
        count = leaf.dns_names.len(),
        fingerprint = xaml::escape(&leaf.sha256_hex),
    )
}

/// The Windows certificate viewer for `chain` (leaf first), owned by `owner`, as Chrome on
/// Windows shows it. It runs its own modal loop, so it gets a thread of its own; the rest of
/// the chain goes into the leaf's store, where the viewer builds the certification path from.
pub(crate) fn show_native(owner: HWND, chain: Vec<Vec<u8>>) {
    let owner = owner as usize;
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let store = CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, std::ptr::null());
        if store.is_null() {
            log::warn!("certificate viewer: no memory store");
            return;
        }
        let encoding = (X509_ASN_ENCODING | PKCS_7_ASN_ENCODING) as u32;
        let mut leaf: PCCERT_CONTEXT = std::ptr::null();
        for (index, der) in chain.iter().enumerate() {
            let out = if index == 0 {
                &raw mut leaf
            } else {
                std::ptr::null_mut()
            };
            let added = CertAddEncodedCertificateToStore(
                store,
                encoding,
                der.as_ptr(),
                der.len() as u32,
                CERT_STORE_ADD_ALWAYS as u32,
                out,
            );
            if !added.as_bool() {
                log::warn!("certificate viewer: certificate {index} was not added");
            }
        }
        if !leaf.is_null() {
            let _ = CryptUIDlgViewContext(
                CERT_STORE_CERTIFICATE_CONTEXT as u32,
                leaf.cast(),
                owner as HWND,
                windows_core::w!("Certificate"),
                0,
                std::ptr::null(),
            );
            let _ = CertFreeCertificateContext(leaf);
        }
        let _ = CertCloseStore(store, 0);
    });
}

/// A flyout holding `content`, anchored under the security icon.
pub(crate) fn flyout(content: &FrameworkElement) -> Result<Flyout> {
    let flyout: Flyout = xaml::load(r#"<Flyout {ns} Placement="BottomEdgeAlignedLeft"/>"#)?;
    flyout.SetContent(&content.cast::<UIElement>()?)?;
    Ok(flyout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_gives_the_tls_parameters_and_the_chain() {
        let json = r#"{"visibleSecurityState":{"securityState":"secure","securityStateIssueIds":[],
            "certificateSecurityState":{"protocol":"TLS 1.3","keyExchange":"","keyExchangeGroup":"X25519",
            "cipher":"AES_128_GCM","certificate":["Zm9v","YmFy"],"subjectName":"example.com"}}}"#;
        let report = parse_report(json).unwrap();
        assert_eq!(report.state, "secure");
        assert_eq!(
            report.tls,
            Some(Tls {
                protocol: "TLS 1.3".into(),
                key_exchange: "X25519".into(),
                cipher: "AES_128_GCM".into(),
            })
        );
        assert_eq!(report.chain, [b"foo".to_vec(), b"bar".to_vec()]);
    }

    #[test]
    fn a_malformed_certificate_is_left_out_of_the_chain() {
        let json = r#"{"visibleSecurityState":{"securityState":"secure",
            "certificateSecurityState":{"protocol":"TLS 1.3","certificate":["Zm9v","Zm9vY","Zm=9v"]}}}"#;
        assert_eq!(parse_report(json).unwrap().chain, [b"foo".to_vec()]);
    }

    #[test]
    fn tls_1_2_names_the_exchange_its_group_and_the_mac() {
        let json = r#"{"visibleSecurityState":{"securityState":"secure","certificateSecurityState":
            {"protocol":"TLS 1.2","keyExchange":"ECDHE_RSA","keyExchangeGroup":"X25519",
            "cipher":"AES_256_CBC","mac":"HMAC-SHA1","certificate":[]}}}"#;
        let tls = parse_report(json).unwrap().tls.unwrap();
        assert_eq!(tls.key_exchange, "ECDHE_RSA with X25519");
        assert_eq!(tls.cipher, "AES_256_CBC with HMAC-SHA1");
    }

    #[test]
    fn a_plain_http_report_has_no_connection_details() {
        let json =
            r#"{"visibleSecurityState":{"securityState":"insecure","securityStateIssueIds":[]}}"#;
        let report = parse_report(json).unwrap();
        assert_eq!((report.tls, report.chain.len()), (None, 0));
    }

    #[test]
    fn headlines_follow_the_scheme_and_the_engines_verdict() {
        let secure = Report {
            state: "secure".into(),
            ..Report::default()
        };
        let broken = Report {
            state: "insecure-broken".into(),
            ..Report::default()
        };
        assert_eq!(
            Headline::of("https://a.test/", Some(&secure)),
            Headline::Secure
        );
        assert_eq!(
            Headline::of("https://a.test/", Some(&broken)),
            Headline::NotSecure
        );
        assert_eq!(Headline::of("http://a.test/", None), Headline::NotSecure);
        assert_eq!(Headline::of("file:///C:/a.html", None), Headline::Local);
        assert_eq!(Headline::of("data:text/html,x", None), Headline::Local);
    }

    #[test]
    fn blob_and_filesystem_pages_take_their_creators_headline() {
        let secure = Report {
            state: "secure".into(),
            ..Report::default()
        };
        assert_eq!(
            Headline::of("blob:http://evil.test/1b2c", None),
            Headline::NotSecure
        );
        assert_eq!(
            Headline::of("filesystem:http://evil.test/temporary/a.html", None),
            Headline::NotSecure
        );
        assert_eq!(
            Headline::of("BLOB:https://a.test/1b2c", Some(&secure)),
            Headline::Secure
        );
        assert_eq!(Headline::of("blob:null/1b2c", None), Headline::Local);
        assert_eq!(
            origin_url("blob:https://a.test/1b2c"),
            "https://a.test/1b2c"
        );
        assert_eq!(origin_url("https://a.test/"), "https://a.test/");
    }
}
