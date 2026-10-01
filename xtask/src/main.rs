//! `cargo xtask`: builds Vsesvit's packages, signs them and writes the update manifest.
//! The formats, file names and signing contract are in docs/design/packaging.md.

mod ctx;
mod icons;
mod linux;
mod manifest;
mod sign;
mod windows;

use std::process::ExitCode;

use ctx::{Ctx, Format};

const USAGE: &str = "\
Usage: cargo xtask package <nsis|deb|rpm|pacman|appimage|flatpak>... [--sign]
       cargo xtask manifest --base-url URL [--notes FILE] [--pub-date RFC3339] [--allow-unverified]
       cargo xtask sign FILE...
       cargo xtask signer generate -w PRIVATE_KEY_FILE [-p PASSWORD] [--force]
       cargo xtask icons OUT_DIR

Artifacts go to target/dist/. Signing reads TAURI_SIGNING_PRIVATE_KEY (the key or a
path to it) and TAURI_SIGNING_PRIVATE_KEY_PASSWORD, like the Tauri CLI.";

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result {
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    match command.as_str() {
        "package" => {
            let ctx = Ctx::new()?;
            let sign = rest.iter().any(|a| a == "--sign");
            let formats = rest
                .iter()
                .filter(|a| *a != "--sign")
                .map(|a| a.parse::<Format>())
                .collect::<Result<Vec<_>>>()?;
            if formats.is_empty() {
                return Err(USAGE.to_owned());
            }
            for format in formats {
                let artifact = match format {
                    Format::Nsis => windows::package(&ctx)?,
                    linux => linux::package(linux, &ctx)?,
                };
                println!("{}", artifact.display());
                if sign {
                    sign::sign_file(&artifact, &ctx.version)?;
                }
            }
            Ok(())
        }
        "manifest" => manifest::run(&Ctx::new()?, rest),
        "sign" if !rest.is_empty() => {
            let ctx = Ctx::new()?;
            rest.iter().try_for_each(|f| sign::sign_file(f.as_ref(), &ctx.version))
        }
        "signer" => match rest.split_first() {
            Some((sub, args)) if sub == "generate" => sign::generate_command(args),
            _ => Err(sign::GENERATE_USAGE.to_owned()),
        },
        "icons" => {
            let [out] = rest else { return Err(USAGE.to_owned()) };
            icons::write_all(out.as_ref())
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn usage_lists_every_subcommand_usage() {
        assert!(super::USAGE.contains(super::manifest::USAGE));
        assert!(super::USAGE.contains(super::sign::GENERATE_USAGE));
    }
}
