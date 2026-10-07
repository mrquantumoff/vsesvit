//! A profile's name and picture from the sync account signed in to it, as Chrome takes them from
//! the Google account: [`AccountDetails`] reads what the provider said about the person, and
//! [`AccountPicture`] fetches the picture and shrinks it, on a worker thread.
//! [`super::ProfilesDir::take_account_details`] stores them, unless the user named or coloured
//! the profile by hand.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use vsesvit_sync_proto::Claims;

use super::NAME_MAX;
use crate::Url;
use crate::favicons::{fetch_agent, local_hosts_allowed};

/// The edge of a stored picture: the picker's 72px avatar at 200% scale.
const SIZE: u32 = 144;
const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024;
const MAX_URL_BYTES: usize = 2048;

/// What a profile takes from its sync account. Either part may be missing, and a missing part
/// leaves the profile's own as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountDetails {
    pub name: Option<String>,
    /// HTTPS only.
    pub picture: Option<Url>,
}

impl AccountDetails {
    /// Chrome names a profile after the account's given name, else its full name. Providers
    /// without either still have a username or an email address, whose local part reads as one.
    pub fn from_claims(claims: &Claims) -> AccountDetails {
        let email_name = claims.email.as_deref().and_then(|email| email.split_once('@')).map(|(local, _)| local);
        let name = [claims.given_name.as_deref(), claims.name.as_deref(), claims.preferred_username.as_deref(), email_name]
            .into_iter()
            .flatten()
            .map(clean_name)
            .find(|name| !name.is_empty());
        AccountDetails { name, picture: claims.picture.as_deref().and_then(picture_url) }
    }
}

/// Whitespace runs as one space, control characters left out, at most [`NAME_MAX`] characters.
fn clean_name(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect();
    words.join(" ").chars().take(NAME_MAX).collect::<String>().trim_end().to_owned()
}

fn picture_url(url: &str) -> Option<Url> {
    if url.len() > MAX_URL_BYTES {
        return None;
    }
    let url = Url::parse(url).ok()?;
    let secure = url.scheme() == "https" || (url.scheme() == "http" && local_hosts_allowed());
    secure.then_some(url)
}

/// An account's picture as a profile keeps it: a square PNG of at most [`SIZE`] pixels a side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountPicture(Vec<u8>);

impl AccountPicture {
    /// Blocking: run it on a worker thread. Public addresses only, like favicons. `None` when the
    /// picture cannot be fetched or decoded.
    pub fn fetch(url: &Url) -> Option<AccountPicture> {
        let fetched = fetch_agent().get(url.as_str()).call().and_then(|mut response| {
            if !response.status().is_success() {
                return Err(ureq::Error::StatusCode(response.status().as_u16()));
            }
            response.body_mut().with_config().limit(MAX_DOWNLOAD_BYTES).read_to_vec()
        });
        match fetched {
            Ok(bytes) => AccountPicture::decode(&bytes),
            Err(e) => {
                log::info!("the sync account's picture: {e}");
                None
            }
        }
    }

    /// Decodes a PNG, JPEG, GIF, WebP or BMP, keeps the square in its middle, and scales that
    /// down to [`SIZE`].
    pub fn decode(bytes: &[u8]) -> Option<AccountPicture> {
        // Decoders of untrusted bytes can panic; that picture is just not taken.
        catch_unwind(AssertUnwindSafe(|| decode(bytes))).ok().flatten().map(AccountPicture)
    }

    pub fn png(&self) -> &[u8] {
        &self.0
    }

    /// Named after its content, so a new picture is a new file, which no image cache mistakes
    /// for the old one.
    pub(super) fn file_name(&self) -> String {
        let hash = Sha256::digest(&self.0);
        let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
        format!("Account Picture {hex}.png")
    }
}

fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok().filter(|f| *f != ImageFormat::Ico)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let edge = image.width().min(image.height());
    let mut image = image.crop_imm((image.width() - edge) / 2, (image.height() - edge) / 2, edge, edge);
    if edge > SIZE {
        image = image.resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    }
    let mut png = Vec::new();
    DynamicImage::from(image.into_rgba8()).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    fn claims(given: Option<&str>, name: Option<&str>, username: Option<&str>, email: Option<&str>) -> Claims {
        let own = |s: Option<&str>| s.map(str::to_owned);
        Claims { given_name: own(given), name: own(name), preferred_username: own(username), email: own(email), picture: None }
    }

    fn name(claims: Claims) -> Option<String> {
        AccountDetails::from_claims(&claims).name
    }

    #[test]
    fn the_name_is_the_given_name_then_the_full_name_the_username_and_the_email() {
        assert_eq!(name(claims(Some("Demir"), Some("Demir Yerli"), Some("demir"), Some("d@example.com"))).as_deref(), Some("Demir"));
        assert_eq!(name(claims(Some("  "), Some("Demir Yerli"), Some("demir"), None)).as_deref(), Some("Demir Yerli"));
        assert_eq!(name(claims(None, None, Some("demir"), Some("d@example.com"))).as_deref(), Some("demir"));
        assert_eq!(name(claims(None, None, None, Some("demir.yerli@example.com"))).as_deref(), Some("demir.yerli"));
        assert_eq!(name(claims(None, None, None, Some("not an address"))), None);
        assert_eq!(name(claims(None, None, None, Some("@example.com"))), None);
    }

    #[test]
    fn a_provider_that_says_nothing_changes_nothing() {
        assert_eq!(AccountDetails::from_claims(&Claims::default()), AccountDetails::default());
    }

    #[test]
    fn names_are_one_line_and_fit_a_profile_name() {
        assert_eq!(name(claims(Some(" Ann\n\tMarie\u{7}  Smith "), None, None, None)).as_deref(), Some("Ann Marie Smith"));
        let long = name(claims(Some(&"x".repeat(200)), None, None, None)).unwrap();
        assert_eq!(long.chars().count(), NAME_MAX);
        assert_eq!(name(claims(Some("\u{0}\u{1b}"), Some("Bob"), None, None)).as_deref(), Some("Bob"));
    }

    #[test]
    fn pictures_come_over_https_only() {
        let picture = |url: &str| AccountDetails::from_claims(&Claims { picture: Some(url.to_owned()), ..Claims::default() }).picture;
        assert_eq!(picture("https://lh3.example.com/a/p=s96-c").map(String::from).as_deref(), Some("https://lh3.example.com/a/p=s96-c"));
        for refused in ["http://example.com/p.png", "file:///etc/passwd", "data:image/png;base64,AAAA", "javascript:alert(1)", "p.png"] {
            assert_eq!(picture(refused), None, "{refused}");
        }
        assert_eq!(picture(&format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES))), None);
    }

    fn encoded(image: RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::from(image).write_to(&mut Cursor::new(&mut out), format).unwrap();
        out
    }

    #[test]
    fn a_picture_is_the_middle_square_scaled_down() {
        let mut wide = RgbaImage::from_pixel(600, 300, Rgba([255, 0, 0, 255]));
        for y in 0..300 {
            for x in 150..450 {
                wide.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
        let picture = AccountPicture::decode(&encoded(wide, ImageFormat::Png)).unwrap();
        let decoded = image::load_from_memory_with_format(picture.png(), ImageFormat::Png).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (SIZE, SIZE));
        assert_eq!(decoded.get_pixel(0, 0), &Rgba([0, 0, 255, 255]), "the red sides are cut off");
        assert_eq!(decoded.get_pixel(SIZE - 1, SIZE - 1), &Rgba([0, 0, 255, 255]));

        let small = AccountPicture::decode(&encoded(RgbaImage::from_pixel(40, 50, Rgba([0; 4])), ImageFormat::Png)).unwrap();
        let small = image::load_from_memory(small.png()).unwrap();
        assert_eq!((small.width(), small.height()), (40, 40), "never scaled up");
    }

    #[test]
    fn what_is_not_a_picture_is_refused() {
        assert_eq!(AccountPicture::decode(b"<!doctype html><title>Not found</title>"), None);
        assert_eq!(AccountPicture::decode(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        let png = encoded(RgbaImage::from_pixel(64, 64, Rgba([0; 4])), ImageFormat::Png);
        assert_eq!(AccountPicture::decode(&png[..40]), None, "truncated");
    }

    #[test]
    fn a_new_picture_has_a_new_file_name() {
        let a = AccountPicture::decode(&encoded(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])), ImageFormat::Png)).unwrap();
        let b = AccountPicture::decode(&encoded(RgbaImage::from_pixel(8, 8, Rgba([3, 2, 1, 255])), ImageFormat::Png)).unwrap();
        assert_ne!(a.file_name(), b.file_name());
        assert_eq!(a.file_name(), a.clone().file_name());
        assert!(a.file_name().ends_with(".png"));
    }
}
