//! Window actions (`win.*`). Their keyboard shortcuts are set application-wide in `app.rs`.

use adw::prelude::*;
use gtk::gio::ActionEntry;
use gtk::glib;
use vsesvit_core::history::Transition;
use webkit::prelude::*;

use super::{BrowserWindow, Focus};
use crate::bookmarks_bar;
use crate::dialogs;
use crate::find_bar::Direction;
use crate::tab::Tab;
use crate::zoom;

pub(super) fn install(window: &BrowserWindow) {
    let on_tab = |name: &str, f: fn(&Tab)| {
        ActionEntry::builder(name)
            .activate(move |window: &BrowserWindow, _, _| {
                if let Some(tab) = window.selected_tab() {
                    f(&tab);
                }
            })
            .build()
    };

    window.add_action_entries([
        ActionEntry::builder("new-tab")
            .activate(|w: &BrowserWindow, _, _| w.new_tab())
            .build(),
        ActionEntry::builder("close-tab")
            .activate(|w: &BrowserWindow, _, _| w.close_selected_tab())
            .build(),
        ActionEntry::builder("reopen-closed-tab")
            .activate(|w: &BrowserWindow, _, _| w.browser().reopen_closed_tab(w))
            .build(),
        ActionEntry::builder("focus-location")
            .activate(|w: &BrowserWindow, _, _| w.address_bar().focus_for_typing())
            .build(),
        on_tab("reload", |tab| {
            tab.set_pending_transition(Transition::Reload);
            tab.web_view().reload();
        }),
        on_tab("reload-bypass-cache", |tab| {
            tab.set_pending_transition(Transition::Reload);
            tab.web_view().reload_bypass_cache();
        }),
        on_tab("stop", |tab| tab.web_view().stop_loading()),
        on_tab("back", |tab| tab.web_view().go_back()),
        on_tab("forward", |tab| tab.web_view().go_forward()),
        ActionEntry::builder("bookmark-page")
            .state(false.to_variant())
            .activate(|w: &BrowserWindow, _, _| w.browser().star_clicked(w))
            .build(),
        ActionEntry::builder("open-bookmark")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|w: &BrowserWindow, _, target| {
                if let Some(url) = bookmarks_bar::url_from_target(target) {
                    w.navigate_with(&url, Transition::Bookmark);
                }
            })
            .build(),
        ActionEntry::builder("open-in-new-tab")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|w: &BrowserWindow, _, target| {
                if let Some(url) = bookmarks_bar::url_from_target(target) {
                    w.open_tab(Some(&url), None, Focus::Foreground);
                }
            })
            .build(),
        ActionEntry::builder("find")
            .activate(|w: &BrowserWindow, _, _| {
                if let Some(tab) = w.selected_tab() {
                    w.ui().find_bar.open(tab.web_view());
                }
            })
            .build(),
        ActionEntry::builder("find-next")
            .activate(|w: &BrowserWindow, _, _| w.ui().find_bar.step(Direction::Next))
            .build(),
        ActionEntry::builder("find-previous")
            .activate(|w: &BrowserWindow, _, _| w.ui().find_bar.step(Direction::Previous))
            .build(),
        on_tab("zoom-in", |tab| zoom_tab(tab, Some(zoom::Step::In))),
        on_tab("zoom-out", |tab| zoom_tab(tab, Some(zoom::Step::Out))),
        on_tab("zoom-reset", |tab| zoom_tab(tab, None)),
        ActionEntry::builder("fullscreen")
            .activate(|w: &BrowserWindow, _, _| {
                if w.is_fullscreen() {
                    w.unfullscreen();
                } else {
                    w.fullscreen();
                }
            })
            .build(),
        ActionEntry::builder("toggle-tab-sidebar")
            .activate(|w: &BrowserWindow, _, _| w.toggle_tab_sidebar())
            .build(),
        ActionEntry::builder("show-bookmarks-bar")
            .state(true.to_variant())
            .change_state(|w: &BrowserWindow, _, value| {
                if let Some(shown) = value.and_then(glib::Variant::get::<bool>) {
                    w.browser().set_bookmarks_bar_visible(shown);
                }
            })
            .build(),
        ActionEntry::builder("show-bookmarks")
            .activate(|w: &BrowserWindow, _, _| dialogs::bookmarks::present(w))
            .build(),
        ActionEntry::builder("show-history")
            .activate(|w: &BrowserWindow, _, _| dialogs::history::present(w))
            .build(),
        ActionEntry::builder("show-extensions")
            .activate(|w: &BrowserWindow, _, _| dialogs::extensions::present(w))
            .build(),
        ActionEntry::builder("show-settings")
            .activate(|w: &BrowserWindow, _, _| dialogs::settings::present(w))
            .build(),
    ]);
    // The target of "(Empty)" folder menu entries: never enabled, so they stay inert.
    let none = gtk::gio::SimpleAction::new("none", None);
    none.set_enabled(false);
    window.add_action(&none);
}

fn zoom_tab(tab: &Tab, step: Option<zoom::Step>) {
    let web_view = tab.web_view();
    let level = step.map_or(zoom::DEFAULT, |step| {
        zoom::step(web_view.zoom_level(), step)
    });
    web_view.set_zoom_level(level);
}
