//! `InstallSource::parse`: what the user typed or pasted, classified once.

use std::path::{Path, PathBuf};

use vsesvit_core::extensions::{ExtensionId, InstallSource, SourceParseError, StoreRef};

const UBO_LITE: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

fn cws(id: &str) -> InstallSource {
    InstallSource::ChromeWebStore { id: ExtensionId::parse(id).unwrap() }
}

fn amo(slug: &str) -> InstallSource {
    InstallSource::Amo { slug_or_guid: slug.to_owned() }
}

#[test]
fn chrome_web_store_urls_and_ids() {
    for input in [
        UBO_LITE,
        "  ddkjiahejlhfcafbddmgiahcphecmpfh\n",
        "https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh",
        "https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh?hl=uk&pli=1",
        "https://chromewebstore.google.com/detail/ddkjiahejlhfcafbddmgiahcphecmpfh",
        "https://chromewebstore.google.com/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh/reviews",
        "https://chrome.google.com/webstore/detail/ublock-origin-lite/ddkjiahejlhfcafbddmgiahcphecmpfh",
        "https://chrome.google.com/webstore/detail/ddkjiahejlhfcafbddmgiahcphecmpfh#details",
        "http://CHROMEWEBSTORE.google.com./detail/x/ddkjiahejlhfcafbddmgiahcphecmpfh",
    ] {
        assert_eq!(InstallSource::parse(input).unwrap(), cws(UBO_LITE), "{input:?}");
    }
    assert!(matches!(InstallSource::parse("https://chromewebstore.google.com/detail/ublock-origin-lite"), Err(SourceParseError::BadId)));
    assert!(matches!(
        InstallSource::parse("https://chromewebstore.google.com/detail/x/ddkjiahejlhfcafbddmgiahcphecmpfz"),
        Err(SourceParseError::BadId)
    ));
    assert!(matches!(InstallSource::parse("https://chromewebstore.google.com/category/extensions"), Err(SourceParseError::Unrecognized)));
    assert!(matches!(
        InstallSource::parse("https://example.com/detail/x/ddkjiahejlhfcafbddmgiahcphecmpfh"),
        Err(SourceParseError::Unrecognized)
    ));
    assert_eq!(cws(UBO_LITE).store(), Some(StoreRef::ChromeWebStore));
}

#[test]
fn amo_urls_and_gecko_ids() {
    for (input, slug) in [
        ("https://addons.mozilla.org/en-US/firefox/addon/ublock-origin/", "ublock-origin"),
        ("https://addons.mozilla.org/uk/firefox/addon/ublock-origin/?utm_source=x", "ublock-origin"),
        ("https://addons.mozilla.org/firefox/addon/ublock-origin", "ublock-origin"),
        ("https://addons.mozilla.org/en-US/android/addon/ublock-origin/reviews/", "ublock-origin"),
        (
            "https://addons.mozilla.org/en-US/firefox/addon/%7Bd10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d%7D/",
            "{d10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d}",
        ),
        ("uBlock0@raymondhill.net", "uBlock0@raymondhill.net"),
        ("{d10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d}", "{d10d0bf8-f5b5-c8b4-a8b2-2b9879e08c5d}"),
    ] {
        assert_eq!(InstallSource::parse(input).unwrap(), amo(slug), "{input:?}");
    }
    assert!(matches!(InstallSource::parse("https://addons.mozilla.org/en-US/firefox/addon/"), Err(SourceParseError::BadId)));
    assert!(matches!(InstallSource::parse("https://addons.mozilla.org/en-US/firefox/addon/a%2Fb/"), Err(SourceParseError::BadId)));
    assert!(matches!(InstallSource::parse("https://addons.mozilla.org/en-US/firefox/"), Err(SourceParseError::Unrecognized)));
    assert_eq!(amo("x").store(), Some(StoreRef::Amo));
}

#[test]
fn files_and_directories() {
    let abs = |p: &str| std::path::absolute(p).unwrap();
    assert_eq!(InstallSource::parse("probe.crx").unwrap(), InstallSource::CrxFile { path: abs("probe.crx") });
    assert_eq!(InstallSource::parse("dir/Add-On.XPI").unwrap(), InstallSource::XpiFile { path: abs("dir/Add-On.XPI") });
    assert_eq!(InstallSource::parse(r"C:\Downloads\x.crx").unwrap().store(), None);
    assert!(matches!(InstallSource::parse(r"C:\Downloads\x.crx").unwrap(), InstallSource::CrxFile { .. }));

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let probe: PathBuf = repo.join("tests").join("fixtures").join("extensions").join("probe");
    assert_eq!(InstallSource::parse(probe.to_str().unwrap()).unwrap(), InstallSource::Unpacked { dir: probe.clone() });
    // Every spelling of the dir reaches the same path, so an unpacked id is stable.
    let dotted = format!("{}/./../../tests/fixtures/extensions/probe", env!("CARGO_MANIFEST_DIR"));
    assert_eq!(InstallSource::parse(&dotted).unwrap(), InstallSource::Unpacked { dir: probe.clone() });
    assert_eq!(InstallSource::from_path(&probe.join("manifest.json")).unwrap(), InstallSource::Unpacked { dir: probe.clone() });
    let file_url = vsesvit_core::Url::from_file_path(&probe).unwrap();
    assert_eq!(InstallSource::parse(file_url.as_str()).unwrap(), InstallSource::Unpacked { dir: probe.clone() });

    for bad in ["", "   ", "ublock-origin", "no-such-dir", "https://example.com/", "ftp://x/y.crx"] {
        assert!(matches!(InstallSource::parse(bad), Err(SourceParseError::Unrecognized)), "{bad:?}");
    }
}
