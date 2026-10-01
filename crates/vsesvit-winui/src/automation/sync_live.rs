//! A live sync between two profiles through a real sync server, run as `--ui-smoke` when
//! `VSESVIT_SYNC_LIVE` names the server. `VSESVIT_SYNC_LIVE_ROLE` is `a`, `b` or `a-delete`, and
//! the two runs meet through marker files in `VSESVIT_SYNC_LIVE_DIR`. A marker counts only for
//! runs with the same `VSESVIT_SYNC_LIVE_RUN`, so markers an earlier pair left there are not
//! taken for this pair's; once `VSESVIT_SYNC_LIVE` is set, the run fails if any of the others
//! is missing or empty. Both sign in by pressing Sign In on the Sync page, which opens the
//! provider's page in a tab, as a user does.
//!
//! A adds bookmarks and opens tabs; B sees them arrive, lists A's tabs under "Tabs from other
//! devices", turns Bookmarks off and on again, and gets the bookmark A added just before it
//! quit. `a-delete`, on A's profile again, deletes the data on the server.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vsesvit_core::Url;
use vsesvit_core::prefs::keys;
use vsesvit_core::testkit::FixtureServer;
use vsesvit_sync::status::State;
use windows_core::Interface;

use super::{confirm_flyout, invoke, settings_on, shoot, wait_loaded};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::{self, Dialog};
use crate::exec;
use crate::window::BrowserWindow;

const POLL: Duration = Duration::from_millis(250);
const SETTLE: Duration = Duration::from_millis(600);
/// Long enough for a sync on the 60 s timer, and some.
const ARRIVAL: Duration = Duration::from_secs(100);
const PARTNER: Duration = Duration::from_secs(300);

pub(super) struct Live {
    server: String,
    role: String,
    dir: PathBuf,
    run: String,
}

impl Live {
    /// `None` when `VSESVIT_SYNC_LIVE` is unset; once it is set, a missing or empty variable is
    /// the run's error, so a driver that leaves one out fails rather than passing a fixture run.
    /// `None` when `VSESVIT_SYNC_LIVE` is unset, and an error when another variable is missing,
    /// so a driver that leaves one out fails rather than passing as a fixture run.
    pub fn from_env() -> Option<std::result::Result<Live, String>> {
        Live::from_vars(|name| std::env::var_os(name))
    }

    fn from_vars(
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Option<std::result::Result<Live, String>> {
        var("VSESVIT_SYNC_LIVE")?;
        let need = |name: &str| {
            var(name)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{name} is not set"))
        };
        let text = |name: &str| {
            need(name)?
                .into_string()
                .map_err(|_| format!("{name} is not Unicode"))
        };
        Some((|| {
            Ok(Live {
                server: text("VSESVIT_SYNC_LIVE")?,
                role: text("VSESVIT_SYNC_LIVE_ROLE")?,
                dir: need("VSESVIT_SYNC_LIVE_DIR")?.into(),
                run: text("VSESVIT_SYNC_LIVE_RUN")?,
            })
        })())
    }

    fn mark(&self, name: &str) -> i64 {
        let at = now_ms();
        let _ = std::fs::write(self.dir.join(name), format!("{} {at}", self.run));
        log::info!("live sync: marked {name}");
        at
    }

    async fn marked(&self, name: &str) -> std::result::Result<i64, String> {
        let file = self.dir.join(name);
        exec::wait_for(PARTNER, POLL, || {
            marker_time(&std::fs::read_to_string(&file).ok()?, &self.run)
        })
        .await
        .ok_or_else(|| format!("the other run never marked {name}"))
    }
}

/// When a marker file says it was marked, if `run` marked it.
fn marker_time(contents: &str, run: &str) -> Option<i64> {
    let (id, at) = contents.trim().rsplit_once(' ')?;
    if id == run { at.parse().ok() } else { None }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn has_bookmark(browser: &Browser, url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| browser.core(|p| p.bookmarks().is_bookmarked(&url)))
}

/// Seconds since the epoch at which the last sync completed, while signed in and not syncing.
fn synced_at(browser: &Browser) -> Option<u64> {
    match browser.sync().state() {
        State::SignedIn {
            last_synced,
            syncing: false,
            ..
        } => last_synced,
        _ => None,
    }
}

/// When the last sync that completed started, as seen from the sync state's changes. A change
/// is uploaded by a sync that started after it; `last_synced`, in whole seconds, cannot tell.
#[derive(Default)]
struct Syncs {
    started: Option<Instant>,
    completed: Option<Instant>,
}

impl Syncs {
    fn saw(&mut self, state: &State, now: Instant) {
        match state {
            State::SignedIn { syncing: true, .. } => {
                self.started.get_or_insert(now);
            }
            State::SignedIn {
                error: None,
                needs_sign_in: false,
                ..
            } => self.completed = self.started.take().or(self.completed),
            _ => self.started = None,
        }
    }

    fn completed_since(&self, since: Instant) -> bool {
        self.completed.is_some_and(|started| started >= since)
    }
}

/// `Syncs` for a browser, kept up to date while this is kept.
struct Follow {
    syncs: Rc<RefCell<Syncs>>,
    _listener: Rc<dyn Fn()>,
}

impl Follow {
    fn new(browser: &Rc<Browser>) -> Follow {
        let syncs = Rc::new(RefCell::new(Syncs::default()));
        let (browser_ref, seen) = (Rc::downgrade(browser), syncs.clone());
        let listener: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(browser) = browser_ref.upgrade() {
                seen.borrow_mut()
                    .saw(&browser.sync().state(), Instant::now());
            }
        });
        browser.sync().on_change(&listener);
        Follow {
            syncs,
            _listener: listener,
        }
    }
}

/// Waits for a sync that started at or after `since` to complete.
async fn synced_since(
    browser: &Browser,
    follow: &Follow,
    since: Instant,
    limit: Duration,
) -> Option<u64> {
    exec::wait_for(limit, POLL, || {
        let completed = follow.syncs.borrow().completed_since(since);
        completed.then(|| synced_at(browser)).flatten()
    })
    .await
}

/// Types the server into the Sync page and presses Sign In; the provider's page opens in a tab
/// and sends the browser back to the sign-in's loopback address.
async fn sign_in(
    live: &Live,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> std::result::Result<(), String> {
    let preview = settings_on(window, "SyncPanel")
        .await
        .map_err(|e| e.to_string())?;
    let server: TextBox = preview.find("SyncServer").map_err(|e| e.to_string())?;
    server.SetText(&live.server).map_err(|e| e.to_string())?;
    let started = Instant::now();
    invoke(
        &preview
            .find::<Button>("SyncSignIn")
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let signed_in = exec::wait_for(Duration::from_secs(60), POLL, || synced_at(browser)).await;
    exec::sleep(SETTLE).await;
    let status = browser.sync().status();
    let name = format!("live-{}-1-sync-page-signed-in", live.role);
    shoot(window, out_dir, &name, steps, |_| {
        json!({
            "server": live.server,
            "stored_server": browser.core(|p| p.prefs().get(&keys::SYNC_SERVER)),
            "title": status.title,
            "subtitle": status.subtitle,
            "signed_in_and_synced_after_s": started.elapsed().as_secs_f32(),
            "ok": signed_in.is_some(),
        })
    })
    .await;
    drop(preview);
    exec::sleep(SETTLE).await;
    let provider_tab = window.active_tab().map(|t| t.state());
    let name = format!("live-{}-2-provider-page", live.role);
    shoot(window, out_dir, &name, steps, |_| {
        json!({
            "tab": provider_tab.as_ref().map(|s| json!({ "url": s.url, "title": s.title })),
            "ok": provider_tab.is_some_and(|s| s.url.contains("/callback")),
        })
    })
    .await;
    signed_in
        .map(drop)
        .ok_or_else(|| "did not sign in within 60 s".to_owned())
}

/// Adds a bookmark the way the star does, and waits for this device's next sync to take it:
/// how long that took after the change is what "sync soon after a change" promises.
async fn bookmark_and_upload(
    live: &Live,
    browser: &Browser,
    follow: &Follow,
    url: &str,
    marker: &str,
    steps: &mut Vec<Value>,
) -> std::result::Result<i64, String> {
    let since = Instant::now();
    browser.bookmark_page(url, url);
    let at = live.mark(marker);
    let uploaded = synced_since(browser, follow, since, Duration::from_secs(40)).await;
    let after_ms = now_ms() - at;
    steps.push(json!({
        "name": format!("live-{}-{marker}-uploaded", live.role),
        "bookmark": url,
        "uploaded_after_ms": after_ms,
        "ok": uploaded.is_some() && after_ms <= 16_000,
    }));
    live.mark(&format!("{marker}-uploaded"));
    Ok(at)
}

pub(super) async fn run(
    live: &Live,
    browser: &Rc<Browser>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> std::result::Result<(), String> {
    let window = browser.windows().into_iter().next().ok_or("no window")?;
    wait_loaded(&window.active_tab().ok_or("no tab")?).await?;
    let device = match live.role.as_str() {
        "b" => "Live B",
        _ => "Live A",
    };
    browser.write_pref(&keys::DEVICE_NAME, &device.to_owned());
    let follow = Follow::new(browser);
    match live.role.as_str() {
        "a" => role_a(live, browser, &follow, &window, out_dir, steps).await,
        "b" => role_b(live, browser, &follow, &window, out_dir, steps).await,
        "a-delete" => role_delete(live, browser, &window, out_dir, steps).await,
        other => Err(format!("unknown role {other}")),
    }
}

async fn role_a(
    live: &Live,
    browser: &Rc<Browser>,
    follow: &Follow,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> std::result::Result<(), String> {
    let fixture = FixtureServer::start().map_err(|e| e.to_string())?;
    sign_in(live, browser, window, out_dir, steps).await?;
    for page in ["/index.html", "/page2.html"] {
        let tab = window
            .open_url_tab(fixture.url(page).as_str(), true)
            .map_err(|e| e.to_string())?;
        wait_loaded(&tab).await?;
    }
    browser.save_session_now();
    bookmark_and_upload(
        live,
        browser,
        follow,
        "https://live-one.example/",
        "a-bookmark1",
        steps,
    )
    .await?;

    live.marked("b-bookmarks-off").await?;
    bookmark_and_upload(
        live,
        browser,
        follow,
        "https://live-two.example/",
        "a-bookmark2",
        steps,
    )
    .await?;

    live.marked("b-bookmarks-on").await?;
    // Quits with no await in between, so no scheduled sync can take it first: only the final
    // sync on the way out uploads it.
    browser.bookmark_page("https://live-three.example/", "live three");
    live.mark("a-bookmark3");
    steps.push(json!({ "name": "live-a-quits-right-after-a-change", "ok": true }));
    Ok(())
}

async fn role_b(
    live: &Live,
    browser: &Rc<Browser>,
    follow: &Follow,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> std::result::Result<(), String> {
    sign_in(live, browser, window, out_dir, steps).await?;

    let changed = live.marked("a-bookmark1").await?;
    live.marked("a-bookmark1-uploaded").await?;
    let arrived = exec::wait_for(ARRIVAL, POLL, || {
        has_bookmark(browser, "https://live-one.example/").then_some(())
    })
    .await;
    steps.push(json!({
        "name": "live-b-bookmark1-arrives",
        "after_a_changed_ms": now_ms() - changed,
        "ok": arrived.is_some(),
    }));

    let preview = dialogs::preview(window, Dialog::History).map_err(|e| e.to_string())?;
    exec::sleep(SETTLE).await;
    let sections: ListView = preview.find("HistorySections").map_err(|e| e.to_string())?;
    sections
        .cast::<Selector>()
        .and_then(|s| s.SetSelectedIndex(1))
        .map_err(|e| e.to_string())?;
    exec::sleep(SETTLE).await;
    let listed = preview.find::<UIElement>("Device0").is_ok();
    let tabs = browser
        .core(|p| p.session().other_devices())
        .map_err(|e| e.to_string())?;
    let a = tabs.iter().find(|d| d.device_name == "Live A");
    let a_tabs: Vec<String> = a
        .map(|d| {
            d.windows
                .iter()
                .flat_map(|w| &w.tabs)
                .map(|t| t.url.to_string())
                .collect()
        })
        .unwrap_or_default();
    shoot(
        window,
        out_dir,
        "live-b-3-tabs-from-other-devices",
        steps,
        |_| {
            json!({
                "devices": tabs.iter().map(|d| &d.device_name).collect::<Vec<_>>(),
                "a_tabs": a_tabs,
                "shown": listed,
                "ok": listed
                    && ["/index.html", "/page2.html"]
                        .iter()
                        .all(|page| a_tabs.iter().any(|u| u.ends_with(page))),
            })
        },
    )
    .await;
    drop(preview);

    let preview = settings_on(window, "SyncPanel")
        .await
        .map_err(|e| e.to_string())?;
    let everything: ToggleSwitch = preview.find("SyncEverything").map_err(|e| e.to_string())?;
    everything.SetIsOn(false).map_err(|e| e.to_string())?;
    exec::sleep(SETTLE).await;
    let bookmarks: ToggleSwitch = preview
        .find("SyncTypeBookmarks")
        .map_err(|e| e.to_string())?;
    bookmarks.SetIsOn(false).map_err(|e| e.to_string())?;
    exec::sleep(SETTLE).await;
    let types = browser.core(|p| p.prefs().get(&keys::SYNC_TYPES));
    shoot(window, out_dir, "live-b-4-customize-sync", steps, |_| {
        json!({
            "types": format!("{types:?}"),
            "ok": !types.contains(&vsesvit_core::sync::DataType::Bookmarks) && types.len() == 4,
        })
    })
    .await;
    live.mark("b-bookmarks-off");

    live.marked("a-bookmark2-uploaded").await?;
    let synced = synced_since(browser, follow, Instant::now(), ARRIVAL).await;
    let absent = !has_bookmark(browser, "https://live-two.example/");
    steps.push(json!({
        "name": "live-b-bookmark2-not-synced-while-off",
        "b_synced_at": synced,
        "absent": absent,
        "ok": synced.is_some() && absent,
    }));

    bookmarks.SetIsOn(true).map_err(|e| e.to_string())?;
    let on = now_ms();
    let arrived = exec::wait_for(Duration::from_secs(30), POLL, || {
        has_bookmark(browser, "https://live-two.example/").then_some(())
    })
    .await;
    exec::sleep(SETTLE).await;
    shoot(
        window,
        out_dir,
        "live-b-5-bookmarks-on-again",
        steps,
        |_| {
            json!({
                "arrived_after_ms": now_ms() - on,
                "ok": arrived.is_some(),
            })
        },
    )
    .await;
    drop(preview);
    live.mark("b-bookmarks-on");

    let quit = live.marked("a-bookmark3").await?;
    exec::sleep(Duration::from_secs(4)).await;
    let arrived = exec::wait_for(ARRIVAL, POLL, || {
        has_bookmark(browser, "https://live-three.example/").then_some(())
    })
    .await;
    steps.push(json!({
        "name": "live-b-gets-what-a-sent-as-it-quit",
        "after_a_quit_ms": now_ms() - quit,
        "ok": arrived.is_some(),
    }));
    live.mark("b-done");
    Ok(())
}

async fn role_delete(
    live: &Live,
    browser: &Rc<Browser>,
    window: &Rc<BrowserWindow>,
    out_dir: &Path,
    steps: &mut Vec<Value>,
) -> std::result::Result<(), String> {
    let signed_in = browser.sync().signed_in();
    let preview = settings_on(window, "SyncPanel")
        .await
        .map_err(|e| e.to_string())?;
    let button: Button = preview
        .find("SyncDeleteServerData")
        .map_err(|e| e.to_string())?;
    invoke(&button).map_err(|e| e.to_string())?;
    exec::sleep(SETTLE).await;
    shoot(
        window,
        out_dir,
        "live-a-delete-1-confirm",
        steps,
        |_| json!({ "signed_in_at_start": signed_in, "ok": signed_in }),
    )
    .await;
    confirm_flyout(&button, "SyncDeleteConfirm").map_err(|e| e.to_string())?;
    let signed_out = exec::wait_for(Duration::from_secs(30), POLL, || {
        (!browser.sync().signed_in()).then_some(())
    })
    .await;
    exec::sleep(SETTLE).await;
    let status = browser.sync().status();
    shoot(window, out_dir, "live-a-delete-2-signed-out", steps, |_| {
        json!({
            "title": status.title,
            "account_kept": browser.core(|p| vsesvit_sync::Account::load(&mut p.sync()).ok().flatten().is_some()),
            "ok": signed_out.is_some(),
        })
    })
    .await;
    drop(preview);
    live.mark("a-deleted");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_in(syncing: bool, error: Option<&str>) -> State {
        State::SignedIn {
            name: None,
            server: "https://sync.test".into(),
            last_synced: Some(1000),
            syncing,
            error: error.map(str::to_owned),
            needs_sign_in: false,
        }
    }

    #[test]
    fn markers_count_only_for_their_run() {
        assert_eq!(
            marker_time("run1 1700000000000\n", "run1"),
            Some(1_700_000_000_000)
        );
        assert_eq!(marker_time("run0 1700000000000", "run1"), None);
        assert_eq!(marker_time("1700000000000", "run1"), None);
        assert_eq!(marker_time("run 1 5", "run 1"), Some(5));
    }

    /// `Live::from_vars` with every variable set to `x`, but `without` unset and `run` for RUN.
    fn live(without: &str, run: &str) -> Option<std::result::Result<Live, String>> {
        Live::from_vars(|name| match name {
            _ if name == without => None,
            "VSESVIT_SYNC_LIVE_RUN" => Some(run.into()),
            _ => Some("x".into()),
        })
    }

    #[test]
    fn a_live_run_missing_a_variable_is_an_error_not_a_fixture_run() {
        assert!(live("VSESVIT_SYNC_LIVE", "run1").is_none());
        assert!(matches!(live("", "run1"), Some(Ok(live)) if live.run == "run1"));
        for missing in [
            "VSESVIT_SYNC_LIVE_ROLE",
            "VSESVIT_SYNC_LIVE_DIR",
            "VSESVIT_SYNC_LIVE_RUN",
        ] {
            let error = live(missing, "run1").and_then(Result::err);
            assert_eq!(error, Some(format!("{missing} is not set")));
        }
        let error = live("", "").and_then(Result::err);
        assert_eq!(error.as_deref(), Some("VSESVIT_SYNC_LIVE_RUN is not set"));
    }

    #[test]
    fn a_sync_that_started_with_the_change_counts_however_soon_it_ends() {
        let since = Instant::now();
        let mut syncs = Syncs::default();
        syncs.saw(&signed_in(true, None), since);
        assert!(!syncs.completed_since(since), "still running");
        syncs.saw(&signed_in(false, None), since + Duration::from_millis(300));
        assert!(syncs.completed_since(since));
    }

    #[test]
    fn a_sync_running_before_the_change_does_not_count() {
        let start = Instant::now();
        let since = start + Duration::from_millis(500);
        let mut syncs = Syncs::default();
        syncs.saw(&signed_in(true, None), start);
        syncs.saw(&signed_in(false, None), since + Duration::from_secs(1));
        assert!(!syncs.completed_since(since));
    }

    #[test]
    fn a_failed_sync_does_not_count() {
        let since = Instant::now();
        let mut syncs = Syncs::default();
        syncs.saw(&signed_in(true, None), since);
        syncs.saw(&signed_in(false, Some("offline")), since);
        assert!(!syncs.completed_since(since));
        syncs.saw(&signed_in(false, None), since);
        assert!(!syncs.completed_since(since), "no sync ran since");
    }
}
