//! Updater configuration, in the shape of `plugins.updater` in `tauri.conf.json` so a Tauri
//! app's block pastes into `packaging/updater.json` unchanged.

use serde::Deserialize;
use url::{Host, Url};

use crate::{DisabledReason, Error};

const BUILTIN: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/updater.json"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Base64 of a minisign public key file, as `tauri signer generate` prints it. Empty turns
    /// the updater off.
    pub pubkey: String,
    /// Tried in order. May hold `{{current_version}}`, `{{target}}`, `{{arch}}` and
    /// `{{bundle_type}}`.
    pub endpoints: Vec<Url>,
    pub windows_install_mode: WindowsInstallMode,
    /// Refuse plain-http endpoints and redirects. Tauri's `dangerousInsecureTransportProtocol`,
    /// inverted.
    pub https_only: bool,
}

/// How the NSIS installer shows itself during an update (`windows.installMode`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowsInstallMode {
    /// The installer's own UI, and the user finishes it.
    BasicUi,
    /// No UI at all.
    Quiet,
    /// A progress bar only.
    #[default]
    Passive,
}

impl Config {
    /// `packaging/updater.json` as compiled in, with `VSESVIT_UPDATER_PUBKEY` and
    /// `VSESVIT_UPDATER_ENDPOINTS` (comma separated) from the build environment replacing its
    /// values when set.
    pub fn builtin() -> Result<Config, Error> {
        Config::from_json(BUILTIN)?
            .with_overrides(option_env!("VSESVIT_UPDATER_PUBKEY"), option_env!("VSESVIT_UPDATER_ENDPOINTS"))
    }

    /// Unknown fields are ignored. Missing fields take Tauri's defaults.
    pub fn from_json(json: &str) -> Result<Config, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            pubkey: String,
            #[serde(default)]
            endpoints: Vec<Url>,
            #[serde(default)]
            windows: RawWindows,
            #[serde(default)]
            dangerous_insecure_transport_protocol: bool,
        }
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase")]
        struct RawWindows {
            #[serde(default, alias = "install-mode")]
            install_mode: WindowsInstallMode,
        }

        let raw: Raw = serde_json::from_str(json).map_err(|e| invalid(e.to_string()))?;
        Config {
            pubkey: raw.pubkey.trim().to_owned(),
            endpoints: raw.endpoints,
            windows_install_mode: raw.windows.install_mode,
            https_only: !raw.dangerous_insecure_transport_protocol,
        }
        .validated()
    }

    fn with_overrides(mut self, pubkey: Option<&str>, endpoints: Option<&str>) -> Result<Config, Error> {
        if let Some(pubkey) = pubkey {
            self.pubkey = pubkey.trim().to_owned();
        }
        if let Some(endpoints) = endpoints {
            self.endpoints = endpoints
                .split(',')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(|e| Url::parse(e).map_err(|err| invalid(format!("endpoint {e:?}: {err}"))))
                .collect::<Result<_, _>>()?;
            // The end-to-end test builds a copy that talks to a plain-http server on this machine.
            self.https_only = !self.endpoints.iter().all(is_loopback);
        }
        self.validated()
    }

    fn validated(self) -> Result<Config, Error> {
        if self.https_only
            && let Some(endpoint) = self.endpoints.iter().find(|e| e.scheme() != "https")
        {
            return Err(invalid(format!("endpoint {endpoint} is not https")));
        }
        Ok(self)
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(domain)) => domain == "localhost",
        None => false,
    }
}

fn invalid(reason: String) -> Error {
    Error::Disabled(DisabledReason::InvalidConfig(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Config {
        Config::from_json(r#"{"pubkey": "a2V5", "endpoints": ["https://example.com/{{target}}"]}"#).unwrap()
    }

    #[test]
    fn overrides_replace_pubkey_and_endpoints() {
        let config = base().with_overrides(Some(" b3RoZXI= "), Some("https://a.test/x, https://b.test/y,")).unwrap();
        assert_eq!(config.pubkey, "b3RoZXI=");
        assert_eq!(config.endpoints, [Url::parse("https://a.test/x").unwrap(), Url::parse("https://b.test/y").unwrap()]);
        assert!(config.https_only, "remote endpoints stay https-only");
    }

    #[test]
    fn absent_overrides_keep_the_file() {
        assert_eq!(base().with_overrides(None, None).unwrap(), base());
    }

    #[test]
    fn loopback_override_allows_plain_http() {
        for endpoint in ["http://127.0.0.1:8080/u", "http://[::1]:8080/u", "http://localhost:8080/u"] {
            let config = base().with_overrides(None, Some(endpoint)).unwrap();
            assert!(!config.https_only, "{endpoint} is loopback");
        }
    }

    #[test]
    fn remote_plain_http_override_is_rejected() {
        let err = base().with_overrides(None, Some("http://127.0.0.1/u,http://example.com/u")).unwrap_err();
        assert!(matches!(err, Error::Disabled(DisabledReason::InvalidConfig(_))), "{err:?}");
    }

    #[test]
    fn malformed_override_endpoint_is_rejected() {
        let err = base().with_overrides(None, Some("not a url")).unwrap_err();
        assert!(matches!(err, Error::Disabled(DisabledReason::InvalidConfig(_))), "{err:?}");
    }
}
