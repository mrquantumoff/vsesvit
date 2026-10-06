//! Cookies in the site-info popup: the site's rule (Default, Allow, Block or Clear on exit) and
//! what it means on the page.

use std::cell::Cell;
use std::rc::Rc;

use vsesvit_core::cookies;
use vsesvit_core::permissions::Setting;
use vsesvit_core::private::Browsing;
use windows_core::{Interface, Result};

use super::BrowserWindow;
use crate::bindings::*;
use crate::tab::Tab;
use crate::{permissions, xaml};

impl BrowserWindow {
    /// The cookie rule of `tab`'s site, and whether third-party cookies are blocked on its page.
    /// `None` on pages of no web site.
    pub(super) fn cookies_status(&self, tab: &Tab) -> Option<(Option<Setting>, bool)> {
        let origin = tab
            .origin()
            .filter(|o| o.as_str().starts_with("https://") || o.as_str().starts_with("http://"))?;
        self.browser()?.core(|p| {
            Some((
                cookies::setting(p, &origin),
                cookies::third_party_blocked(p, Browsing::Normal, Some(&origin)),
            ))
        })
    }

    /// Wires the choice in site-info popup content `popup` for `tab`, when it has one. A new
    /// rule applies in every tab, and the page loads again under it.
    pub(super) fn wire_cookies_choice(
        &self,
        popup: &FrameworkElement,
        tab: &Rc<Tab>,
    ) -> Result<()> {
        let (Ok(choice), Some((current, _))) = (
            xaml::find::<ComboBox>(popup, "CookiesChoice"),
            self.cookies_status(tab),
        ) else {
            return Ok(());
        };
        let status: TextBlock = xaml::find(popup, "CookiesStatus")?;
        let choices = cookies::site_choices(current, true);
        let selector = choice.cast::<Selector>()?;
        let (source, w, t) = (selector.clone(), self.me.clone(), Rc::downgrade(tab));
        let shown = Cell::new(current);
        selector
            .SelectionChanged(move |_, _| {
                let picked =
                    crate::dialogs::selected_index(&source).and_then(|i| choices.get(i).copied());
                let (Some(picked), Some(w), Some(tab)) = (picked, w.upgrade(), t.upgrade()) else {
                    return;
                };
                // A combo box raises this for its initial selection too.
                if picked != shown.get() && w.set_site_cookies(&tab, picked) {
                    shown.set(picked);
                    if let Some((setting, blocked)) = w.cookies_status(&tab) {
                        let _ = status.SetText(cookies::site_status(blocked, setting));
                    }
                }
            })?
            .forget();
        Ok(())
    }

    /// Stores `setting` as the cookie rule of `tab`'s site, saying whether it could.
    fn set_site_cookies(&self, tab: &Tab, setting: Option<Setting>) -> bool {
        let (Some(browser), Some(origin)) = (self.browser(), tab.origin()) else {
            return false;
        };
        if let Err(e) = browser.core(|p| cookies::set(p, &origin, setting)) {
            log::warn!("tab {}: cookies for {}: {e}", tab.id, origin.as_str());
            return false;
        }
        log::info!(
            "tab {}: cookies for {} set to {}",
            tab.id,
            origin.as_str(),
            cookies::choice_label(setting)
        );
        permissions::settings_changed(&browser);
        tab.reload();
        true
    }
}
