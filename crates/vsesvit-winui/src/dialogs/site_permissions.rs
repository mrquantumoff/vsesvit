//! Settings' Site permissions page: every stored site setting, grouped by site, each with its
//! Allow / Block choice and a button that removes it (back to Ask).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use vsesvit_core::permissions::{Origin, Permission, Setting, SiteSetting};
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

/// The settings by origin, in `SitePermissions::all`'s order.
fn by_site(settings: Vec<SiteSetting>) -> Vec<(Origin, Vec<(Permission, Setting)>)> {
    let mut sites: Vec<(Origin, Vec<(Permission, Setting)>)> = Vec::new();
    for s in settings {
        match sites.last_mut() {
            Some((origin, list)) if *origin == s.origin => list.push((s.permission, s.setting)),
            _ => sites.push((s.origin, vec![(s.permission, s.setting)])),
        }
    }
    sites
}

/// The choices a stored setting offers: Allow only where it can be remembered.
fn choices(permission: Permission) -> Vec<Setting> {
    if permission.remembers_allow() {
        vec![Setting::Allow, Setting::Block]
    } else {
        vec![Setting::Block]
    }
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

fn label(setting: Setting) -> &'static str {
    match setting {
        Setting::Allow => "Allow",
        Setting::Block => "Block",
    }
}

fn site_markup(index: usize, origin: &Origin, settings: &[(Permission, Setting)]) -> String {
    let rows: String = settings
        .iter()
        .map(|&(permission, setting)| {
            let options = choices(permission);
            let selected = options.iter().position(|s| *s == setting).unwrap_or(0);
            let items: String = options
                .iter()
                .map(|s| format!(r#"<ComboBoxItem Content="{}"/>"#, label(*s)))
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
        xaml::escape(&origin.host_for_display())
    )
}

fn fill(page: &Rc<Page>) -> Result<()> {
    let Some(browser) = page.browser.upgrade() else {
        return Ok(());
    };
    let sites = by_site(browser.core(|p| p.site_permissions().all()));
    let children = page.list.Children()?;
    children.Clear()?;
    xaml::set_visible(&page.empty, sites.is_empty())?;
    for (index, (origin, settings)) in sites.into_iter().enumerate() {
        let site: FrameworkElement = xaml::load(&site_markup(index, &origin, &settings))?;
        children.Append(&site.cast::<UIElement>()?)?;
        for (permission, setting) in settings {
            let key = permission.key();
            let options = choices(permission);
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
/// under it.
fn set(
    browser: &Rc<Browser>,
    origin: &Origin,
    permission: Permission,
    setting: Option<Setting>,
) -> bool {
    if let Err(e) = browser.core(|p| p.site_permissions().set(origin, permission, setting)) {
        log::warn!(
            "site permission {permission:?} for {}: {e}",
            origin.as_str()
        );
        return false;
    }
    if setting.is_none() {
        permissions::stop_captures(browser, origin, &[permission]);
    }
    permissions::settings_changed(browser);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_group_by_site_in_order() {
        let a = Origin::parse("https://a.test").unwrap();
        let b = Origin::parse("https://b.test").unwrap();
        let setting = |origin: &Origin, permission, setting| SiteSetting {
            origin: origin.clone(),
            permission,
            setting,
        };
        let sites = by_site(vec![
            setting(&a, Permission::Camera, Setting::Allow),
            setting(&a, Permission::ScreenShare, Setting::Block),
            setting(&b, Permission::Location, Setting::Block),
        ]);
        assert_eq!(
            sites,
            [
                (
                    a,
                    vec![
                        (Permission::Camera, Setting::Allow),
                        (Permission::ScreenShare, Setting::Block)
                    ]
                ),
                (b, vec![(Permission::Location, Setting::Block)]),
            ]
        );
        assert_eq!(choices(Permission::ScreenShare), [Setting::Block]);
    }

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
