//! Event handlers of the window's own controls, its keyboard accelerators, and the events of
//! its tab lists.

use std::cell::OnceCell;
use std::rc::{Rc, Weak};

use windows_core::{Interface, Result};

use super::{BrowserWindow, MenuAction};
use crate::bindings::*;
use crate::dialogs::Dialog;
use crate::exec;
use crate::shortcuts::{BINDINGS, Command, Mods};
use crate::player::PlayerEvents;
use crate::strip::StripEvents;
use crate::{xaml, zoom};

impl BrowserWindow {
    pub(super) fn wire(&self) -> Result<()> {
        let me = || self.me.clone();
        let ui = &self.ui;

        let w = me();
        ui.toolbar
            .cast::<FrameworkElement>()?
            .LayoutUpdated(move |_, _| with(&w, |w| w.update_drag_regions()))?
            .forget();

        let bar = ui.bookmark_items.cast::<ListViewBase>()?;
        let w = me();
        bar.ItemClick(move |_, args| {
            if let Some(item) = args.as_ref().and_then(|a| a.ClickedItem().ok()) {
                with(&w, |w| w.bar_item_clicked(&item));
            }
        })?
        .forget();
        let w = me();
        bar.DragItemsCompleted(move |_, _| with(&w, BrowserWindow::bar_item_dropped))?
            .forget();
        let w = me();
        ui.bookmarks_bar
            .SizeChanged(move |_, _| with(&w, BrowserWindow::fit_bookmarks_bar))?
            .forget();
        let w = me();
        click(&ui.bookmarks_overflow, move || {
            with(&w, |w| {
                if let Err(e) = w.show_bookmarks_overflow() {
                    log::warn!("bookmarks overflow menu: {e}");
                }
            });
        })?;

        let w = me();
        click(&ui.back, move || with(&w, |w| w.run(Command::Back)))?;
        let w = me();
        click(&ui.forward, move || with(&w, |w| w.run(Command::Forward)))?;
        let w = me();
        click(&ui.reload, move || {
            with(&w, |w| {
                if let Some(tab) = w.active_tab() {
                    tab.reload_or_stop();
                }
            });
        })?;
        let w = me();
        click(&ui.home, move || with(&w, |w| w.go_home()))?;
        let w = me();
        click(&ui.star, move || with(&w, |w| w.star_clicked()))?;
        let w = me();
        click(&ui.copy_link, move || with(&w, |w| w.run(Command::CopyCleanLink)))?;
        let w = me();
        click(&ui.site_button, move || {
            with(&w, |w| {
                if let Err(e) = w.show_connection() {
                    log::warn!("connection popup: {e}");
                }
            });
        })?;
        let w = me();
        click(&ui.update_action, move || with(&w, |w| w.update_clicked()))?;
        for (name, step) in [
            ("ZoomIn", zoom::Step::In),
            ("ZoomOut", zoom::Step::Out),
            ("ZoomReset", zoom::Step::Reset),
        ] {
            let w = me();
            let button: Button = xaml::find(&ui.root, name)?;
            click(&button, move || with(&w, |w| w.zoom_clicked(step)))?;
        }
        let w = me();
        click(&ui.downloads, move || {
            with(&w, |w| w.show_dialog(Dialog::Downloads));
        })?;

        let w = me();
        ui.address
            .TextChanged(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                if args
                    .Reason()
                    .is_ok_and(|r| r == AutoSuggestionBoxTextChangeReason::UserInput)
                {
                    w.address_edited_by_user();
                }
            })?
            .forget();
        let w = me();
        ui.address
            .QuerySubmitted(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                let chosen = args
                    .ChosenSuggestion()
                    .ok()
                    .and_then(|c| {
                        c.cast::<windows_reference::IReference<windows_core::HSTRING>>()
                            .ok()
                    })
                    .and_then(|c| c.Value().ok())
                    .map(|c| c.to_string_lossy());
                let text = chosen.unwrap_or_else(|| args.QueryText().unwrap_or_default());
                w.address_submitted(&text);
            })?
            .forget();
        let w = me();
        ui.address
            .cast::<UIElement>()?
            .GotFocus(move |_, _| with(&w, |w| w.address_focus_changed(true)))?
            .forget();
        let w = me();
        ui.address
            .cast::<UIElement>()?
            .LostFocus(move |_, _| with(&w, |w| w.address_focus_changed(false)))?
            .forget();

        let menu = [
            ("MenuNewTab", MenuAction::Run(Command::NewTab)),
            ("MenuNewWindow", MenuAction::Run(Command::NewWindow)),
            ("MenuBookmarks", MenuAction::Show(Dialog::Bookmarks)),
            ("MenuHistory", MenuAction::Show(Dialog::History)),
            ("MenuDownloads", MenuAction::Show(Dialog::Downloads)),
            ("MenuExtensions", MenuAction::Show(Dialog::Extensions)),
            ("MenuSettings", MenuAction::Show(Dialog::Settings)),
            ("MenuAbout", MenuAction::Show(Dialog::About)),
        ];
        for (name, action) in menu {
            let item: MenuFlyoutItem = xaml::find(&ui.root, name)?;
            let w = me();
            item.Click(move |_, _| {
                if let Some(w) = w.upgrade() {
                    match action {
                        MenuAction::Run(command) => w.run(command),
                        MenuAction::Show(dialog) => w.show_dialog(dialog),
                    }
                }
            })?
            .forget();
        }

        let w = me();
        self.window
            .Closed(move |_, _| {
                if let Some(w) = w.upgrade() {
                    w.on_closed();
                }
            })?
            .forget();
        Ok(())
    }

    pub(super) fn install_accelerators(&self) -> Result<()> {
        let root = self.ui.root.cast::<UIElement>()?;
        root.SetKeyboardAcceleratorPlacementMode(KeyboardAcceleratorPlacementMode::Hidden)?;
        let accelerators = root.KeyboardAccelerators()?;
        for binding in BINDINGS {
            let accelerator = KeyboardAccelerator::new()?;
            accelerator.SetKey(VirtualKey(i32::from(binding.vk)))?;
            accelerator.SetModifiers(virtual_key_modifiers(binding.mods))?;
            let w = self.me.clone();
            let command = binding.command;
            accelerator
                .Invoked(move |_, args| {
                    if let Some(args) = args.as_ref() {
                        let _ = args.SetHandled(true);
                    }
                    if let Some(w) = w.upgrade() {
                        w.run(command);
                    }
                })?
                .forget();
            accelerators.Append(&accelerator)?;
        }
        Ok(())
    }

    pub(super) fn on_closed(&self) {
        if self.closed.replace(true) {
            return;
        }
        let browser = self.browser();
        if let Some(browser) = &browser {
            browser.window_closing(self);
        }
        for tab in self.tabs.take() {
            tab.close();
        }
        if let Some(browser) = browser {
            browser.window_closed(self);
        }
    }
}

/// The window is created after its tab lists, so their events reach it through `slot`.
pub(super) type WindowSlot = Rc<OnceCell<Weak<BrowserWindow>>>;

pub(super) fn strip_events(slot: &WindowSlot) -> StripEvents {
    let on = |slot: &WindowSlot| {
        let slot = slot.clone();
        move |f: &dyn Fn(&BrowserWindow)| {
            if let Some(window) = slot.get().and_then(Weak::upgrade) {
                f(&window);
            }
        }
    };
    let w = on(slot);
    let selection_changed = Box::new(move |kind| w(&|w| w.strip_selection_changed(kind)));
    let w = slot.clone();
    let close = Box::new(move |id| {
        // A close button's click must finish before its row goes away.
        let w = w.get().cloned();
        exec::spawn(async move {
            if let Some(w) = w.and_then(|w| w.upgrade()) {
                w.close_tab(id);
            }
        });
    });
    let w = on(slot);
    let new_tab = Box::new(move || w(&|w| w.run(Command::NewTab)));
    let w = on(slot);
    let reordered = Box::new(move || {
        w(&|w| {
            w.keep_pinned_first();
            if let Some(browser) = w.browser() {
                browser.session_changed();
            }
        });
    });
    let w = on(slot);
    let toggle_collapsed = Box::new(move || {
        w(&|w| {
            if let Some(browser) = w.browser() {
                browser.set_tab_pane_collapsed(!w.is_pane_collapsed());
            }
        });
    });
    let w = on(slot);
    let toggle_muted = Box::new(move |id| {
        w(&|w| {
            if let Some(tab) = w.tab(id) {
                tab.set_muted(!tab.state().muted);
            }
        });
    });
    let w = on(slot);
    let pane_resized = Box::new(move |width: f64| {
        w(&|w| {
            if let Some(browser) = w.browser() {
                browser.set_tab_pane_width(width as u32);
            }
        });
    });
    let w = on(slot);
    let pane_space_changed = Box::new(move || w(&|w| w.update_pip()));
    let w = on(slot);
    let menu = Box::new(move |id, menu: &MenuFlyout| w(&|w| w.fill_tab_menu(id, menu)));
    StripEvents {
        selection_changed,
        close,
        new_tab,
        reordered,
        toggle_collapsed,
        toggle_muted,
        menu,
        pane_space_changed,
        pane_resized,
    }
}

pub(super) fn player_events(slot: &WindowSlot) -> PlayerEvents {
    let on = |slot: &WindowSlot| {
        let slot = slot.clone();
        move |f: &dyn Fn(&BrowserWindow)| {
            if let Some(window) = slot.get().and_then(Weak::upgrade) {
                f(&window);
            }
        }
    };
    let w = on(slot);
    let go_to_tab = Box::new(move || w(&|w| w.player_go_to_tab()));
    let w = on(slot);
    let action = Box::new(move |action| w(&|w| w.player_action(action)));
    let w = on(slot);
    let toggle_muted = Box::new(move || w(&|w| w.player_toggle_muted()));
    PlayerEvents {
        go_to_tab,
        action,
        toggle_muted,
    }
}

fn virtual_key_modifiers(mods: Mods) -> VirtualKeyModifiers {
    let mut bits = 0;
    if mods.bits() & Mods::CTRL.bits() != 0 {
        bits |= VirtualKeyModifiers::Control.0;
    }
    if mods.bits() & Mods::SHIFT.bits() != 0 {
        bits |= VirtualKeyModifiers::Shift.0;
    }
    if mods.bits() & Mods::ALT.bits() != 0 {
        bits |= VirtualKeyModifiers::Menu.0;
    }
    VirtualKeyModifiers(bits)
}

pub(super) fn with(window: &Weak<BrowserWindow>, f: impl FnOnce(&BrowserWindow)) {
    if let Some(window) = window.upgrade() {
        f(&window);
    }
}

pub(super) fn click(button: &impl Interface, handler: impl Fn() + 'static) -> Result<()> {
    button
        .cast::<ButtonBase>()?
        .Click(move |_, _| handler())?
        .forget();
    Ok(())
}
