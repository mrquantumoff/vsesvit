//! Owned by W1.5.
//! A launch handed to the running browser: the process's own command line, or a later one on the
//! same profile forwarded over the instance channel (W1.4). With no normal window open it opens the
//! startup windows; otherwise it raises the last normal window, or opens its targets in a new one.
//! With only private windows open, the browser starts afresh in normal windows, as Chrome does.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use gpui::Context;
use vsesvit_core::Url;
use vsesvit_core::private::Browsing;
use vsesvit_core::search::classify_url;

use super::{BrowserState, BrowserStateEvent, WindowId};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invocation {
    /// The command line's targets, as typed.
    pub targets: Vec<String>,
    /// The launching process's working directory, which relative files are read against.
    pub cwd: PathBuf,
    /// The launch's startup id (`DESKTOP_STARTUP_ID`, `XDG_ACTIVATION_TOKEN`), for raising.
    pub startup_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvocationOutcome {
    StartupWindows,
    Raised(WindowId),
    NewWindow(WindowId),
}

impl BrowserState {
    /// Runs a launch. Returns what it did and a line for each target it could not open, for the
    /// launching process's stderr.
    pub fn handle_invocation(&mut self, invocation: Invocation, cx: &mut Context<Self>) -> (InvocationOutcome, Vec<String>) {
        let mut errors = Vec::new();
        let mut urls = Vec::with_capacity(invocation.targets.len());
        for target in &invocation.targets {
            match resolve_cli_target(OsStr::new(target), &invocation.cwd, Path::is_file) {
                Some(url) => urls.push(url),
                None => errors.push(format!("vsesvit: cannot open {target}")),
            }
        }
        let outcome = match self.windows_of(Browsing::Normal).first() {
            None => {
                self.open_startup_windows(urls, cx);
                InvocationOutcome::StartupWindows
            }
            Some(&window) if urls.is_empty() => {
                cx.emit(BrowserStateEvent::RaiseWindow { window, startup_id: invocation.startup_id });
                InvocationOutcome::Raised(window)
            }
            Some(_) => InvocationOutcome::NewWindow(self.open_window(Browsing::Normal, urls, cx)),
        };
        (outcome, errors)
    }
}

/// A command-line target: an existing file relative to the launching process's working directory,
/// else a URL, a path or a host as the address bar reads it.
fn resolve_cli_target(arg: &OsStr, cwd: &Path, is_file: impl Fn(&Path) -> bool) -> Option<Url> {
    let path = cwd.join(arg);
    if path.is_absolute() && is_file(&path) {
        return Url::from_file_path(&path).ok();
    }
    arg.to_str().and_then(classify_url).map(|target| target.url().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_prefer_existing_files_then_urls_then_hosts() {
        let cwd = if cfg!(windows) { Path::new(r"C:\u") } else { Path::new("/home/u") };
        let page = cwd.join("page.html");
        let exists = |p: &Path| p == page;
        let resolve = |arg: &str| resolve_cli_target(OsStr::new(arg), cwd, exists).map(String::from);
        assert_eq!(resolve("https://example.com/a?b#c").as_deref(), Some("https://example.com/a?b#c"));
        assert_eq!(resolve("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(resolve("page.html"), Url::from_file_path(&page).ok().map(String::from));
        assert_eq!(resolve("example.com/path").as_deref(), Some("https://example.com/path"));
        assert_eq!(resolve("not a target"), None);
    }
}
