//! How the address bar writes URLs: decoded where that is safe, and simplified.

use vsesvit_core::address::{readable_url, simplified_url};

#[test]
fn hosts_and_paths_read_decoded() {
    for (url, shown) in [
        ("https://uk.wikipedia.org/wiki/%D0%9A%D0%B8%D1%97%D0%B2", "https://uk.wikipedia.org/wiki/Київ"),
        ("https://xn--e1afmkfd.xn--j1amh/", "https://пример.укр/"),
        ("https://xn--bcher-kva.example/", "https://bücher.example/"),
        ("https://example.com/%E2%9C%93?q=%D1%8F#%D0%B0", "https://example.com/✓?q=я#а"),
        ("https://example.com/a%20b%2Fc%3F", "https://example.com/a%20b%2Fc%3F"),
        ("https://example.com/%D0%9A%20%D0%B8", "https://example.com/К%20и"),
        ("https://example.com/%E2%80%AEtxt", "https://example.com/%E2%80%AEtxt"),
        ("https://example.com/pay%C2%ADpal", "https://example.com/pay%C2%ADpal"),
        ("https://example.com/a%E3%85%A4b", "https://example.com/a%E3%85%A4b"),
        ("https://example.com/a%D8%9Cb", "https://example.com/a%D8%9Cb"),
        ("https://example.com/a%E1%A0%8Eb", "https://example.com/a%E1%A0%8Eb"),
        ("https://example.com/a%EF%B8%8Fb", "https://example.com/a%EF%B8%8Fb"),
        ("https://example.com/a%F3%A0%80%81b", "https://example.com/a%F3%A0%80%81b"),
        ("https://example.com/%D0%9A%C2%AD", "https://example.com/К%C2%AD"),
        ("https://example.com/%FF%FE", "https://example.com/%FF%FE"),
        ("https://example.com/100%", "https://example.com/100%"),
        ("https://user@xn--bcher-kva.example/", "https://user@bücher.example/"),
        ("file:///C:/%D0%B4%D0%BE%D0%BA.txt", "file:///C:/док.txt"),
        ("data:text/plain,%D0%B2", "data:text/plain,в"),
        ("about:blank", "about:blank"),
        ("", ""),
    ] {
        assert_eq!(readable_url(url), shown, "{url}");
    }
}

#[test]
fn lookalike_hosts_keep_their_punycode() {
    // Cyrillic "аррӏе", all letters that look Latin: shown as punycode, like Chrome does.
    let apple = format!("https://{}/", idna_encode("аррӏе.com"));
    assert_eq!(readable_url(&apple), apple);
    // Mixed Latin and Cyrillic in one label.
    let mixed = format!("https://{}/", idna_encode("pаypal.com"));
    assert_eq!(readable_url(&mixed), mixed);
}

#[test]
fn more_lookalike_hosts_keep_their_punycode() {
    for host in [
        // Cyrillic and Latin letters that imitate other Latin ones.
        "ѵіѕа.com",
        "ınstagram.com",
        "ǀinkedin.com",
        "ɡoogle.com",
        // IPA and Latin Extended Additional letters, with a-z or alone.
        "pạypal.com",
        "ɑpple.com",
        "goọgle.com",
        "ạṗṗḷẹ.com",
        // Armenian lookalikes, alone and mixed with Latin ones from outside a-z.
        "օօց.com",
        "ɡօօɡḷẹ.com",
        "ẹxаmple.com",
        // Two scripts other than Latin, Greek and Cyrillic; a script not used for names.
        "աბ.com",
        "ꭰꮪꮃ.com",
    ] {
        let url = format!("https://{}/", idna_encode(host));
        assert_eq!(readable_url(&url), url, "{host}");
    }
}

#[test]
fn names_in_one_script_read_decoded() {
    for host in [
        "ελληνικά.gr",
        "հայաստան.am",
        "ישראל.il",
        "مصر.eg",
        "भारत.in",
        "ไทย.th",
        "საქართველო.ge",
        "例子.中国",
        "ドメイン名例.jp",
        "らーめん.jp",
        "한국.kr",
        "straße.de",
    ] {
        let url = format!("https://{}/", idna_encode(host));
        assert_eq!(readable_url(&url), format!("https://{host}/"), "{host}");
    }
}

fn idna_encode(host: &str) -> String {
    vsesvit_core::Url::parse(&format!("https://{host}/")).unwrap().host_str().unwrap().to_owned()
}

#[test]
fn simplified_urls_drop_https_www_and_a_bare_slash() {
    for (shown, simplified) in [
        ("https://www.example.com/", "example.com"),
        ("https://example.com/", "example.com"),
        ("https://example.com/a/b?q=1#top", "example.com/a/b?q=1#top"),
        ("https://www.example.com/?q=1", "example.com/?q=1"),
        ("https://example.com/a/", "example.com/a/"),
        ("https://www.com/", "www.com"),
        ("https://sub.www.example.com/", "sub.www.example.com"),
        ("https://example.com:8443/", "example.com:8443"),
        ("https://www.пример.укр/", "пример.укр"),
        ("https://uk.wikipedia.org/wiki/Київ", "uk.wikipedia.org/wiki/Київ"),
        ("https://user@example.com/", "https://user@example.com/"),
        ("http://example.com/", "http://example.com/"),
        ("file:///C:/notes/a.html", "file:///C:/notes/a.html"),
        ("about:blank", "about:blank"),
        ("", ""),
    ] {
        assert_eq!(simplified_url(shown), simplified, "{shown}");
    }
}
