//! Content types for files served from `chrome-extension://`. WebKit refuses to run a
//! script or apply a stylesheet served with the wrong type, so this is by extension
//! (extension packages carry no metadata) with a conservative default.

pub fn for_path(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "txt" | "md" => "text/plain",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "map" => "application/json",
        "webm" => "video/webm",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_and_unknown() {
        assert_eq!(super::for_path("popup.html"), "text/html");
        assert_eq!(super::for_path("js/Background.JS"), "text/javascript");
        assert_eq!(super::for_path("a.b.css"), "text/css");
        assert_eq!(super::for_path("_locales/en/messages.json"), "application/json");
        assert_eq!(super::for_path("icon.svg"), "image/svg+xml");
        assert_eq!(super::for_path("noext"), "application/octet-stream");
        assert_eq!(super::for_path("x.unknownext"), "application/octet-stream");
    }
}
