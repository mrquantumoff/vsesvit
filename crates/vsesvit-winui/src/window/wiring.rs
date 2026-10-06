//! Event handlers of the window's own controls, its keyboard accelerators, and the events of
//! its tab lists.

use std::cell::OnceCell;
use std::rc::{Rc, Weak};

use windows_core::{Interface, Result};

use super::{BrowserWindow, MenuAction};
use crate::bindings::*;
use crate::dialogs::{Dialog, on_click};
use crate::exec;
use crate::shortcuts::{self, Command, Mods};
use crate::player::PlayerEvents;
use crate::strip::StripEvents;
use crate::{platform, xaml, zoom};

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
        on_click(&ui.bookmarks_overflow, move || {
            with(&w, |w| {
                if let Err(e) = w.show_bookmarks_overflow() {
                    log::warn!("bookmarks overflow menu: {e}");
                }
            });
        })?;

        let w = me();
        on_click(&ui.back, move || with(&w, |w| w.run(Command::Back)))?;
        let w = me();
        on_click(&ui.forward, move || with(&w, |w| w.run(Command::Forward)))?;
        let w = me();
        on_click(&ui.reload, move || {
            with(&w, |w| {
                if let Some(tab) = w.active_tab() {
                    tab.reload_or_stop();
                }
            });
        })?;
        let w = me();
        on_click(&ui.home, move || with(&w, |w| w.go_home()))?;
        let w = me();
        on_click(&ui.star, move || with(&w, |w| w.star_clicked()))?;
        let w = me();
        on_click(&ui.pip, move || with(&w, |w| w.pip_clicked()))?;
        self.wire_split_divider()?;
        let w = me();
        on_click(&ui.copy_link, move || {
            with(&w, |w| w.run(Command::CopyCleanLink))
        })?;
        let w = me();
        on_click(&ui.site_button, move || {
            with(&w, |w| {
                if let Err(e) = w.show_connection() {
                    log::warn!("connection popup: {e}");
                }
            });
        })?;
        let w = me();
        on_click(&ui.update_action, move || with(&w, |w| w.update_clicked()))?;
        for (name, step) in [
            ("ZoomIn", zoom::Step::In),
            ("ZoomOut", zoom::Step::Out),
            ("ZoomReset", zoom::Step::Reset),
        ] {
            let w = me();
            let button: Button = xaml::find(&ui.root, name)?;
            on_click(&button, move || with(&w, |w| w.zoom_clicked(step)))?;
        }
        let w = me();
        on_click(&ui.downloads, move || {
            with(&w, |w| w.show_dialog(Dialog::Downloads));
        })?;
        let w = me();
        on_click(&ui.tab_search, move || with(&w, |w| w.run(Command::SearchTabs)))?;

        let w = me();
        ui.address
            .TextChanged(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                let typed = args
                    .Reason()
                    .is_ok_and(|r| r == AutoSuggestionBoxTextChangeReason::UserInput);
                w.address_text_changed(typed);
            })?
            .forget();
        let w = me();
        ui.address
            .cast::<FrameworkElement>()?
            .Loaded(move |_, _| with(&w, BrowserWindow::watch_suggestion_list))?
            .forget();
        let w = me();
        ui.address
            .cast::<UIElement>()?
            .PreviewKeyDown(move |_, args| {
                let (Some(w), Some(args)) = (w.upgrade(), args.as_ref()) else {
                    return;
                };
                let vk = args.Key().map_or(0, |k| u16::try_from(k.0).unwrap_or(0));
                if w.address_key_down(vk, platform::held_modifiers()) {
                    let _ = args.SetHandled(true);
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
            ("MenuSavePage", MenuAction::Run(Command::SavePage)),
            ("MenuPrint", MenuAction::Run(Command::Print)),
            (
                "MenuDeveloperTools",
                MenuAction::Run(Command::DeveloperTools),
            ),
            ("MenuViewSource", MenuAction::Run(Command::ViewSource)),
            ("MenuSettings", MenuAction::Show(Dialog::Settings)),
            ("MenuWelcome", MenuAction::Show(Dialog::Welcome)),
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
        xaml::find::<FlyoutBase>(&ui.root, "MainMenu")?
            .Opening(move |_, _| with(&w, BrowserWindow::menu_opening))?
            .forget();

        let w = me();
        self.window
            .Activated(move |_, args| {
                let state = args.as_ref().and_then(|a| a.WindowActivationState().ok());
                if state != Some(WindowActivationState::Deactivated) {
                    with(&w, BrowserWindow::window_activated);
                }
            })?
            .forget();
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
        self.set_accelerators()
    }

    /// The window's accelerators from the bindings in effect; none while a shortcut is being
    /// captured, so the key pressed for it runs nothing.
    pub(super) fn set_accelerators(&self) -> Result<()> {
        let accelerators = self.ui.root.cast::<UIElement>()?.KeyboardAccelerators()?;
        accelerators.Clear()?;
        if self.shortcuts_suspended.get() {
            return Ok(());
        }
        for binding in shortcuts::current().list() {
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
        self.close_scripted_dialog();
        let browser = self.browser();
        if let Some(browser) = &browser {
            browser.window_closing(self);
        }
        for tab in self.tabs.take() {
            tab.close();
        }
        // XAML keeps the text box that last had the focus until it shuts down, and then
        // destroys it after the window's input site is gone, which crashes; releasing the
        // content now lets it go while the window still exists.
        let _ = self.window.SetContent(None::<&UIElement>);
        if let Some(browser) = browser {
            browser.window_closed(self);
        }
    }
}

/// The window is created after its tab lists, so their events reach it through `slot`.
pub(super) type WindowSlot = Rc<OnceCell<Weak<BrowserWindow>>>;

/// Runs a callback on the window in `slot`, if it is still open.
fn on(slot: &WindowSlot) -> impl Fn(&dyn Fn(&BrowserWindow)) + 'static {
    let slot = slot.clone();
    move |f| {
        if let Some(window) = slot.get().and_then(Weak::upgrade) {
            f(&window);
        }
    }
}

/// Runs `run` on the window in `slot` on the next turn, so the control that fired it finishes
/// first: a command may rebuild or remove the menu, button or row it came from.
pub(super) fn later<C: 'static>(
    slot: &WindowSlot,
    run: fn(&BrowserWindow, C),
) -> impl Fn(C) + 'static {
    let slot = slot.clone();
    move |c| {
        let window = slot.get().cloned();
        exec::spawn(async move {
            if let Some(window) = window.and_then(|w| w.upgrade()) {
                run(&window, c);
            }
        });
    }
}

pub(super) fn strip_events(slot: &WindowSlot) -> StripEvents {
    let w = on(slot);
    let selection_changed = Box::new(move |kind| w(&|w| w.strip_selection_changed(kind)));
    // A close button's click must finish before its row goes away.
    let close = Box::new(later(slot, BrowserWindow::close_tab));
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
    let search_tabs = Box::new(move || w(&|w| w.run(Command::SearchTabs)));
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
        search_tabs,
        toggle_muted,
        menu,
        pane_space_changed,
        pane_resized,
    }
}

pub(super) fn player_events(slot: &WindowSlot) -> PlayerEvents {
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
    if mods.has(Mods::CTRL) {
        bits |= VirtualKeyModifiers::Control.0;
    }
    if mods.has(Mods::SHIFT) {
        bits |= VirtualKeyModifiers::Shift.0;
    }
    if mods.has(Mods::ALT) {
        bits |= VirtualKeyModifiers::Menu.0;
    }
    VirtualKeyModifiers(bits)
}

pub(super) fn with(window: &Weak<BrowserWindow>, f: impl FnOnce(&BrowserWindow)) {
    if let Some(window) = window.upgrade() {
        f(&window);
    }
}
