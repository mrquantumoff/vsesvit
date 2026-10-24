//! Owned by W0.5.
//! `OUT_DIR/testenv/env.json`: what the launcher made, for the guard to check (design section 7.3).
//! `xtask`'s launcher includes this file by path, so the writer and the reader share one schema.

use serde_json::{Value, json};

/// The variable that names `env.json`. Anyone can set it, so the guard trusts nothing it says
/// until the machine confirms it.
pub const VAR: &str = "VSESVIT_TESTENV";

/// The root-window property of the private X server that holds the token.
pub const ROOT_PROPERTY: &str = "_VSESVIT_TESTENV";

/// The launcher's fresh 128-bit token, as 32 lowercase hex digits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn from_bytes(bytes: [u8; 16]) -> Token {
        Token(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn parse(text: &str) -> Result<Token, String> {
        if text.len() == 32 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            Ok(Token(text.to_owned()))
        } else {
            Err(format!("token {text:?} is not 32 lowercase hex digits"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The private desktop the Windows launcher creates for this token.
    pub fn desktop_name(&self) -> String {
        format!("vsesvit-test-{}", self.0)
    }
}

/// Where the launcher put the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// Linux: a private Xvfb on `display` (`:N`) and a private D-Bus session.
    Xvfb { display: String, xvfb_pid: u32, dbus_address: String },
    /// Windows: a private desktop of the interactive window station, never switched to.
    Desktop { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvFile {
    pub token: Token,
    pub place: Place,
}

impl EnvFile {
    pub fn to_json(&self) -> String {
        let value = match &self.place {
            Place::Xvfb { display, xvfb_pid, dbus_address } => json!({
                "token": self.token.as_str(),
                "kind": "xvfb",
                "display": display,
                "xvfb_pid": xvfb_pid,
                "dbus_address": dbus_address,
            }),
            Place::Desktop { name } => json!({
                "token": self.token.as_str(),
                "kind": "desktop",
                "desktop": name,
            }),
        };
        serde_json::to_string_pretty(&value).expect("a JSON object serializes")
    }

    pub fn parse(text: &str) -> Result<EnvFile, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let field = |name: &str| value[name].as_str().ok_or_else(|| format!("no string {name:?}"));
        let token = Token::parse(field("token")?)?;
        let place = match field("kind")? {
            "xvfb" => Place::Xvfb {
                display: field("display")?.to_owned(),
                xvfb_pid: value["xvfb_pid"]
                    .as_u64()
                    .and_then(|p| u32::try_from(p).ok())
                    .ok_or("no pid \"xvfb_pid\"")?,
                dbus_address: field("dbus_address")?.to_owned(),
            },
            "desktop" => Place::Desktop { name: field("desktop")?.to_owned() },
            other => return Err(format!("unknown kind {other:?}")),
        };
        Ok(EnvFile { token, place })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> Token {
        Token::from_bytes([0xab; 16])
    }

    #[test]
    fn both_places_round_trip() {
        for place in [
            Place::Xvfb { display: ":101".into(), xvfb_pid: 4242, dbus_address: "unix:path=/o/bus,guid=1".into() },
            Place::Desktop { name: token().desktop_name() },
        ] {
            let file = EnvFile { token: token(), place };
            assert_eq!(EnvFile::parse(&file.to_json()), Ok(file));
        }
    }

    #[test]
    fn a_token_is_32_lowercase_hex_digits() {
        assert_eq!(token().as_str().len(), 32);
        assert!(Token::parse(&"A".repeat(32)).is_err());
        assert!(Token::parse("abc").is_err());
        assert_eq!(token().desktop_name(), format!("vsesvit-test-{}", "ab".repeat(16)));
    }

    #[test]
    fn a_file_missing_a_field_does_not_parse() {
        assert!(EnvFile::parse(r#"{"token":"abababababababababababababababab","kind":"xvfb","display":":1"}"#).is_err());
        assert!(EnvFile::parse(r#"{"token":"abababababababababababababababab","kind":"tty"}"#).is_err());
        assert!(EnvFile::parse("[]").is_err());
    }
}
