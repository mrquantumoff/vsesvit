use std::path::{Path, PathBuf};

use url::Url;
use vsesvit_update::{Config, DisabledReason, Error, Format, Installation, WindowsInstallMode};

#[test]
fn the_marker_decides_the_installation() {
    let dir = Path::new("/opt/vsesvit/lib/vsesvit");
    let image = || Some(PathBuf::from("/home/u/Apps/Vsesvit.AppImage"));
    let cases: [(Option<&str>, Option<PathBuf>, Installation); 12] = [
        (None, None, Installation::Unpackaged),
        (None, image(), Installation::Unpackaged),
        (Some("nsis"), None, Installation::Nsis { install_dir: dir.to_path_buf() }),
        (Some("deb\n"), None, Installation::Deb),
        (Some(" rpm "), None, Installation::Rpm),
        (Some("pacman"), None, Installation::Pacman),
        (Some("appimage"), image(), Installation::AppImage { image: image().unwrap() }),
        (Some("appimage"), None, Installation::Unpackaged),
        (Some("appimage"), Some(PathBuf::new()), Installation::Unpackaged),
        (Some("flatpak"), None, Installation::Flatpak),
        (Some("snap"), None, Installation::Unpackaged),
        (Some(""), None, Installation::Unpackaged),
    ];
    for (marker, appimage, expected) in cases {
        assert_eq!(Installation::from_parts(marker, dir, appimage.clone()), expected, "marker {marker:?}, $APPIMAGE {appimage:?}");
    }
}

#[test]
fn variant_and_self_updating_follow_the_installation() {
    let cases = [
        (Installation::Unpackaged, None, false, None),
        (Installation::Nsis { install_dir: "C:/V".into() }, Some("nsis"), true, Some(Format::Nsis)),
        (Installation::Deb, Some("deb"), true, Some(Format::Deb)),
        (Installation::Rpm, Some("rpm"), true, Some(Format::Rpm)),
        (Installation::Pacman, Some("pacman"), true, Some(Format::Pacman)),
        (Installation::AppImage { image: "/a".into() }, Some("appimage"), true, Some(Format::AppImage)),
        (Installation::Flatpak, Some("flatpak"), false, None),
    ];
    for (installation, variant, self_updates, format) in cases {
        assert_eq!(installation.variant(), variant, "{installation:?}");
        assert_eq!(installation.self_updates(), self_updates, "{installation:?}");
        assert_eq!(installation.format(), format, "{installation:?}");
    }
}

#[test]
fn a_running_test_binary_is_unpackaged() {
    assert_eq!(Installation::detect(), Installation::Unpackaged, "no package-format marker next to the test binary");
}

#[test]
fn a_tauri_updater_block_pastes_in_unchanged() {
    let config = Config::from_json(
        r#"{
            "active": true,
            "pubkey": "cHVia2V5",
            "endpoints": ["https://u.test/{{target}}/{{arch}}/{{current_version}}", "https://mirror.test/latest.json"],
            "windows": { "installMode": "quiet", "installerArgs": ["/NS"] }
        }"#,
    )
    .unwrap();
    assert_eq!(config.pubkey, "cHVia2V5");
    assert_eq!(config.endpoints.len(), 2);
    assert_eq!(config.endpoints[1], Url::parse("https://mirror.test/latest.json").unwrap());
    assert_eq!(config.windows_install_mode, WindowsInstallMode::Quiet);
    assert!(config.https_only);
}

#[test]
fn missing_fields_take_tauris_defaults() {
    let config = Config::from_json("{}").unwrap();
    assert_eq!((config.pubkey.as_str(), config.endpoints.len()), ("", 0));
    assert_eq!(config.windows_install_mode, WindowsInstallMode::Passive);
    assert!(config.https_only);
}

#[test]
fn plain_http_endpoints_need_the_dangerous_flag() {
    let err = Config::from_json(r#"{"endpoints": ["http://u.test/x"]}"#).unwrap_err();
    assert!(matches!(err, Error::Disabled(DisabledReason::InvalidConfig(_))), "{err:?}");
    let config =
        Config::from_json(r#"{"endpoints": ["http://u.test/x"], "dangerousInsecureTransportProtocol": true}"#).unwrap();
    assert!(!config.https_only);
}

#[test]
fn malformed_json_is_an_invalid_config() {
    for json in ["", "42", r#"{"endpoints": ["not a url"]}"#, r#"{"windows": {"installMode": "loud"}}"#] {
        let err = Config::from_json(json).unwrap_err();
        assert!(matches!(err, Error::Disabled(DisabledReason::InvalidConfig(_))), "{json:?}: {err:?}");
    }
}

#[test]
fn the_builtin_config_parses() {
    let config = Config::builtin().expect("packaging/updater.json and the build overrides are valid");
    assert!(!config.endpoints.is_empty(), "at least one endpoint is compiled in");
}
