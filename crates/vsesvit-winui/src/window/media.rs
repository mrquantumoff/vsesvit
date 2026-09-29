//! The window's sidebar player: which tab it follows, what it shows, and what its
//! picture-in-picture box shows while the tab is not on screen: the tab's own web view
//! presenting its video, or for sound alone the page's artwork.
//!
//! The player follows the tab that started playing sound last, until the tab closes or shows a
//! page with nothing to play. Its web view goes into the box only while the vertical pane is
//! expanded, another tab is selected, the page has a video, and the tabs leave room for the
//! box; once the tabs reach it, the box goes and the controls stay.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::exec;
use crate::layout::StripKind;
use crate::media::{MediaAction, Playback};
use crate::player::PlayerLook;
use crate::tab::{Tab, TabId};

/// What the picture-in-picture box shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pip {
    /// The tab's web view, which shows only its playing video.
    Video(TabId),
    /// The artwork of what a page without video plays.
    Artwork(String),
}

#[derive(Default)]
pub(super) struct MediaState {
    /// The tab the player follows.
    tab: Cell<Option<TabId>>,
    playback: RefCell<Option<Playback>>,
    pip: RefCell<Option<Pip>>,
    /// Counts playback queries, so only the latest answer is shown.
    query: Cell<u64>,
}

/// Lets a page react to a player button before it is asked what it plays.
const AFTER_ACTION: Duration = Duration::from_millis(300);

impl BrowserWindow {
    /// A tab started or stopped playing sound, or was muted or unmuted.
    pub fn tab_audio_changed(&self, tab: &Tab) {
        if tab.state().audible {
            self.media.tab.set(Some(tab.id));
        }
        if self.media.tab.get() == Some(tab.id) {
            self.refresh_media();
        }
    }

    /// The followed tab's playback, asked from its page, then shown.
    pub(super) fn refresh_media(&self) {
        let query = self.media.query.get() + 1;
        self.media.query.set(query);
        let Some(tab) = self.media.tab.get().and_then(|id| self.tab(id)) else {
            self.show_media();
            return;
        };
        let me = self.me.clone();
        exec::spawn(async move {
            let playback = tab.playback().await;
            let Some(window) = me.upgrade() else { return };
            if window.media.query.get() != query {
                return;
            }
            let idle = playback.as_ref().is_none_or(|p| {
                !p.playing && !p.video && !p.previous && !p.next && p.title.is_empty()
            });
            if idle && !tab.state().audible && window.media.tab.get() == Some(tab.id) {
                window.media.tab.set(None);
            }
            *window.media.playback.borrow_mut() = playback;
            window.show_media();
        });
    }

    pub(super) fn show_media(&self) {
        let tab = self.media.tab.get().and_then(|id| self.tab(id));
        let look = tab.map(|tab| {
            let state = tab.state();
            let playback = self.media.playback.borrow().clone().unwrap_or_default();
            PlayerLook {
                title: if playback.title.is_empty() {
                    state.title
                } else {
                    playback.title
                },
                artist: playback.artist,
                favicon: tab.look().favicon,
                playing: playback.playing,
                muted: state.muted,
                previous: playback.previous,
                next: playback.next,
            }
        });
        self.player.show(look.as_ref());
        self.update_pip();
    }

    /// Fills the picture-in-picture box as the window's state asks, moving the followed tab's
    /// web view into it or back to the page grid.
    pub(super) fn update_pip(&self) {
        let wanted = self.pip_wanted();
        let current = self.media.pip.borrow().clone();
        if wanted == current {
            return;
        }
        match current {
            Some(Pip::Video(id)) => self.release_pip(id),
            Some(Pip::Artwork(_)) => self.player.set_artwork(None),
            None => {}
        }
        *self.media.pip.borrow_mut() = wanted.clone();
        match &wanted {
            Some(Pip::Video(id)) => {
                if let Err(e) = self.attach_pip(*id) {
                    log::warn!("picture in picture: {e}");
                    self.release_pip(*id);
                    *self.media.pip.borrow_mut() = None;
                }
            }
            Some(Pip::Artwork(url)) => self.player.set_artwork(Some(url)),
            None => {}
        }
        self.player.set_pip_visible(self.media.pip.borrow().is_some());
        self.place_views(self.active_tab().map(|t| t.id));
    }

    pub(super) fn in_pip(&self, id: TabId) -> bool {
        *self.media.pip.borrow() == Some(Pip::Video(id))
    }

    fn pip_wanted(&self) -> Option<Pip> {
        let id = self.media.tab.get()?;
        let pane = StripKind::of(self.tabs_position.get()) == StripKind::Side
            && !self.side.is_compact()
            && !self.fullscreen.get();
        let active = self.active_tab().map(|t| t.id);
        let on_screen = active == Some(id)
            || self
                .split
                .get()
                .is_some_and(|s| s.has(id) && active.is_some_and(|a| s.has(a)));
        let content = match self.media.playback.borrow().as_ref() {
            Some(playback) if playback.video => Some(Pip::Video(id)),
            Some(playback) if !playback.artwork.is_empty() => {
                Some(Pip::Artwork(playback.artwork.clone()))
            }
            _ => None,
        };
        // The free space counts the box while it is shown, so both states agree on the room.
        let block = self.player.pip_block();
        let shown = if self.media.pip.borrow().is_some() {
            block
        } else {
            0.0
        };
        let room = self.side.free_height() + shown >= block;
        content.filter(|_| pane && !on_screen && room)
    }

    fn attach_pip(&self, id: TabId) -> Result<()> {
        let tab = self.tab(id).ok_or_else(windows_core::Error::empty)?;
        let view = tab.view().cast::<UIElement>()?;
        remove_child(&self.ui.pages, &view)?;
        if let Ok(element) = view.cast::<FrameworkElement>() {
            Grid::SetColumn(&element, 0)?;
            Grid::SetColumnSpan(&element, 1)?;
        }
        self.player.pip_host().Children()?.InsertAt(0, &view)?;
        view.SetVisibility(Visibility::Visible)?;
        exec::spawn(async move {
            if !tab.present_pip(true).await {
                log::debug!("tab {}: no video to show in picture in picture", tab.id);
            }
        });
        Ok(())
    }

    fn release_pip(&self, id: TabId) {
        let Some(tab) = self.tab(id) else { return };
        let moved = tab.view().cast::<UIElement>().and_then(|view| {
            remove_child(self.player.pip_host(), &view)?;
            view.SetVisibility(Visibility::Collapsed)?;
            self.ui.pages.Children()?.Append(&view)
        });
        if let Err(e) = moved {
            log::warn!("picture in picture: {e}");
        }
        exec::spawn(async move {
            tab.present_pip(false).await;
        });
    }

    /// A closing tab leaves the player; its web view is back in the page grid for removal.
    pub(super) fn media_tab_closing(&self, id: TabId) {
        if self.in_pip(id) {
            self.release_pip(id);
            *self.media.pip.borrow_mut() = None;
            self.player.set_pip_visible(false);
        }
        if self.media.tab.get() == Some(id) {
            // Another tab still playing takes over.
            let playing = self
                .tabs
                .borrow()
                .iter()
                .find(|t| t.id != id && t.state().audible)
                .map(|t| t.id);
            self.media.tab.set(playing);
            *self.media.playback.borrow_mut() = None;
        }
    }

    pub(super) fn player_go_to_tab(&self) {
        if let Some(id) = self.media.tab.get() {
            let _ = self.strip().select(id);
            self.sync_selection();
        }
    }

    /// A player button: the page's own action, then the player catches up.
    pub(crate) fn player_action(&self, action: MediaAction) {
        let Some(tab) = self.media.tab.get().and_then(|id| self.tab(id)) else {
            return;
        };
        let me = self.me.clone();
        exec::spawn(async move {
            tab.media_action(action).await;
            exec::sleep(AFTER_ACTION).await;
            if let Some(window) = me.upgrade() {
                window.refresh_media();
            }
        });
    }

    pub(super) fn player_toggle_muted(&self) {
        if let Some(tab) = self.media.tab.get().and_then(|id| self.tab(id)) {
            tab.set_muted(!tab.state().muted);
        }
    }

    /// The tab the player follows, for scripted runs.
    pub fn media_tab(&self) -> Option<TabId> {
        self.media.tab.get()
    }

    /// The tab whose web view is in the picture-in-picture box, for scripted runs.
    pub fn pip_tab(&self) -> Option<TabId> {
        match *self.media.pip.borrow() {
            Some(Pip::Video(id)) => Some(id),
            _ => None,
        }
    }

    /// The artwork the picture-in-picture box shows, for scripted runs.
    pub fn pip_artwork(&self) -> Option<String> {
        match &*self.media.pip.borrow() {
            Some(Pip::Artwork(url)) => Some(url.clone()),
            _ => None,
        }
    }

    pub fn player_shown(&self) -> bool {
        crate::xaml::is_visible(self.player.element())
    }
}

fn remove_child(panel: &Panel, child: &UIElement) -> Result<()> {
    let children = panel.Children()?;
    let mut index = 0;
    if children.IndexOf(child, &mut index)? {
        children.RemoveAt(index)?;
    }
    Ok(())
}
