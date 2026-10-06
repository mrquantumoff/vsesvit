//! The `sync_passphrase` check: a profile signed in to an account with no sync passphrase sets
//! one through the flyout on Settings' Sync page, which takes only a long enough passphrase typed
//! the same twice, and then syncs with it.

use std::rc::Rc;
use std::time::Duration;

use serde_json::json;
use vsesvit_sync::status::Action;
use vsesvit_sync::{Account, Encryption};

use super::{Probe, until};
use crate::automation::{invoke, settings_on};
use crate::bindings::*;
use crate::browser::Browser;
use crate::dialogs::SyncPage;
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

/// Stores [`account_without_passphrase`], signed in, and has the sync state follow it.
pub(crate) fn sign_in_without_passphrase(browser: &Browser) -> Result<(), String> {
    let account = account_without_passphrase().map_err(err)?;
    browser
        .core(|p| {
            account.save(&mut p.sync())?;
            p.sync()
                .set_secret_state("account.session", b"x")
                .map_err(vsesvit_sync::Error::from)
        })
        .map_err(err)?;
    sync::reload(browser);
    Ok(())
}

fn stored_encryption(browser: &Browser) -> Option<Encryption> {
    browser
        .core(|p| Account::load(&mut p.sync()))
        .ok()
        .flatten()
        .map(|a| a.encryption())
}

/// The check, after which the profile signs out again, whether it passed or not.
pub(super) async fn sync_passphrase(
    window: &Rc<BrowserWindow>,
    p: &Probe,
) -> Result<String, String> {
    let browser = window.browser().ok_or("no browser")?;
    let result = run(window, &browser, p).await;
    sync::sign_out(&browser);
    result
}

async fn run(window: &Rc<BrowserWindow>, browser: &Browser, p: &Probe) -> Result<String, String> {
    sign_in_without_passphrase(browser)?;
    let actions = browser.sync().status().actions;
    let preview = settings_on(window, "SyncPanel").await.map_err(err)?;
    let page = preview
        .wired::<SyncPage>()
        .ok_or("the Settings dialog has no Sync page")?;
    let mut detail = vec![format!(
        "stored {:?}, offering {:?}",
        stored_encryption(browser),
        actions.iter().map(|a| a.label()).collect::<Vec<_>>()
    )];
    if actions.first() != Some(&Action::SetPassphrase) {
        return Err(detail.join("; "));
    }
    invoke(&preview.find::<Button>("SyncSetPassphrase").map_err(err)?).map_err(err)?;
    let flyout = until(p, |p| {
        p.observe("Set Passphrase… opened no flyout");
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
    detail.push(format!("opened {opened:?}; typed {typed:?}"));
    let expected = [
        (2, Some("Use at least 8 characters".to_owned()), false),
        (2, Some("The passphrases don't match".to_owned()), false),
        (2, None, true),
    ];
    if opened != (2, None, false) || typed != expected {
        return Err(detail.join("; "));
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
    detail
        .push("set: stored Ready, the flyout closed and Settings offers Change Passphrase…".into());
    Ok(detail.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seeded_account_asks_for_a_new_passphrase() {
        let account = account_without_passphrase().expect("an account");
        assert_eq!(account.encryption(), Encryption::Set);
    }
}
