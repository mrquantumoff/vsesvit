//! Applying a verified artifact, per format (docs/design/packaging.md, "Installing, per format").

use std::ffi::OsString;

use crate::{DisabledReason, Downloaded, Error, Format, Installation};

/// What the shell does once [`Downloaded::install`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// The Windows installer is running and waits for this process. Save the session and exit.
    /// The library never exits the process itself.
    ExitNow,
    /// The package manager installed the new version. It runs after a restart.
    Relaunch,
    /// The new AppImage is in place. It runs on the next launch.
    NextLaunch,
}

/// An install that did not happen. The verified download comes back, so trying again does not
/// download it again.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct InstallFailed {
    pub error: Error,
    pub downloaded: Downloaded,
}

impl From<InstallFailed> for Error {
    fn from(failed: InstallFailed) -> Error {
        failed.error
    }
}

impl Downloaded {
    /// `relaunch_args` are the arguments the relaunched browser gets, without the program name.
    /// Only the Windows installer uses them. A successful install consumes the download: the
    /// package manager is done with it, or the Windows installer is running from it.
    #[expect(clippy::result_large_err, reason = "once per update, and the error carries the download back")]
    pub fn install(self, installation: &Installation, relaunch_args: &[OsString]) -> Result<Installed, InstallFailed> {
        let result = self.apply(installation, relaunch_args);
        result.map_err(|error| InstallFailed { error, downloaded: self })
    }

    /// For a browser quitting with an update ready: runs the Windows installer silently
    /// (`/S /UPDATE`) and does not relaunch.
    #[cfg(windows)]
    #[expect(clippy::result_large_err, reason = "once per update, and the error carries the download back")]
    pub fn install_on_exit(self, installation: &Installation) -> Result<Installed, InstallFailed> {
        let result = match (installation, self.format) {
            (Installation::Nsis { .. }, Format::Nsis) => nsis::launch(&self.path, std::ffi::OsStr::new("/S /UPDATE")),
            _ => Err(mismatch(installation)),
        };
        result.map_err(|error| InstallFailed { error, downloaded: self })
    }

    fn apply(&self, installation: &Installation, relaunch_args: &[OsString]) -> Result<Installed, Error> {
        match (installation, self.format) {
            (Installation::Nsis { .. }, Format::Nsis) => {
                #[cfg(windows)]
                return nsis::launch(&self.path, &nsis::parameters(self.install_mode, relaunch_args));
                #[cfg(not(windows))]
                return {
                    let _ = relaunch_args;
                    Err(Error::Install("an NSIS update can only be installed on Windows".into()))
                };
            }
            (Installation::Deb, Format::Deb) => self.pkexec("dpkg", &["-i"]),
            (Installation::Rpm, Format::Rpm) => self.pkexec("rpm", &["-U"]),
            (Installation::Pacman, Format::Pacman) => self.pkexec("pacman", &["-U", "--noconfirm"]),
            (Installation::AppImage { image }, Format::AppImage) => {
                #[cfg(unix)]
                return appimage::replace(&self.path, image);
                #[cfg(not(unix))]
                return {
                    let _ = image;
                    Err(Error::Install("an AppImage update can only be installed on Linux".into()))
                };
            }
            _ => Err(mismatch(installation)),
        }
    }

    #[cfg(unix)]
    fn pkexec(&self, program: &str, args: &[&str]) -> Result<Installed, Error> {
        let status = std::process::Command::new("pkexec")
            .arg(program)
            .args(args)
            .arg(&self.path)
            .status()
            .map_err(|e| Error::Install(format!("could not run pkexec: {e}")))?;
        match status.code() {
            Some(0) => {
                let _ = std::fs::remove_file(&self.path);
                Ok(Installed::Relaunch)
            }
            // pkexec's own codes: the password prompt was dismissed, or authorization failed.
            Some(126 | 127) => Err(Error::Install("administrator authorization was not given".into())),
            _ => Err(Error::Install(format!("{program} failed ({status})"))),
        }
    }

    #[cfg(not(unix))]
    fn pkexec(&self, program: &str, _args: &[&str]) -> Result<Installed, Error> {
        Err(Error::Install(format!("a {program} package can only be installed on Linux")))
    }
}

fn mismatch(installation: &Installation) -> Error {
    match installation.format() {
        Some(expected) => Error::WrongArtifactType(expected),
        None => Error::Disabled(DisabledReason::NotSelfUpdating),
    }
}

#[cfg(unix)]
mod appimage {
    use std::fs::{self, File};
    use std::io;
    use std::path::Path;

    use super::Installed;
    use crate::Error;

    /// Copies the new image next to the running one and renames it over, so a crash leaves
    /// either the old image or the new one, never half of one.
    pub(super) fn replace(new: &Path, image: &Path) -> Result<Installed, Error> {
        let fail = |e: io::Error| Error::Install(format!("replacing {}: {e}", image.display()));
        let (Some(dir), Some(name)) = (image.parent(), image.file_name()) else {
            return Err(Error::Install(format!("{} is not a file path", image.display())));
        };
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(".vsesvit-update");
        let temp = dir.join(temp_name);

        let result = (|| {
            fs::copy(new, &temp)?;
            fs::set_permissions(&temp, fs::metadata(image)?.permissions())?;
            File::open(&temp)?.sync_all()?;
            fs::rename(&temp, image)?;
            File::open(dir)?.sync_all()
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&temp);
            return Err(fail(e));
        }
        let _ = fs::remove_file(new);
        Ok(Installed::NextLaunch)
    }
}

#[cfg(windows)]
mod nsis {
    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;
    use windows_sys::w;

    use super::Installed;
    use crate::{Error, WindowsInstallMode};

    /// Tauri's order: mode, `/UPDATE`, restart flag, `/ARGS` and the escaped arguments.
    pub(super) fn parameters(mode: WindowsInstallMode, relaunch_args: &[OsString]) -> OsString {
        let (mode_args, restart_args): (&[&str], &[&str]) = match mode {
            WindowsInstallMode::Passive => (&["/P"], &["/R"]),
            WindowsInstallMode::Quiet => (&["/S"], &["/R"]),
            WindowsInstallMode::BasicUi => (&[], &[]),
        };
        let mut parts: Vec<OsString> = mode_args.iter().map(OsString::from).collect();
        parts.push("/UPDATE".into());
        parts.extend(restart_args.iter().map(OsString::from));
        parts.push("/ARGS".into());
        parts.extend(relaunch_args.iter().map(|arg| escape(arg)));
        parts.join(OsStr::new(" "))
    }

    /// `ShellExecuteW` rather than `Command`: `CreateProcess` refuses an installer whose
    /// manifest asks for elevation (`ERROR_ELEVATION_REQUIRED`), where the shell shows the UAC
    /// prompt. That is what a per-machine install needs, and it is what Tauri does.
    pub(super) fn launch(installer: &Path, parameters: &OsStr) -> Result<Installed, Error> {
        let file = wide(installer.as_os_str());
        let parameters = wide(parameters);
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call; null window and
        // directory are allowed.
        let instance = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                w!("open"),
                file.as_ptr(),
                parameters.as_ptr(),
                std::ptr::null(),
                SW_SHOW,
            )
        };
        // ShellExecuteW's documented failure signal is a value of 32 or less.
        if instance as isize <= 32 {
            return Err(Error::Install(format!(
                "could not start the installer: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Installed::ExitNow)
    }

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain([0]).collect()
    }

    /// Windows command-line quoting, plus quoting any argument with a `/` so the installer does
    /// not read it as one of its own flags. Ported from Tauri's `escape_nsis_current_exe_arg`.
    pub(super) fn escape(arg: &OsStr) -> OsString {
        let quote = arg.is_empty() || arg.as_encoded_bytes().iter().any(|c| matches!(c, b' ' | b'\t' | b'/'));
        let mut out: Vec<u16> = Vec::new();
        if quote {
            out.push(u16::from(b'"'));
        }
        let mut backslashes = 0usize;
        for unit in arg.encode_wide() {
            if unit == u16::from(b'\\') {
                backslashes += 1;
            } else {
                if unit == u16::from(b'"') {
                    out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
                }
                backslashes = 0;
            }
            out.push(unit);
        }
        if quote {
            out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
            out.push(u16::from(b'"'));
        }
        OsString::from_wide(&out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn passive_mode_relaunches_with_escaped_args() {
            let args = [OsString::from("--profile"), OsString::from(r"C:\Users\A B\p"), OsString::from("https://x.test/a")];
            assert_eq!(
                parameters(WindowsInstallMode::Passive, &args),
                OsString::from(r#"/P /UPDATE /R /ARGS --profile "C:\Users\A B\p" "https://x.test/a""#)
            );
        }

        #[test]
        fn modes_pick_their_flags() {
            assert_eq!(parameters(WindowsInstallMode::Quiet, &[]), OsString::from("/S /UPDATE /R /ARGS"));
            assert_eq!(parameters(WindowsInstallMode::BasicUi, &[]), OsString::from("/UPDATE /ARGS"));
        }

        #[test]
        fn escaping_matches_windows_rules() {
            let cases = [
                ("plain", "plain"),
                ("", r#""""#),
                ("a b", r#""a b""#),
                (r#"say "hi""#, r#""say \"hi\"""#),
                (r"trailing\", r"trailing\"),
                (r"dir with space\", r#""dir with space\\""#),
                (r#"back\"quote"#, r#"back\\\"quote"#),
            ];
            for (input, expected) in cases {
                assert_eq!(escape(OsStr::new(input)), OsString::from(expected), "escaping {input:?}");
            }
        }
    }
}
