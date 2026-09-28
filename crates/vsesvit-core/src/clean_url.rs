//! The link "copy clean link" puts on the clipboard: [`clean`] drops the query parameters that
//! only track who shared or clicked a link, and unwraps the redirects search engines and social
//! sites wrap outgoing links in. The rules are data: [`GLOBAL`] applies on every site, [`SITES`]
//! only on the sites that use a name for tracking, [`REDIRECTS`] names the wrappers.
//!
//! A rule belongs here only when removing it never changes what the page shows. A missed tracker
//! costs less than a broken link.

use url::form_urlencoded;

use crate::Url;

/// A query parameter name, matched ignoring ASCII case after percent-decoding.
#[derive(Clone, Copy)]
enum Param {
    Exact(&'static str),
    Prefix(&'static str),
}

use Param::{Exact, Prefix};

impl Param {
    fn matches(self, name: &str) -> bool {
        match self {
            Exact(p) => name.eq_ignore_ascii_case(p),
            Prefix(p) => name.get(..p.len()).is_some_and(|head| head.eq_ignore_ascii_case(p)),
        }
    }
}

/// Which hosts a rule covers.
#[derive(Clone, Copy)]
enum Domain {
    /// The domain and its subdomains: `Suffix("youtube.com")` covers `m.youtube.com`.
    Suffix(&'static str),
    /// A brand under any country domain, subdomains included: `Brand("google")` covers
    /// `google.de`, `www.google.co.uk` and `google.com.au`, not `google.example.com`.
    Brand(&'static str),
}

use Domain::{Brand, Suffix};

impl Domain {
    fn matches(self, host: &str) -> bool {
        match self {
            Suffix(s) => host.strip_suffix(s).is_some_and(|rest| rest.is_empty() || rest.ends_with('.')),
            Brand(b) => {
                let labels: Vec<&str> = host.split('.').collect();
                let Some(at) = labels.iter().position(|l| *l == b) else {
                    return false;
                };
                match &labels[at + 1..] {
                    [_] => true,
                    [second, _] => matches!(*second, "co" | "com"),
                    _ => false,
                }
            }
        }
    }
}

/// Tracking parameters a site adds to its own links.
struct Site {
    domains: &'static [Domain],
    params: &'static [Param],
    /// A last path segment starting with this is tracking too: Amazon's `/dp/B0…/ref=sr_1_1`.
    path_tail: Option<&'static str>,
}

/// A link that only forwards to another: the target is the first of `targets` present.
struct Redirect {
    domains: &'static [Domain],
    path: &'static str,
    targets: &'static [&'static str],
}

/// Click and campaign identifiers of ad networks, analytics and mailing tools, on any site.
const GLOBAL: &[Param] = &[
    Prefix("utm_"),
    Exact("fbclid"),
    Exact("gclid"),
    Exact("dclid"),
    Exact("gbraid"),
    Exact("wbraid"),
    Exact("msclkid"),
    Exact("yclid"),
    Exact("twclid"),
    Exact("ttclid"),
    Exact("li_fat_id"),
    Exact("igshid"),
    Exact("mc_cid"),
    Exact("mc_eid"),
    Exact("_ga"),
    Exact("_gl"),
    Exact("_hsenc"),
    Exact("_hsmi"),
    Exact("mkt_tok"),
    Exact("oly_anon_id"),
    Exact("oly_enc_id"),
    Exact("vero_id"),
    Exact("rb_clickid"),
    Exact("wickedid"),
    Exact("srsltid"),
    Exact("sc_cid"),
    Exact("spm"),
];

/// Share and referral markers of single sites. YouTube's `t` (start time), Spotify's `context`,
/// Amazon's `keywords`, `th` and `psc` (the chosen variant) and Google's `q` change the page and
/// stay.
const SITES: &[Site] = &[
    Site {
        domains: &[Suffix("youtube.com"), Suffix("youtu.be")],
        params: &[Exact("si"), Exact("pp"), Exact("feature")],
        path_tail: None,
    },
    Site { domains: &[Suffix("spotify.com")], params: &[Exact("si")], path_tail: None },
    Site {
        domains: &[Brand("amazon")],
        params: &[
            Exact("ref"),
            Exact("ref_"),
            Prefix("pf_rd_"),
            Prefix("pd_rd_"),
            Exact("content-id"),
            Exact("_encoding"),
            Exact("qid"),
            Exact("sr"),
            Exact("crid"),
            Exact("sprefix"),
        ],
        path_tail: Some("ref="),
    },
    Site {
        domains: &[Suffix("x.com"), Suffix("twitter.com")],
        params: &[Exact("s"), Exact("t"), Exact("ref_src"), Exact("ref_url")],
        path_tail: None,
    },
    Site { domains: &[Suffix("instagram.com")], params: &[Exact("igsh")], path_tail: None },
    Site {
        domains: &[Suffix("tiktok.com")],
        params: &[Exact("is_from_webapp"), Exact("sender_device"), Exact("_r"), Exact("_t")],
        path_tail: None,
    },
    Site {
        domains: &[Suffix("linkedin.com")],
        params: &[Exact("trk"), Exact("trackingId"), Exact("lipi")],
        path_tail: None,
    },
    Site { domains: &[Suffix("reddit.com")], params: &[Exact("share_id"), Exact("rdt")], path_tail: None },
    Site {
        domains: &[Suffix("facebook.com")],
        params: &[Exact("mibextid"), Exact("__tn__"), Prefix("__cft__")],
        path_tail: None,
    },
    Site {
        domains: &[Brand("google")],
        params: &[
            Exact("ved"),
            Exact("ei"),
            Exact("sca_esv"),
            Exact("sxsrf"),
            Exact("gs_lcrp"),
            Exact("oq"),
            Exact("aqs"),
            Exact("sourceid"),
        ],
        path_tail: None,
    },
];

const REDIRECTS: &[Redirect] = &[
    Redirect { domains: &[Brand("google")], path: "/url", targets: &["url", "q"] },
    Redirect {
        domains: &[Suffix("l.facebook.com"), Suffix("lm.facebook.com")],
        path: "/l.php",
        targets: &["u"],
    },
];

/// `url` without tracking parameters, and a redirect wrapper replaced by the cleaned link it
/// forwards to. Everything else keeps its form: the order, encoding and values of other
/// parameters and the fragment. A query left empty loses its `?`. Text that does not parse, a
/// scheme other than `http` or `https`, and a URL with nothing to remove come back unchanged.
pub fn clean(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return url.to_owned();
    }
    if let Some(target) = redirect_target(&parsed) {
        return clean(&target);
    }
    let host = parsed.host_str().unwrap_or_default();
    let sites: Vec<&Site> = SITES.iter().filter(|s| s.domains.iter().any(|d| d.matches(host))).collect();
    let tracking = |name: &str| GLOBAL.iter().chain(sites.iter().flat_map(|s| s.params)).any(|p| p.matches(name));

    let pairs: Vec<&str> = parsed.query().unwrap_or_default().split('&').filter(|p| !p.is_empty()).collect();
    let kept: Vec<&str> = pairs.iter().copied().filter(|pair| !tracking(&param_name(pair))).collect();
    let path = sites.iter().find_map(|s| s.path_tail.and_then(|tail| strip_path_tail(parsed.path(), tail)));
    if kept.len() == pairs.len() && path.is_none() {
        return url.to_owned();
    }

    let query = kept.join("&");
    parsed.set_query((!query.is_empty()).then_some(&query));
    if let Some(path) = path {
        parsed.set_path(&path);
    }
    parsed.into()
}

fn param_name(pair: &str) -> String {
    form_urlencoded::parse(pair.as_bytes()).next().map(|(name, _)| name.into_owned()).unwrap_or_default()
}

/// `path` without its last segment when that starts with `tail`.
fn strip_path_tail(path: &str, tail: &str) -> Option<String> {
    let (head, last) = path.trim_end_matches('/').rsplit_once('/')?;
    last.starts_with(tail).then(|| head.to_owned())
}

/// The http(s) link a redirect wrapper forwards to.
fn redirect_target(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let redirect = REDIRECTS.iter().find(|r| r.path == url.path() && r.domains.iter().any(|d| d.matches(host)))?;
    let target = redirect
        .targets
        .iter()
        .find_map(|t| url.query_pairs().find(|(name, _)| name == t).map(|(_, value)| value.into_owned()))?;
    Url::parse(&target).is_ok_and(|u| matches!(u.scheme(), "http" | "https")).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::clean;

    fn assert_cleans(cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(clean(input), *expected, "clean({input:?})");
        }
    }

    #[test]
    fn global_params() {
        assert_cleans(&[
            ("https://example.com/a?utm_source=x&utm_medium=y&utm_campaign=z&utm_content=c&utm_term=t", "https://example.com/a"),
            ("https://example.com/a?UTM_Source=x", "https://example.com/a"),
            ("https://example.com/?fbclid=1&gclid=2&dclid=3&gbraid=4&wbraid=5", "https://example.com/"),
            ("https://example.com/?msclkid=1&yclid=2&twclid=3&ttclid=4&li_fat_id=5&igshid=6", "https://example.com/"),
            ("https://example.com/?mc_cid=1&mc_eid=2&_ga=3&_gl=4&_hsenc=5&_hsmi=6&mkt_tok=7", "https://example.com/"),
            ("https://example.com/?oly_anon_id=1&oly_enc_id=2&vero_id=3&rb_clickid=4&wickedid=5", "https://example.com/"),
            ("https://example.com/?srsltid=1&sc_cid=2&spm=3", "https://example.com/"),
            ("https://example.com/p?id=7&utm_source=x", "https://example.com/p?id=7"),
        ]);
    }

    #[test]
    fn site_params_apply_only_on_their_site() {
        assert_cleans(&[
            ("https://example.com/?si=1&feature=2&ref=3&s=4&t=5&trk=6", "https://example.com/?si=1&feature=2&ref=3&s=4&t=5&trk=6"),
            ("https://notyoutube.com/?si=1", "https://notyoutube.com/?si=1"),
            ("https://google.example.com/?ved=1", "https://google.example.com/?ved=1"),
        ]);
    }

    #[test]
    fn youtube() {
        assert_cleans(&[
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=abc&pp=ygU&feature=shared", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            ("https://youtu.be/dQw4w9WgXcQ?si=abc&t=42", "https://youtu.be/dQw4w9WgXcQ?t=42"),
            ("https://m.youtube.com/watch?v=x&t=1m2s&list=PL1&index=3&si=abc", "https://m.youtube.com/watch?v=x&t=1m2s&list=PL1&index=3"),
        ]);
    }

    #[test]
    fn spotify() {
        assert_cleans(&[
            ("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc123", "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            (
                "https://open.spotify.com/track/4u?si=abc&context=spotify%3Aalbum%3A1",
                "https://open.spotify.com/track/4u?context=spotify%3Aalbum%3A1",
            ),
        ]);
    }

    #[test]
    fn amazon() {
        assert_cleans(&[
            (
                "https://www.amazon.com/dp/B08N5WRWNW/ref=sr_1_1?crid=2X&keywords=echo&qid=1700&sprefix=ech%2Caps&sr=8-1",
                "https://www.amazon.com/dp/B08N5WRWNW?keywords=echo",
            ),
            ("https://www.amazon.co.uk/gp/product/B0/ref=ppx_yo_dt?ie=UTF8&psc=1", "https://www.amazon.co.uk/gp/product/B0?ie=UTF8&psc=1"),
            (
                "https://www.amazon.de/dp/B0?pf_rd_r=1&pf_rd_p=2&pd_rd_w=3&pd_rd_r=4&content-id=5&_encoding=UTF8&ref_=6&th=1",
                "https://www.amazon.de/dp/B0?th=1",
            ),
            ("https://www.amazon.com.au/s?k=kettle&ref=nb_sb_noss", "https://www.amazon.com.au/s?k=kettle"),
            ("https://www.amazon.com/dp/B0/", "https://www.amazon.com/dp/B0/"),
            ("https://example.com/dp/B0/ref=sr_1_1", "https://example.com/dp/B0/ref=sr_1_1"),
        ]);
    }

    #[test]
    fn x_and_twitter() {
        assert_cleans(&[
            ("https://x.com/user/status/1?s=20&t=abc", "https://x.com/user/status/1"),
            ("https://twitter.com/user?ref_src=twsrc%5Etfw&ref_url=x", "https://twitter.com/user"),
        ]);
    }

    #[test]
    fn instagram() {
        assert_cleans(&[
            ("https://www.instagram.com/p/Cx/?igsh=MWQ", "https://www.instagram.com/p/Cx/"),
            ("https://www.instagram.com/reel/Cx/?igshid=NTc", "https://www.instagram.com/reel/Cx/"),
        ]);
    }

    #[test]
    fn tiktok() {
        assert_cleans(&[(
            "https://www.tiktok.com/@u/video/7?is_from_webapp=1&sender_device=pc&_r=1&_t=8k&lang=en",
            "https://www.tiktok.com/@u/video/7?lang=en",
        )]);
    }

    #[test]
    fn linkedin() {
        assert_cleans(&[(
            "https://www.linkedin.com/jobs/view/1/?trk=public&trackingId=abc&lipi=urn%3Ali",
            "https://www.linkedin.com/jobs/view/1/",
        )]);
    }

    #[test]
    fn reddit() {
        assert_cleans(&[(
            "https://www.reddit.com/r/rust/comments/abc/x/?share_id=1&rdt=2&utm_source=share&sort=top",
            "https://www.reddit.com/r/rust/comments/abc/x/?sort=top",
        )]);
    }

    #[test]
    fn facebook() {
        assert_cleans(&[
            ("https://www.facebook.com/share/p/1/?mibextid=abc", "https://www.facebook.com/share/p/1/"),
            (
                "https://www.facebook.com/page/posts/1?__cft__%5B0%5D=AZ&__cft__[1]=BZ&__tn__=%2CO%2CP-R&id=9",
                "https://www.facebook.com/page/posts/1?id=9",
            ),
        ]);
    }

    #[test]
    fn google_search() {
        assert_cleans(&[
            (
                "https://www.google.com/search?q=rust+url&oq=rust&gs_lcrp=Eg&sourceid=chrome&ie=UTF-8&sca_esv=1&sxsrf=2&ei=3&ved=4&aqs=5",
                "https://www.google.com/search?q=rust+url&ie=UTF-8",
            ),
            ("https://www.google.co.uk/search?q=tea&ved=1", "https://www.google.co.uk/search?q=tea"),
            ("https://www.google.de/search?q=bier&ei=1", "https://www.google.de/search?q=bier"),
        ]);
    }

    #[test]
    fn redirects_unwrap_to_the_cleaned_target() {
        assert_cleans(&[
            (
                "https://www.google.com/url?sa=t&rct=j&url=https%3A%2F%2Fexample.com%2Fa%3Fid%3D1%26utm_source%3Dx&ved=2&usg=3",
                "https://example.com/a?id=1",
            ),
            ("https://www.google.fr/url?q=https://example.com/b&sa=U&ved=1", "https://example.com/b"),
            (
                "https://l.facebook.com/l.php?u=https%3A%2F%2Fexample.com%2F%3Ffbclid%3Dabc&h=AT0",
                "https://example.com/",
            ),
            ("https://lm.facebook.com/l.php?u=http%3A%2F%2Fexample.org%2Fx", "http://example.org/x"),
        ]);
    }

    #[test]
    fn redirects_to_non_http_targets_stay() {
        assert_cleans(&[
            ("https://www.google.com/url?q=javascript:alert(1)&ved=1", "https://www.google.com/url?q=javascript:alert(1)"),
            ("https://l.facebook.com/l.php?u=not%20a%20url", "https://l.facebook.com/l.php?u=not%20a%20url"),
        ]);
    }

    #[test]
    fn fragment_encoding_and_order_are_kept() {
        assert_cleans(&[
            ("https://example.com/a?b=2&utm_source=x&a=%D0%B2+1#sec-2", "https://example.com/a?b=2&a=%D0%B2+1#sec-2"),
            ("https://example.com/a?utm_source=x#top", "https://example.com/a#top"),
            ("https://example.com/a?z=1&fbclid=x&y=%2F&x", "https://example.com/a?z=1&y=%2F&x"),
        ]);
    }

    #[test]
    fn empty_query_is_dropped() {
        assert_cleans(&[
            ("https://example.com/a?fbclid=x", "https://example.com/a"),
            ("https://example.com/a?&utm_source=x&", "https://example.com/a"),
        ]);
    }

    #[test]
    fn unchanged_inputs() {
        assert_cleans(&[
            ("not a url", "not a url"),
            ("", ""),
            ("example.com/?utm_source=x", "example.com/?utm_source=x"),
            ("ftp://example.com/?utm_source=x", "ftp://example.com/?utm_source=x"),
            ("file:///C:/a.html?utm_source=x", "file:///C:/a.html?utm_source=x"),
            ("about:blank", "about:blank"),
            ("https://EXAMPLE.com", "https://EXAMPLE.com"),
            ("https://example.com/?q=1&&r=2", "https://example.com/?q=1&&r=2"),
        ]);
    }
}
