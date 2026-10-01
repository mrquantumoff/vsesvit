//! Linux shell of the Vsesvit browser: GTK4 + libadwaita + WebKitGTK 6.0, on top of
//! `vsesvit-core` (profile store) and `vsesvit-webext` (extension runtime).
//! Compiles to nothing on other targets.
#![cfg(target_os = "linux")]

mod address_bar;
mod app;
mod bookmark_drag;
mod bookmark_editor;
mod bookmark_menu;
mod bookmarks_bar;
mod browser;
mod cli;
mod closed_tabs;
mod dialogs;
mod downloads;
mod engine;
mod error_page;
mod extensions;
mod favicons;
mod find_bar;
mod keymap;
mod location;
mod motion;
mod omnibox;
mod page_menu;
mod permissions;
mod profile;
mod save_page;
pub mod screenshot;
#[cfg(feature = "self-test")]
mod self_test;
mod session;
mod site_info;
mod sync;
mod tab;
#[cfg(test)]
mod test_support;
mod updates;
mod window;
mod zoom;

use std::cell::Cell;
use std::ffi::OsString;
use std::process::ExitCode;

use cli::Command;
use gtk::prelude::*;

thread_local! {
    /// Set by the self-test; see [`popup`].
    static SCRIPTED: Cell<bool> = const { Cell::new(false) };
}

/// Opens a menu or bubble from `anchor`, which holds it until it closes or the anchor
/// leaves the window (a rebuilt bar, a closed window), whichever comes first.
///
/// An autohide popover takes a Wayland popup grab, which the compositor refuses without a
/// real input event, so under the self-test's scripted clicks popovers open without
/// autohide and stay up to be checked and captured.
pub(crate) fn popup(popover: &impl IsA<gtk::Popover>, anchor: &impl IsA<gtk::Widget>) {
    let popover = popover.upcast_ref::<gtk::Popover>();
    popover.set_parent(anchor);
    let release = |popover: &gtk::Popover| {
        if popover.parent().is_some() {
            popover.unparent();
        }
    };
    let unrealize = anchor.connect_unrealize(glib::clone!(
        #[weak]
        popover,
        move |_| release(&popover)
    ));
    let anchor = anchor.upcast_ref::<gtk::Widget>().downgrade();
    let unrealize = Cell::new(Some(unrealize));
    // Unparenting from inside `closed` confuses GTK's popover teardown. The anchor keeps
    // its handler until then, in case it leaves the window first.
    popover.connect_closed(move |popover| {
        let (popover, anchor, unrealize) = (popover.clone(), anchor.clone(), unrealize.take());
        glib::idle_add_local_once(move || {
            release(&popover);
            if let (Some(anchor), Some(handler)) = (anchor.upgrade(), unrealize) {
                anchor.disconnect(handler);
            }
        });
    });
    if SCRIPTED.get() {
        popover.set_autohide(false);
    }
    popover.popup();
}

/// The application id for the default profile. Other profiles derive theirs from it.
pub const APP_ID: &str = "dev.mrquantumoff.vsesvit";

static LOGGER: glib::GlibLogger = glib::GlibLogger::new(
    glib::GlibLoggerFormat::Plain,
    glib::GlibLoggerDomain::CrateTarget,
);

/// `vsesvit [URL...] [--profile-dir DIR] | --self-test OUT_DIR [--network] | --check-for-updates | --update`
pub fn run() -> ExitCode {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    let args: Vec<OsString> = std::env::args_os().collect();
    let command = match cli::parse(args.iter().skip(1).cloned()) {
        Ok(command) => command,
        Err(e) => {
            eprintln!("vsesvit: {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("vsesvit {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::CheckForUpdates => updates::headless::check(),
        Command::Update => updates::headless::update(),
        #[cfg(feature = "self-test")]
        Command::SelfTest { out_dir, network } => self_test::run(&out_dir, network),
        #[cfg(not(feature = "self-test"))]
        Command::SelfTest { .. } => {
            eprintln!("vsesvit: this build was made without the `self-test` feature");
            ExitCode::FAILURE
        }
        Command::Browse { profile_dir, .. } => app::run(profile_dir, &args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::wait_until;
    use glib::subclass::signal::SignalId;

    #[gtk::test]
    fn a_popup_leaves_nothing_on_its_anchor_once_it_closes() {
        let anchor = gtk::Button::new();
        let window = gtk::Window::builder().child(&anchor).build();
        window.present();
        wait_until("the anchor on screen", || anchor.is_mapped());
        for _ in 0..3 {
            let popover = gtk::Popover::builder().autohide(false).build();
            popup(&popover, &anchor);
            wait_until("the popover", || popover.is_mapped());
            popover.popdown();
            wait_until("the popover released", || popover.parent().is_none());
        }
        let unrealize = SignalId::lookup("unrealize", gtk::Widget::static_type()).unwrap();
        let left = glib::signal::signal_has_handler_pending(&anchor, unrealize, None, false);
        let open = gtk::Popover::builder().autohide(false).build();
        let closing = gtk::Popover::builder().autohide(false).build();
        for popover in [&open, &closing] {
            popup(popover, &anchor);
            wait_until("the popover", || popover.is_mapped());
        }
        closing.popdown();
        window.destroy();
        assert!(!left, "closed popovers leave no handlers on the anchor");
        assert!(open.parent().is_none(), "an open popover leaves with its anchor");
        assert!(closing.parent().is_none(), "a popover just closed leaves with its anchor too");
    }
}
