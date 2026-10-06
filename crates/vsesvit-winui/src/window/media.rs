//! The window's sidebar player: which tab it follows, what it shows, and what its
//! picture-in-picture box shows while the tab is not on screen: the tab's own web view
//! presenting its video, or for sound alone the page's artwork.
//!
//! The player follows the tab that started playing sound last, until the tab closes or shows a
//! page with nothing to play. Its web view goes into the box only while the vertical pane is
//! expanded, another tab is selected, the page has a video, its site allowed picture-in-picture
//! ([`Permission::PictureInPicture`]), and the tabs leave room for the box; once the tabs reach
//! it, the box goes and the controls stay. Settings can turn off picture-in-picture, or the
//! whole player and the box with it.
//!
//! While the selected tab plays a video, the address bar's picture-in-picture button turns it
//! on or off for the tab's site. Turning it on also puts the video in the box at once, the
//! page's place showing where it went, until another tab is selected or the box is clicked.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use vsesvit_core::permissions::{Permission, Setting};
use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::layout::StripKind;
use crate::media::{MediaAction, Playback};
use crate::player::PlayerLook;
use crate::tab::{Tab, TabId};
use crate::{exec, permissions, xaml};

/// What the picture-in-picture box shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pip {
    /// The tab's web view, which shows only its playing video.
    Video(TabId),
    /// The artwork of what a page without video plays.
    Artwork(String),
}

/// The Settings switches of the player and its picture-in-picture box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Switches {
    pub player: bool,
    pub pip: bool,
}

impl Default for Switches {
    fn default() -> Self {
        Switches {
            player: true,
            pip: true,
        }
    }
}

impl Switches {
    /// The box goes with the player.
    fn pip_shown(self) -> bool {
        self.player && self.pip
    }
}

/// The toolbar's picture-in-picture button: `None` hides it, else whether it shows on. It shows
/// while the selected tab plays a video (`video_here`) on a site that can remember the choice
/// (`allowed` is whether that site allowed picture-in-picture), and the player and its
/// picture-in-picture are on.
fn pip_button(switches: Switches, video_here: bool, allowed: Option<bool>) -> Option<bool> {
    if switches.pip_shown() && video_here {
        allowed
    } else {
        None
    }
}

/// What a click on the button stores for the site, and whether the video goes into the box at
/// once.
fn pip_click(allowed: bool) -> (Setting, bool) {
    if allowed {
        (Setting::Block, false)
    } else {
        (Setting::Allow, true)
    }
}

/// What the box would show for tab `id` playing `playback`: its video when its site allowed
/// picture-in-picture, else the artwork of what it plays.
fn pip_content(playback: Option<&Playback>, id: TabId, video_allowed: bool) -> Option<Pip> {
    let playback = playback?;
    if playback.video && video_allowed {
        Some(Pip::Video(id))
    } else if !playback.artwork.is_empty() {
        Some(Pip::Artwork(playback.artwork.clone()))
    } else {
        None
    }
}

#[derive(Default)]
pub(super) struct MediaState {
    switches: Cell<Switches>,
    /// The tab the player follows.
    tab: Cell<Option<TabId>>,
    playback: RefCell<Option<Playback>>,
    pip: RefCell<Option<Pip>>,
    /// The selected tab whose video the toolbar's button put in the box.
    requested: Cell<Option<TabId>>,
    /// Counts playback queries, so only the latest answer is shown.
    query: Cell<u64>,
}

/// Lets a page react to a player button before it is asked what it plays.
const AFTER_ACTION: Duration = Duration::from_millis(300);

impl BrowserWindow {
    /// Shows or hides the player and its picture-in-picture box as Settings say.
    pub fn set_media_switches(&self, player: bool, pip: bool) {
        self.media.switches.set(Switches { player, pip });
        self.show_media();
    }

    /// A tab started or stopped playing sound, or was muted or unmuted.
    pub fn tab_audio_changed(&self, tab: &Tab) {
        if tab.state().audible {
            self.media.tab.set(Some(tab.id));
            if self.media.requested.get().is_some_and(|id| id != tab.id) {
                self.media.requested.set(None);
            }
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

    /// Shows what the followed tab plays, and the box and the toolbar's button as they follow.
    pub(crate) fn show_media(&self) {
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
        let shown = look.filter(|_| self.media.switches.get().player);
        self.player.show(shown.as_ref());
        self.update_pip();
        self.show_pip_button();
    }

    /// Whether the site of `tab` allowed picture-in-picture; `None` for a page without one.
    fn pip_allowed(&self, tab: &Tab) -> Option<bool> {
        let origin = tab.origin()?;
        let browser = self.browser()?;
        let setting = browser.core(|p| {
            p.site_permissions_in(self.browsing)
                .get(&origin, Permission::PictureInPicture)
        });
        Some(setting == Some(Setting::Allow))
    }

    /// The toolbar's picture-in-picture button, for the selected tab.
    pub(super) fn show_pip_button(&self) {
        let active = self.active_tab();
        let video_here = active
            .as_ref()
            .is_some_and(|t| self.media.tab.get() == Some(t.id))
            && self.media.playback.borrow().as_ref().is_some_and(|p| p.video);
        let allowed = active
            .as_ref()
            .filter(|_| video_here)
            .and_then(|t| self.pip_allowed(t));
        let button = pip_button(self.media.switches.get(), video_here, allowed);
        let _ = xaml::set_visible(&self.ui.pip, button.is_some());
        if let Some(on) = button {
            let _ = self.ui.pip.SetIsChecked(Some(on));
            let tip = if on {
                "Turn off picture-in-picture for this site"
            } else {
                "Turn on picture-in-picture for this site"
            };
            let _ = xaml::set_tip(&self.ui.pip, tip);
        }
    }

    /// The toolbar's picture-in-picture button: turns picture-in-picture on for the selected
    /// tab's site and puts its video in the box, or turns it off there and ends it.
    pub(crate) fn pip_clicked(&self) {
        let (Some(tab), Some(browser)) = (self.active_tab(), self.browser()) else {
            return;
        };
        let (Some(origin), Some(allowed)) = (tab.origin(), self.pip_allowed(&tab)) else {
            return;
        };
        let (setting, start) = pip_click(allowed);
        let stored = browser.core(|p| {
            p.site_permissions_in(self.browsing)
                .set(&origin, Permission::PictureInPicture, Some(setting))
        });
        if let Err(e) = stored {
            log::warn!("picture-in-picture for {}: {e}", origin.as_str());
        }
        let followed = self.media.tab.get() == Some(tab.id);
        self.media.requested.set((start && followed).then_some(tab.id));
        // Every window shows the site's new setting, this one too.
        permissions::settings_changed(&browser);
    }

    /// Another tab was selected: a video the button put in the box stays there only while its
    /// tab is off screen.
    pub(super) fn media_selection_moved(&self) {
        self.media.requested.set(None);
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
        if !self.media.switches.get().pip_shown() {
            return None;
        }
        let id = self.media.tab.get()?;
        let pane = StripKind::of(self.tabs_position.get()) == StripKind::Side
            && !self.side.is_compact()
            && !self.fullscreen.get();
        let active = self.active_tab().map(|t| t.id);
        let on_screen = (active == Some(id)
            || self
                .split
                .get()
                .is_some_and(|s| s.has(id) && active.is_some_and(|a| s.has(a))))
            && self.media.requested.get() != Some(id);
        let allowed = self
            .tab(id)
            .and_then(|tab| self.pip_allowed(&tab))
            .unwrap_or(false);
        let content = pip_content(self.media.playback.borrow().as_ref(), id, allowed);
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
        xaml::remove_child(&self.ui.pages, &view)?;
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
            xaml::remove_child(self.player.pip_host(), &view)?;
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
        if self.media.requested.get() == Some(id) {
            self.media.requested.set(None);
        }
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

    /// The player's title or its box: selects the followed tab, and brings home a video the
    /// toolbar's button put in the box while the tab was selected.
    pub(super) fn player_go_to_tab(&self) {
        if let Some(id) = self.media.tab.get() {
            if self.media.requested.get() == Some(id) {
                self.media.requested.set(None);
                self.update_pip();
            }
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
        xaml::is_visible(self.player.element())
    }

    /// The toolbar's picture-in-picture button while it shows: whether it is on.
    pub fn pip_button_shown(&self) -> Option<bool> {
        xaml::is_visible(&self.ui.pip)
            .then(|| self.ui.pip.IsChecked().ok())
            .flatten()
    }

    /// Whether the page grid shows where the selected tab's video went.
    pub fn pip_placeholder_shown(&self) -> bool {
        xaml::is_visible(&self.ui.pip_placeholder)
    }

    /// Clicks the picture-in-picture box, as the user would.
    pub fn click_pip_box(&self) {
        self.player_go_to_tab();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ART: &str = "https://a.test/art.png";
    const ON: Switches = Switches {
        player: true,
        pip: true,
    };

    #[test]
    fn the_button_shows_for_a_video_in_the_selected_tab_of_a_site() {
        assert_eq!(pip_button(ON, true, Some(false)), Some(false));
        assert_eq!(pip_button(ON, true, Some(true)), Some(true));
        assert_eq!(pip_button(ON, false, Some(true)), None, "no video here");
        assert_eq!(pip_button(ON, true, None), None, "no site to remember");
        let no_pip = Switches { pip: false, ..ON };
        assert_eq!(pip_button(no_pip, true, Some(true)), None);
        let no_player = Switches {
            player: false,
            ..ON
        };
        assert_eq!(pip_button(no_player, true, Some(true)), None);
    }

    #[test]
    fn a_click_turns_the_site_on_and_starts_or_turns_it_off() {
        assert_eq!(pip_click(false), (Setting::Allow, true));
        assert_eq!(pip_click(true), (Setting::Block, false));
    }

    #[test]
    fn only_a_site_that_allowed_it_shows_its_video() {
        let video = Playback {
            video: true,
            artwork: ART.into(),
            ..Playback::default()
        };
        assert_eq!(pip_content(Some(&video), 7, true), Some(Pip::Video(7)));
        assert_eq!(
            pip_content(Some(&video), 7, false),
            Some(Pip::Artwork(ART.into())),
            "the player's artwork, not the site's page"
        );
        let silent = Playback {
            video: true,
            ..Playback::default()
        };
        assert_eq!(pip_content(Some(&silent), 7, false), None);
        let sound = Playback {
            artwork: ART.into(),
            ..Playback::default()
        };
        assert_eq!(
            pip_content(Some(&sound), 7, true),
            Some(Pip::Artwork(ART.into()))
        );
        assert_eq!(pip_content(None, 7, true), None);
    }
}
