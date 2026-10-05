//! Settings' "See all site data": the sites with cookies in the profile, each with how many and
//! a Delete button, and Delete all. Each opening of the list reads it from the engine again.

use std::rc::{Rc, Weak};

use serde_json::json;
use vsesvit_core::cookies::{self as core_cookies, SiteCookies};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
use crate::browser::Browser;
use crate::tab::devtools_in;
use crate::{cookies, exec, xaml};

/// The list's parts.
struct List {
    rows: Panel,
    empty: UIElement,
    delete_all: Button,
    browser: Weak<Browser>,
}

pub(super) fn wire(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<()> {
    let list = Rc::new(List {
        rows: xaml::find(root, "SiteDataList")?,
        empty: xaml::find(root, "SiteDataEmpty")?,
        delete_all: xaml::find(root, "SiteDataDeleteAll")?,
        browser: Rc::downgrade(browser),
    });
    let opened = list.clone();
    xaml::find::<FlyoutBase>(root, "SiteDataFlyout")?
        .Opening(move |_, _| exec::spawn(fill(opened.clone())))?
        .forget();
    let all = list.clone();
    on_click(&list.delete_all, move || {
        exec::spawn(delete(all.clone(), None));
    })
}

/// Reads the profile's cookies and shows them by site.
async fn fill(list: Rc<List>) {
    let Some(core) = list.browser.upgrade().and_then(|b| cookies::any_core(&b)) else {
        return;
    };
    let sites = match cookies::all_cookies(&core).await {
        Ok(all) => core_cookies::group_by_site(all.iter().map(cookies::Cookie::site)),
        Err(e) => {
            log::warn!("site data: {e}");
            Vec::new()
        }
    };
    if let Err(e) = show(&list, &sites) {
        log::warn!("site data list: {e}");
    }
}

fn show(list: &Rc<List>, sites: &[SiteCookies]) -> Result<()> {
    let children = list.rows.Children()?;
    children.Clear()?;
    xaml::set_visible(&list.empty, sites.is_empty())?;
    list.delete_all
        .cast::<Control>()?
        .SetIsEnabled(!sites.is_empty())?;
    for (index, site) in sites.iter().enumerate() {
        let row: FrameworkElement = xaml::load(&format!(
            r#"<Grid {{ns}} ColumnSpacing="8">
                 <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
                 <TextBlock x:Name="SiteData{index}" Text="{}" VerticalAlignment="Center" TextTrimming="CharacterEllipsis"/>
                 <Button x:Name="SiteDataDelete{index}" Grid.Column="1" Content="Delete"/>
               </Grid>"#,
            xaml::escape(&row_label(site))
        ))?;
        children.Append(&row.cast::<UIElement>()?)?;
        let (l, name) = (list.clone(), site.site.clone());
        on_click(
            &xaml::find::<Button>(&row, &format!("SiteDataDelete{index}"))?,
            move || exec::spawn(delete(l.clone(), Some(name.clone()))),
        )?;
    }
    Ok(())
}

/// Deletes the cookies of `site` (`Cookie::site`), or every site's, with the storage of their
/// http and https origins, then shows what is left.
async fn delete(list: Rc<List>, site: Option<String>) {
    let Some(core) = list.browser.upgrade().and_then(|b| cookies::any_core(&b)) else {
        return;
    };
    let deleted = async {
        let all = cookies::all_cookies(&core).await?;
        let mut sites: Vec<&str> = all
            .iter()
            .map(cookies::Cookie::site)
            .filter(|s| site.as_deref().is_none_or(|picked| picked == *s))
            .collect();
        sites.sort_unstable();
        sites.dedup();
        for host in &sites {
            for scheme in ["http", "https"] {
                let params =
                    json!({ "origin": format!("{scheme}://{host}"), "storageTypes": "all" });
                devtools_in(&core, "", "Storage.clearDataForOrigin", &params.to_string()).await?;
            }
        }
        cookies::delete_cookies(&core, |s| sites.contains(&s)).await
    }
    .await;
    match deleted {
        Ok(n) => log::info!(
            "site data: deleted {n} cookie(s) of {}",
            site.as_deref().unwrap_or("every site")
        ),
        Err(e) => log::warn!("site data: {e}"),
    }
    fill(list).await;
}

/// A row of the list: "example.com · 3 cookies".
fn row_label(site: &SiteCookies) -> String {
    let unit = if site.count == 1 { "cookie" } else { "cookies" };
    format!("{} · {} {unit}", site.site, site.count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_count_their_cookies() {
        let row = |count| SiteCookies {
            site: "e.test".into(),
            count,
        };
        assert_eq!(row_label(&row(1)), "e.test · 1 cookie");
        assert_eq!(row_label(&row(3)), "e.test · 3 cookies");
    }
}
