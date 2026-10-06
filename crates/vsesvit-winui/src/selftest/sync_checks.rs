//! The `sync_passphrase` check: a profile signed in to an account with no sync passphrase syncs
//! unencrypted, and the offer to encrypt it appears by itself once; the passphrase is set through
//! the flyout on Settings' Sync page, which takes only a long enough passphrase typed the same
//! twice; and a device without the account's keys is asked for its passphrase by a prompt that
//! closes no other way than entering it or signing out.

use std::rc::Rc;
use std::time::Duration;

use serde_json::json;
use vsesvit_core::prefs::keys;
use vsesvit_sync::status::{OFFER, State};
use vsesvit_sync::{Account, Encryption};

use super::{Probe, until};
use crate::automation::{invoke, settings_on};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::SyncPage;
use crate::dialogs::sync_prompt::{self, Asks, PromptDialog};
use crate::window::BrowserWindow;
use crate::{exec, sync};

const SETTLE: Duration = Duration::from_millis(300);
const PASSPHRASE: &str = "correct horse battery";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// An account on a server that refuses at once, which has looked through the account and found
/// no key record.
fn account_without_passphrase() -> serde_json::Result<Account> {
    serde_json::from_value(json!({
        "sign_in": "self-test", "server": "http://127.0.0.1:9", "name": "Self-test",
        "limits": { "max_batch": 100, "max_record_bytes": 1_048_576, "max_request_bytes": 4_194_304 },
        "download_cursor": 0, "upload_cursors": {}, "last_synced": null,
        "server_keys": "missing", "plaintext_trusted": false,
    }))
}

/// The profile signed in to [`account_without_passphrase`] until dropped, then signed out with
/// the offer's preference as it was, however the run using it ended.
pub(crate) struct WithoutPassphrase {
    browser: Rc<Browser>,
    offered: bool,
}

impl WithoutPassphrase {
    /// `offered` is what `SYNC_PASSPHRASE_OFFERED` says meanwhile: true keeps the offer from
    /// showing.
    pub(crate) fn sign_in(browser: &Rc<Browser>, offered: bool) -> Result<Self, String> {
        let account = account_without_passphrase().map_err(err)?;
        let this = WithoutPassphrase {
            browser: browser.clone(),
            offered: offered_pref(browser),
        };
        browser.write_pref(&keys::SYNC_PASSPHRASE_OFFERED, &offered);
        browser
            .core(|p| {
                account.save(&mut p.sync())?;
                p.sync()
                    .set_secret_state("account.session", b"x")
                    .map_err(vsesvit_sync::Error::from)
            })
            .map_err(err)?;
        sync::reload(browser);
        Ok(this)
    }
}

impl Drop for WithoutPassphrase {
    fn drop(&mut self) {
        sync::sign_out(&self.browser);
        self.browser
            .write_pref(&keys::SYNC_PASSPHRASE_OFFERED, &self.offered);
    }
}

fn offered_pref(browser: &Browser) -> bool {
    browser.core(|p| p.prefs().get(&keys::SYNC_PASSPHRASE_OFFERED))
}

fn stored_encryption(browser: &Browser) -> Option<Encryption> {
    browser
        .core(|p| Account::load(&mut p.sync()))
        .ok()
        .flatten()
        .map(|a| a.encryption())
}

/// Waits for the prompt asking `asks` to show by itself.
async fn prompted(p: &Probe, browser: &Browser, asks: Asks) -> Rc<PromptDialog> {
    until(p, |p| {
        let shown = sync_prompt::shown(browser);
        p.observe(format!(
            "waiting for the {asks:?} prompt; shown: {:?}",
            shown.as_ref().map(|s| s.asks())
        ));
        shown.filter(|s| s.asks() == asks)
    })
    .await
}

/// Presses one of the prompt's own buttons.
fn press(prompt: &PromptDialog, button: &str) -> Result<(), String> {
    let found = prompt
        .button(button)
        .ok_or_else(|| format!("the prompt has no {button}"))?;
    invoke(&found).map_err(err)
}

/// The check; the profile signs out again, whether it passed or not.
pub(super) async fn sync_passphrase(
    window: &Rc<BrowserWindow>,
    p: &Probe,
) -> Result<String, String> {
    let browser = window.browser().ok_or("no browser")?;
    let _signed_in = WithoutPassphrase::sign_in(&browser, false)?;
    let detail = [
        offer(window, &browser, p).await?,
        set(window, &browser, p).await?,
        enter(&browser, p).await?,
    ];
    Ok(detail.join("; "))
}

/// Settings shows the account syncing unencrypted, and the offer shows by itself, once.
async fn offer(window: &Rc<BrowserWindow>, browser: &Browser, p: &Probe) -> Result<String, String> {
    let status = browser.sync().status();
    let preview = settings_on(window, "SyncPanel").await.map_err(err)?;
    let buttons = ["SyncSyncNow", "SyncSetPassphrase"].map(|b| preview.find::<Button>(b).is_ok());
    if !status.subtitle.contains("Not encrypted") || buttons != [true, true] {
        return Err(format!(
            "Settings says {:?}; Sync Now and Encrypt with a Passphrase… shown: {buttons:?}",
            status.subtitle
        ));
    }
    let prompt = prompted(p, browser, Asks::Offer).await;
    let words = prompt.words();
    let (_, body, accept, decline) = OFFER;
    if words != (body.to_owned(), accept.to_owned(), decline.to_owned()) {
        return Err(format!("the offer says {words:?}"));
    }
    press(&prompt, "CloseButton")?;
    until(p, |p| {
        let (open, shown, offered) = (
            prompt.is_open(),
            sync_prompt::shown(browser).is_some(),
            offered_pref(browser),
        );
        p.observe(format!(
            "after Not Now: open {open}, a prompt shown {shown}, offered {offered}"
        ));
        (!open && !shown && offered).then_some(())
    })
    .await;
    sync::reload(browser);
    exec::sleep(SETTLE).await;
    if let Some(again) = sync_prompt::shown(browser) {
        return Err(format!("after a reload {:?} showed again", again.asks()));
    }
    Ok(format!(
        "Settings says {:?} with Sync Now and Encrypt with a Passphrase…; the offer showed by itself in core's words, and Not Now closed it for good",
        status.subtitle
    ))
}

/// Encrypt with a Passphrase… in Settings sets one.
async fn set(window: &Rc<BrowserWindow>, browser: &Browser, p: &Probe) -> Result<String, String> {
    let preview = settings_on(window, "SyncPanel").await.map_err(err)?;
    let page = preview
        .wired::<SyncPage>()
        .ok_or("the Settings dialog has no Sync page")?;
    invoke(&preview.find::<Button>("SyncSetPassphrase").map_err(err)?).map_err(err)?;
    let flyout = until(p, |p| {
        p.observe("Encrypt with a Passphrase… opened no flyout");
        page.passphrase()
    })
    .await;
    let opened = flyout.shown();
    let mut typed = Vec::new();
    for (text, confirm) in [
        ("short", "short"),
        (PASSPHRASE, "correct horse batterY"),
        (PASSPHRASE, PASSPHRASE),
    ] {
        flyout.fill(text, confirm).map_err(err)?;
        exec::sleep(SETTLE).await;
        typed.push(flyout.shown());
    }
    let expected = [
        (2, Some("Use at least 8 characters".to_owned()), false),
        (2, Some("The passphrases don't match".to_owned()), false),
        (2, None, true),
    ];
    if opened != (2, None, false) || typed != expected {
        return Err(format!("the flyout opened {opened:?}; typed {typed:?}"));
    }
    flyout.accept();
    drop(flyout);
    until(p, |p| {
        let stored = stored_encryption(browser);
        let change = preview.find::<Button>("SyncChangePassphrase").is_ok();
        let open = page.passphrase().is_some();
        p.observe(format!(
            "stored {stored:?}, Change Passphrase… shown: {change}, flyout open: {open}"
        ));
        (stored == Some(Encryption::Ready) && change && !open).then_some(())
    })
    .await;
    Ok("Encrypt with a Passphrase… took only a valid passphrase, stored a Ready account, and Settings offers Change Passphrase…".into())
}

/// Without its keys the device is asked for the passphrase, by a prompt that stays open when
/// asked to close as Escape does, says a wrong one is wrong, and takes the right one.
async fn enter(browser: &Browser, p: &Probe) -> Result<String, String> {
    until(p, |p| {
        p.observe("waiting for the sync after setting the passphrase to end");
        matches!(
            browser.sync().state(),
            State::SignedIn { syncing: false, .. }
        )
        .then_some(())
    })
    .await;
    browser
        .core(|p| p.sync().set_secret_state("account.keyring", b""))
        .map_err(err)?;
    sync::reload(browser);
    let prompt = prompted(p, browser, Asks::Passphrase(Encryption::Enter)).await;
    let (_, accept, dismiss) = prompt.words();
    if (accept.as_str(), dismiss.as_str()) != ("Start Syncing", "Sign Out") {
        return Err(format!("the prompt offers {accept:?} and {dismiss:?}"));
    }
    prompt.dismiss();
    exec::sleep(SETTLE).await;
    let same = sync_prompt::shown(browser).is_some_and(|s| Rc::ptr_eq(&s, &prompt));
    if !prompt.is_open() || !same {
        return Err("asked to close as Escape does, it closed".into());
    }
    let form = prompt.form().ok_or("the prompt has no passphrase field")?;
    form.fill("wrong horse battery", "").map_err(err)?;
    exec::sleep(SETTLE).await;
    press(&prompt, "PrimaryButton")?;
    let said = until(p, |p| {
        let (_, problem) = form.shown();
        p.observe(format!(
            "after a wrong passphrase the prompt says {problem:?}"
        ));
        problem
    })
    .await;
    if said != "The passphrase is wrong" || !prompt.is_open() {
        return Err(format!(
            "a wrong passphrase said {said:?}; open: {}",
            prompt.is_open()
        ));
    }
    form.fill(PASSPHRASE, "").map_err(err)?;
    exec::sleep(SETTLE).await;
    press(&prompt, "PrimaryButton")?;
    until(p, |p| {
        let stored = stored_encryption(browser);
        p.observe(format!(
            "after the right passphrase: stored {stored:?}, open {}",
            prompt.is_open()
        ));
        (stored == Some(Encryption::Ready) && !prompt.is_open()).then_some(())
    })
    .await;
    Ok(format!(
        "without its keys the passphrase prompt showed by itself with {accept:?} and {dismiss:?}, stayed open when asked to close as Escape does, said {said:?} for a wrong passphrase and took the right one"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seeded_account_syncs_unencrypted() {
        let account = account_without_passphrase().expect("an account");
        assert_eq!(account.encryption(), Encryption::Off);
    }
}
