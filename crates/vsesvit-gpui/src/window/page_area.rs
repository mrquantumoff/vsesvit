//! Owned by W1.7.
//! The page slot (UA section 1.6, design section 1.4): the element where the selected tab's page
//! shows, and what the window does to its surface, the engine and the focus policy as the slot is
//! laid out. The element reports its bounds in prepaint; the window compares what it wants with what
//! it last did, so a frame changes only what moved, and drawing the same frame twice changes nothing.

use std::collections::HashSet;

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Bounds, Context, Entity, Hsla, IntoElement, ParentElement as _, Pixels, Styled as _, WeakEntity, Window, canvas, div};
use vsesvit_engine::{NativeHandle, PhysRect, TabId};

use super::BrowserWindow;
use crate::app::shell::Shell;
use crate::engine_bridge::{self, PageArea, hooks::TabHook};
use crate::tab::Tab;

/// What the window last did to its surface and told the engine and the focus policy.
#[derive(Debug, Default)]
pub struct PageSlot {
    /// The content host's rectangle in the window's client area, in physical pixels.
    content: Option<PhysRect>,
    scale: Option<f32>,
    /// The page shown in the host.
    shown: Option<(TabId, NativeHandle)>,
    /// The pages whose visibility this window has set, so a page created while another tab was
    /// selected is hidden once.
    placed: HashSet<NativeHandle>,
    /// What the focus policy was last told.
    area: Option<PageArea>,
    /// The hooks' snapshot needs this window's tabs again.
    hooks_stale: bool,
    /// A tab whose page takes the keyboard once it is the one shown.
    keyboard_waits_for: Option<TabId>,
}

impl PageSlot {
    pub fn give_keyboard_to(&mut self, tab: TabId) {
        self.keyboard_waits_for = Some(tab);
    }

    /// GPUI took the keyboard after a page was promised it.
    pub fn give_keyboard_to_chrome(&mut self) {
        self.keyboard_waits_for = None;
    }

    pub fn tabs_changed(&mut self) {
        self.hooks_stale = true;
    }

    /// The page shown now, for tests and the self-test.
    pub fn shown(&self) -> Option<(TabId, NativeHandle)> {
        self.shown
    }

    pub fn content(&self) -> Option<PhysRect> {
        self.content
    }

    /// What the focus policy was last told about the page's place on the screen.
    pub fn area(&self) -> Option<&PageArea> {
        self.area.as_ref()
    }
}

/// The slot's element. It paints `fill` under the page, or nothing at all when the window must stay
/// transparent there (Windows).
pub fn element(window: WeakEntity<BrowserWindow>, fill: Option<Hsla>) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .when_some(fill, |d, fill| d.bg(fill))
        .child(
            canvas(
                move |bounds, w, cx| {
                    window.update(cx, |this, cx| this.lay_out_page(bounds, w, cx)).ok();
                },
                |_, _, _, _| {},
            )
            .size_full(),
        )
}

/// `bounds` in physical pixels, rounded the way the page's window is placed.
pub fn physical(bounds: Bounds<Pixels>, scale: f32) -> PhysRect {
    let x = (f32::from(bounds.origin.x) * scale).round() as i32;
    let y = (f32::from(bounds.origin.y) * scale).round() as i32;
    let right = (f32::from(bounds.right()) * scale).round() as i32;
    let bottom = (f32::from(bounds.bottom()) * scale).round() as i32;
    PhysRect { x, y, width: (right - x).max(0) as u32, height: (bottom - y).max(0) as u32 }
}

impl BrowserWindow {
    /// The slot was laid out at `bounds`: the host follows it, every tab of the window gets its
    /// browser, the selected tab's page shows and the others hide, and the focus policy and the hooks
    /// learn where the page is.
    pub(super) fn lay_out_page(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.probe.page = Some(bounds);
        let Some(surface) = self.surface.clone() else { return };
        let Some(state) = self.state.upgrade() else { return };
        let scale = window.scale_factor();
        let content = physical(bounds, scale);
        let page_size = PhysRect { x: 0, y: 0, width: content.width, height: content.height };
        let selected = self.tabs.read(cx).selected();
        let tabs: Vec<(TabId, Entity<Tab>)> =
            self.tabs.read(cx).tabs().iter().filter_map(|&id| state.read(cx).tab(id).map(|tab| (id, tab))).collect();
        let engine = Shell::engine(cx);
        let Ok(mut host) = surface.try_borrow_mut() else {
            log::error!("window {}: the surface is borrowed; the page is laid out next frame", self.id.0);
            window.refresh();
            return;
        };
        let parent = host.host();

        let moved = self.page.content != Some(content);
        if moved {
            host.set_content_rect(content);
            self.page.content = Some(content);
        }
        let rescaled = self.page.scale.is_some_and(|old| old != scale);
        if self.page.scale != Some(scale) {
            self.page.scale = Some(scale);
            self.page.hooks_stale = true;
        }

        drop(host);
        let Some(shown_changed) = self.show_selected(cx) else { return };
        let want = self.page.shown;
        if let (true, Some((id, _))) = (moved && !shown_changed, want) {
            engine.notify_moved(id);
        }
        if rescaled {
            for (id, tab) in &tabs {
                if tab.read(cx).native().is_some() {
                    engine.notify_screen_info_changed(*id);
                }
            }
        }
        for (id, tab) in &tabs {
            if !tab.read(cx).needs_browser() {
                continue;
            }
            let visible = selected == Some(*id);
            if let Err(e) = tab.update(cx, |tab, cx| tab.create_browser(parent, page_size, scale, visible, cx)) {
                log::warn!("window {}: no browser for tab {}: {e}", self.id.0, id.get());
            }
        }
        self.publish_page_area(content, scale, shown_changed, window, cx);
        if let Some(tab) = self.page.keyboard_waits_for
            && self.page.shown.is_some_and(|(shown, _)| shown == tab)
        {
            self.page.keyboard_waits_for = None;
            engine_bridge::focus(cx).page_wants_keyboard(tab);
        }
        if self.page.hooks_stale || moved {
            self.page.hooks_stale = false;
            let hook = TabHook { browsing: self.browsing, parent, bounds: page_size, scale_factor: scale };
            publish_hooks(tabs.iter().map(|(id, _)| (*id, hook)), cx);
        }
    }

    /// Shows the selected tab's page and hides every other page of the window: at once on a tab switch,
    /// so the focus policy never moves the keyboard to a page still hidden, and again at every layout
    /// for pages created since. Returns whether the shown page changed; `None` while the surface is
    /// busy or not laid out yet.
    pub(super) fn show_selected(&mut self, cx: &mut Context<Self>) -> Option<bool> {
        let surface = self.surface.clone()?;
        self.page.content?;
        let state = self.state.upgrade()?;
        let selected = self.tabs.read(cx).selected();
        let live: Vec<(TabId, NativeHandle)> = self
            .tabs
            .read(cx)
            .tabs()
            .iter()
            .filter_map(|&id| state.read(cx).tab(id).and_then(|tab| tab.read(cx).native()).map(|n| (id, n)))
            .collect();
        let want = live.iter().copied().find(|(id, _)| Some(*id) == selected);
        let mut visibility = Vec::new();
        {
            let Ok(mut host) = surface.try_borrow_mut() else {
                log::error!("window {}: the surface is borrowed; pages are shown next frame", self.id.0);
                return None;
            };
            self.page.placed.retain(|native| live.iter().any(|(_, n)| n == native));
            for &(id, native) in &live {
                if self.page.placed.insert(native) && want != Some((id, native)) {
                    host.show_page(native, false);
                    visibility.push((id, false));
                }
            }
            if self.page.shown != want {
                if let Some((old, native)) = self.page.shown
                    && live.contains(&(old, native))
                {
                    host.show_page(native, false);
                    visibility.push((old, false));
                }
                if let Some((new, native)) = want {
                    host.show_page(native, true);
                    visibility.push((new, true));
                }
            }
        }
        let changed = self.page.shown != want;
        self.page.shown = want;
        let engine = Shell::engine(cx);
        for (id, visible) in visibility {
            engine.set_visible(id, visible);
        }
        Some(changed)
    }

    /// Tells the focus policy where the shown page is on the screen, after every change and after every
    /// `show_page`, which lets the policy retry a page focus the OS refused.
    fn publish_page_area(&mut self, content: PhysRect, scale: f32, shown_changed: bool, window: &Window, cx: &App) {
        let origin = window.bounds().origin;
        let rect = PhysRect {
            x: (f32::from(origin.x) * scale).round() as i32 + content.x,
            y: (f32::from(origin.y) * scale).round() as i32 + content.y,
            ..content
        };
        let holes = self.overlays.read(cx).screen_holes().to_vec();
        let area = self.page.shown.zip(self.toplevel).map(|(_, toplevel)| PageArea { toplevel, rect, holes });
        if area != self.page.area || shown_changed {
            self.page.area = area.clone();
            engine_bridge::focus(cx).set_page_area(self.gpui_id, area);
        }
    }
}

impl BrowserWindow {
    /// The overlays' holes changed: the focus policy learns the page's area without them.
    pub(super) fn holes_changed(&mut self, window: &Window, cx: &App) {
        if let (Some(content), Some(scale)) = (self.page.content, self.page.scale) {
            self.publish_page_area(content, scale, false, window, cx);
        }
    }
}

/// Writes the hooks' answers for these tabs; a snapshot the engine is reading is written next frame.
fn publish_hooks(tabs: impl Iterator<Item = (TabId, TabHook)>, cx: &App) {
    let snapshot = engine_bridge::hooks::snapshot(cx);
    let Ok(mut snapshot) = snapshot.try_borrow_mut() else { return };
    snapshot.tabs.extend(tabs);
}

/// Forgets a tab that left its window.
pub(super) fn forget_hook(tab: TabId, cx: &App) {
    let snapshot = engine_bridge::hooks::snapshot(cx);
    if let Ok(mut snapshot) = snapshot.try_borrow_mut() {
        snapshot.tabs.remove(&tab);
    }
}
