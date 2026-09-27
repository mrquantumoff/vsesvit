//! The runtime dependency table from docs/design/packaging.md, "Runtime dependencies".

pub struct Dependency {
    pub deb: &'static str,
    pub rpm: &'static str,
    pub pacman: &'static str,
    /// Recommends / optdepends instead of a hard dependency.
    pub optional: bool,
}

pub const RUNTIME: [Dependency; 7] = [
    Dependency { deb: "libgtk-4-1", rpm: "gtk4", pacman: "gtk4", optional: false },
    Dependency { deb: "libadwaita-1-0", rpm: "libadwaita", pacman: "libadwaita", optional: false },
    Dependency { deb: "libwebkitgtk-6.0-4", rpm: "webkitgtk6.0", pacman: "webkitgtk-6.0", optional: false },
    Dependency { deb: "gstreamer1.0-plugins-good", rpm: "gstreamer1-plugins-good", pacman: "gst-plugins-good", optional: false },
    Dependency { deb: "gstreamer1.0-plugins-bad", rpm: "gstreamer1-plugins-bad-free", pacman: "gst-plugins-bad", optional: false },
    Dependency { deb: "gstreamer1.0-libav", rpm: "gstreamer1-plugin-libav", pacman: "gst-libav", optional: false },
    Dependency { deb: "pkexec", rpm: "polkit", pacman: "polkit", optional: true },
];

/// Why the optional dependency is worth installing, for optdepends and similar fields.
pub const OPTIONAL_REASON: &str = "install updates from inside the browser";

pub fn required(name: fn(&Dependency) -> &'static str) -> Vec<&'static str> {
    RUNTIME.iter().filter(|d| !d.optional).map(name).collect()
}

pub fn optional(name: fn(&Dependency) -> &'static str) -> Vec<&'static str> {
    RUNTIME.iter().filter(|d| d.optional).map(name).collect()
}
