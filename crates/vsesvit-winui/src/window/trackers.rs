//! Tracking protection in the site-info popup: the switch that turns it off for the site, and
//! the count under it, which follows the page while the popup is open.

use std::rc::Rc;

use vsesvit_core::prefs::keys;
use vsesvit_core::trackers::{self, TrackingProtection};
use windows_core::Result;

use super::BrowserWindow;
use crate::bindings::*;
use crate::tab::Tab;
use crate::xaml;

impl BrowserWindow {
    /// The switch for `tab`'s page: whether protection is on for its site, and how many
    /// trackers it blocked on the page. `None` on pages of no web site, and while protection is
    /// off everywhere.
    pub(super) fn tracking_status(&self, tab: &Tab) -> Option<(bool, usize)> {
        let origin = tab
            .origin()
            .filter(|o| o.as_str().starts_with("https://") || o.as_str().starts_with("http://"))?;
        let (level, allowed) = self.browser()?.core(|p| {
            (
                p.prefs().get(&keys::TRACKING_PROTECTION),
                trackers::allowed(p, &origin),
            )
        });
        (level != TrackingProtection::Off).then(|| (!allowed, tab.blocked_trackers().len()))
    }

    /// Wires the switch in site-info popup content `popup` for `tab`, when it has one.
    pub(super) fn wire_tracking_switch(
        &self,
        popup: &FrameworkElement,
        tab: &Rc<Tab>,
    ) -> Result<()> {
        let Ok(switch) = xaml::find::<ToggleSwitch>(popup, "TrackingProtectionSwitch") else {
            return Ok(());
        };
        let (source, w, t) = (switch.clone(), self.me.clone(), Rc::downgrade(tab));
        switch
            .Toggled(move |_, _| {
                let (Some(w), Some(tab), Ok(on)) = (w.upgrade(), t.upgrade(), source.IsOn()) else {
                    return;
                };
                if w.tracking_status(&tab)
                    .is_some_and(|(stored, _)| stored != on)
                {
                    tab.set_tracking_protection(on);
                }
            })?
            .forget();
        Ok(())
    }

    /// The open site-info popup's count of blocked trackers, as the selected tab's page loads.
    pub(super) fn show_tracking_status(&self) {
        let (Some(popup), Some(tab)) = (self.connection_popup(), self.active_tab()) else {
            return;
        };
        let (Some((on, blocked)), Ok(status)) = (
            self.tracking_status(&tab),
            xaml::find::<TextBlock>(&popup, "TrackingProtectionStatus"),
        ) else {
            return;
        };
        let text = trackers::site_status(on, Some(blocked));
        if status.Text().is_ok_and(|shown| shown != text) {
            let _ = status.SetText(&text);
        }
    }
}
