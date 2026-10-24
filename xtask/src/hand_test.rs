//! Owned by W0.5.
//! `cargo xtask hand-test [--flavour gpui|winui|gtk]`: builds and starts the real browser with a
//! scratch profile on the user's own desktop, for the scripts in hand-tests.md (design section 7.4).
//! No scripted input and no self-test code: the user drives it. Only the user runs this.

use std::process::Command;

use crate::{Result, Status, workspace_root};

const USAGE: &str = "usage: cargo xtask hand-test [--flavour gpui|winui|gtk]";

#[derive(Debug, PartialEq, Eq)]
enum Flavour {
    Gpui,
    Winui,
    Gtk,
}

impl Flavour {
    fn name(&self) -> &'static str {
        match self {
            Flavour::Gpui => "gpui",
            Flavour::Winui => "winui",
            Flavour::Gtk => "gtk",
        }
    }

    /// The features after `-p vsesvit`: each default shell is its OS's default build.
    fn build_args(&self) -> &'static [&'static str] {
        match self {
            Flavour::Gpui => &["--no-default-features", "--features", "gpui"],
            Flavour::Winui | Flavour::Gtk => &[],
        }
    }
}

fn parse(args: &[String]) -> Result<Flavour> {
    let default = if cfg!(windows) { Flavour::Winui } else { Flavour::Gtk };
    let flavour = match args {
        [] => default,
        [flag, name] if flag == "--flavour" => match name.as_str() {
            "gpui" => Flavour::Gpui,
            "winui" => Flavour::Winui,
            "gtk" => Flavour::Gtk,
            _ => return Err(USAGE.to_owned()),
        },
        _ => return Err(USAGE.to_owned()),
    };
    match flavour {
        Flavour::Winui if !cfg!(windows) => Err("the WinUI shell runs on Windows only".into()),
        Flavour::Gtk if cfg!(windows) => Err("the GTK shell runs on Linux only".into()),
        flavour => Ok(flavour),
    }
}

pub fn run(args: &[String]) -> Result<Status> {
    let flavour = parse(args)?;
    let mut build = vec!["-p", "vsesvit"];
    build.extend(flavour.build_args());
    let program = crate::testenv::cargo_build(&build, "vsesvit")?;
    let profile = crate::probe::fresh(workspace_root().join("target").join("hand-test").join(flavour.name()))?.join("profile");
    eprintln!(
        "hand-test: {} --profile-dir {}; the scripts are in docs/design/linux-cef/hand-tests.md",
        program.display(),
        profile.display()
    );
    let status = Command::new(&program).arg("--profile-dir").arg(&profile).status().map_err(|e| format!("{}: {e}", program.display()))?;
    match status.code() {
        Some(0) => Ok(Status::Done),
        code => Err(format!("the browser exited with {code:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn flavours() {
        let default = if cfg!(windows) { Flavour::Winui } else { Flavour::Gtk };
        assert_eq!(parse(&[]), Ok(default));
        assert_eq!(parse(&args(&["--flavour", "gpui"])), Ok(Flavour::Gpui));
        assert!(parse(&args(&["--flavour", if cfg!(windows) { "gtk" } else { "winui" }])).is_err());
        assert!(parse(&args(&["--flavour"])).is_err());
        assert!(parse(&args(&["--self-test", "out"])).is_err());
        assert_eq!(Flavour::Gpui.build_args(), ["--no-default-features", "--features", "gpui"]);
    }
}
