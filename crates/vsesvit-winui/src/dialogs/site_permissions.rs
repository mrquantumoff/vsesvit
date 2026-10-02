//! Settings' Site permissions page: every stored site setting, grouped by site, each with its
//! Allow / Block choice and a button that removes it (back to Ask).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use vsesvit_core::permissions::{Origin, Permission, Setting, SiteGroup};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
use crate::browser::Browser;
use crate::{exec, permissions, xaml};

/// The list's parts, refilled after a removal.
struct Page {
    list: Panel,
    empty: UIElement,
    browser: Weak<Browser>,
}

pub(super) fn wire(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<()> {
    let page = Rc::new(Page {
        list: xaml::find(root, "SitePermissionsList")?,
        empty: xaml::find(root, "SitePermissionsEmpty")?,
        browser: Rc::downgrade(browser),
    });
    fill(&page)
}

/// The setting a combo box last stored. A combo box raises SelectionChanged for its initial
/// selection too, so only a pick that differs from this is the user's.
struct Shown(Cell<Setting>);

impl Shown {
    /// `picked`, when it differs from the setting stored.
    fn change(&self, picked: Setting) -> Option<Setting> {
        (picked != self.0.get()).then_some(picked)
    }

    fn applied(&self, setting: Setting) {
        self.0.set(setting);
    }
}

fn site_markup(index: usize, site: &SiteGroup) -> String {
    let rows: String = site
        .settings
        .iter()
        .map(|&(permission, setting)| {
            let options = permission.settings();
            let selected = options.iter().position(|s| *s == setting).unwrap_or(0);
            let items: String = options
                .iter()
                .map(|s| format!(r#"<ComboBoxItem Content="{}"/>"#, s.label()))
                .collect();
            format!(
                r#"<Grid ColumnSpacing="12">
                     <Grid.ColumnDefinitions>
                       <ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/>
                       <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
                     </Grid.ColumnDefinitions>
                     <FontIcon Glyph="{glyph}" FontSize="16" VerticalAlignment="Center"/>
                     <TextBlock Grid.Column="1" Text="{label}" VerticalAlignment="Center"/>
                     <ComboBox x:Name="SiteChoice{index}{key}" Grid.Column="2" Width="120" SelectedIndex="{selected}"
                               AutomationProperties.Name="{label}">{items}</ComboBox>
                     <Button x:Name="SiteRemove{index}{key}" Grid.Column="3" Width="36" Height="32" Padding="0"
                             ToolTipService.ToolTip="Remove" AutomationProperties.Name="Remove">
                       <FontIcon Glyph="&#xE711;" FontSize="12"/>
                     </Button>
                   </Grid>"#,
                glyph = permissions::glyph(permission),
                label = permission.label(),
                key = permission.key(),
            )
        })
        .collect();
    format!(
        r#"<StackPanel {{ns}} Spacing="8">
             <TextBlock x:Name="Site{index}" Text="{}" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
             {rows}
           </StackPanel>"#,
        xaml::escape(&site.heading)
    )
}

fn fill(page: &Rc<Page>) -> Result<()> {
    let Some(browser) = page.browser.upgrade() else {
        return Ok(());
    };
    let sites = browser.core(|p| p.site_permissions().by_site());
    let children = page.list.Children()?;
    children.Clear()?;
    xaml::set_visible(&page.empty, sites.is_empty())?;
    for (index, group) in sites.into_iter().enumerate() {
        let site: FrameworkElement = xaml::load(&site_markup(index, &group))?;
        children.Append(&site.cast::<UIElement>()?)?;
        let origin = group.origin;
        for (permission, setting) in group.settings {
            let key = permission.key();
            let options = permission.settings();
            let selector = xaml::find::<ComboBox>(&site, &format!("SiteChoice{index}{key}"))?
                .cast::<Selector>()?;
            let (source, b, o) = (selector.clone(), page.browser.clone(), origin.clone());
            let shown = Shown(Cell::new(setting));
            selector
                .SelectionChanged(move |_, _| {
                    let picked =
                        super::selected_index(&source).and_then(|i| options.get(i).copied());
                    if let (Some(picked), Some(b)) =
                        (picked.and_then(|p| shown.change(p)), b.upgrade())
                        && set(&b, &o, permission, Some(picked))
                    {
                        shown.applied(picked);
                    }
                })?
                .forget();
            let (p, o) = (page.clone(), origin.clone());
            on_click(
                &xaml::find::<Button>(&site, &format!("SiteRemove{index}{key}"))?,
                move || {
                    if let Some(b) = p.browser.upgrade() {
                        set(&b, &o, permission, None);
                    }
                    let p = p.clone();
                    // Not from inside the click of a button the refill removes.
                    exec::spawn(async move {
                        if let Err(e) = fill(&p) {
                            log::warn!("site permissions page: {e}");
                        }
                    });
                },
            )?;
        }
    }
    Ok(())
}

/// Stores `setting`, saying whether it could; a block, or a removal, ends what the site captures
/// under it, reloading the tabs the shell let capture (see `permissions::must_reload`).
fn set(
    browser: &Rc<Browser>,
    origin: &Origin,
    permission: Permission,
    setting: Option<Setting>,
) -> bool {
    let before = browser.core(|p| {
        let mut site = p.site_permissions();
        let before = site.get(origin, permission);
        site.set(origin, permission, setting).map(|()| before)
    });
    let before = match before {
        Ok(before) => before,
        Err(e) => {
            log::warn!(
                "site permission {permission:?} for {}: {e}",
                origin.as_str()
            );
            return false;
        }
    };
    if setting.is_none() {
        permissions::stop_captures(browser, origin, &[permission]);
    }
    permissions::settings_changed(browser);
    permissions::reload_taken_back(browser, origin, permission, before, setting);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_back_to_the_filled_setting_is_stored() {
        let shown = Shown(Cell::new(Setting::Allow));
        // The initial selection.
        assert_eq!(shown.change(Setting::Allow), None);
        assert_eq!(shown.change(Setting::Block), Some(Setting::Block));
        shown.applied(Setting::Block);
        assert_eq!(shown.change(Setting::Block), None);
        assert_eq!(shown.change(Setting::Allow), Some(Setting::Allow));
    }
}
