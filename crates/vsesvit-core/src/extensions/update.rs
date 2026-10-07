//! The update check, shaped like the install pipeline: `prepare_update_check` (UI thread,
//! cheap) captures a job per store extension, `UpdateCheck::run` (any thread, no DB
//! handle) asks each store which of them have a newer version and fetches and verifies
//! those exactly as an install does, and `commit_updates` (UI thread) commits them.
//!
//! ```text
//! prepare_update_check() ──▶ UpdateCheck ──run()──▶ Updates ──commit_updates()──▶ UpdateReport
//!   (records the check time)    │ per store: Chrome Web Store gupdate XML (20 per request),
//!                               │ Edge Add-ons Omaha 3.1 JSON (one request), AMO API (one each)
//!                               ▼
//!                   newer = cmp_versions(store, installed) == Greater ──▶ InstallJob::run
//! ```

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use super::install::{self, MAX_API_RESPONSE_BYTES, network_error};
use super::manifest::cmp_versions;
use super::permissions::RE_ENABLE_LEAD;
use super::{ExtensionId, Extensions, InstallError, InstallJob, InstallSource, InstalledExtension, Intent, StagedInstall, StoreRef, Stores};
use crate::{Error, Url, db};

/// How often Chrome checks its extensions for updates.
pub const UPDATE_INTERVAL: Duration = Duration::from_secs(5 * 60 * 60);
/// The first periodic check waits this long after startup, so it never slows the start.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
/// Extensions per Chrome Web Store update check, as Chrome batches them.
const CWS_BATCH: usize = 20;
/// LOCAL `meta` row: when this device last checked, unix ms.
const CHECKED_KEY: &str = "extensions_update_checked_ms";

/// `Send`, no DB handle. Built by `Extensions::prepare_update_check`, run on any thread.
#[derive(Debug)]
pub struct UpdateCheck {
    candidates: Vec<Candidate>,
    stores: Stores,
    chrome_version: String,
}

#[derive(Debug)]
struct Candidate {
    id: ExtensionId,
    store: StoreRef,
    /// The installed version.
    version: String,
    /// Fetches the store's current version, if the store has a newer one.
    job: InstallJob,
}

/// What `UpdateCheck::run` found and fetched.
#[derive(Debug)]
pub struct Updates {
    /// Newer versions, verified and unpacked, for `commit_updates`.
    pub staged: Vec<StagedInstall>,
    /// Extensions whose check or download failed. The others were not held up.
    pub failed: Vec<(ExtensionId, InstallError)>,
}

/// What `commit_updates` did.
#[derive(Debug, Default)]
pub struct UpdateReport {
    /// Extensions now at a newer version. Some may be [`withheld`](InstalledExtension::withheld).
    pub updated: Vec<InstalledExtension>,
    pub failed: Vec<(ExtensionId, String)>,
}

impl UpdateReport {
    /// One line both shells show after a check the user started: "Updated 2 extensions",
    /// then, for each update that is off until the user re-enables it, Chrome's sentence and
    /// what it can now do.
    pub fn summary(&self) -> String {
        let extensions = |n: usize| if n == 1 { "1 extension".to_owned() } else { format!("{n} extensions") };
        let outcome = match (self.updated.len(), self.failed.len()) {
            (0, 0) => "Your extensions are up to date".to_owned(),
            (0, failed) => format!("{} could not be updated", extensions(failed)),
            (updated, 0) => format!("Updated {}", extensions(updated)),
            (updated, failed) => format!("Updated {}; {failed} could not be updated", extensions(updated)),
        };
        let disabled = self.updated.iter().filter(|ext| !ext.withheld.is_empty()).map(|ext| {
            let warnings: Vec<&str> = ext.withheld.iter().map(|w| w.text.as_str()).collect();
            format!(
                "The newest version of the extension “{}” requires more permissions, so it has been disabled. {RE_ENABLE_LEAD} {}",
                ext.manifest.name,
                warnings.join("; ")
            )
        });
        std::iter::once(outcome).chain(disabled).collect::<Vec<_>>().join(". ")
    }
}

impl Extensions<'_> {
    /// How long until the periodic update check is due: [`FIRST_CHECK_DELAY`] after startup,
    /// then [`UPDATE_INTERVAL`] after this device's last check.
    pub fn next_update_check(&mut self) -> Result<Duration, Error> {
        let last = db::meta_get(&self.p.conn, CHECKED_KEY)?;
        Ok(update_wait(last.map(|ms| ms as u64), self.p.clock.now_ms()))
    }

    /// Cheap and synchronous. Records now as the last check, and captures a job for every
    /// extension installed here from a store that the synced record still wants, enabled
    /// or not (Chrome updates disabled extensions too). Local and unpacked installs have no
    /// store to ask.
    pub fn prepare_update_check(&mut self) -> Result<UpdateCheck, Error> {
        let now = self.p.clock.now_ms() as i64;
        self.p.write(|tx| Ok(db::meta_set(&tx.sql, CHECKED_KEY, now)?))?;
        let installed: Vec<(String, String, String)> = {
            let mut stmt = self.p.conn.prepare(
                "SELECT i.id, i.version, i.source FROM extension_installs i JOIN extensions e ON e.id = i.id \
                 WHERE e.installed AND i.source_kind IN ('chrome_web_store', 'edge_addons', 'amo') ORDER BY i.installed_ms, i.id",
            )?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
        };
        let candidates = installed
            .into_iter()
            .filter_map(|(id, version, source)| {
                let id = ExtensionId::parse(&id).ok()?;
                let source: InstallSource = serde_json::from_str(&source).ok()?;
                let store = source.store()?;
                let job = self.job_expecting(source, Intent::Update, Some(id.clone()));
                Some(Candidate { id, store, version, job })
            })
            .collect();
        Ok(UpdateCheck { candidates, stores: self.p.stores.clone(), chrome_version: self.p.chrome_version.clone() })
    }

    /// UI thread. Commits each update with `Intent::Update`. One the user no longer wants,
    /// or that a local copy replaced meanwhile, is dropped without a word.
    pub fn commit_updates(&mut self, updates: Updates) -> UpdateReport {
        let mut report = UpdateReport {
            updated: Vec::new(),
            failed: updates.failed.into_iter().map(|(id, e)| (id, e.to_string())).collect(),
        };
        for staged in updates.staged {
            let id = staged.id.clone();
            match self.commit_update(staged) {
                Ok(updated) => report.updated.extend(updated),
                Err(e) => report.failed.push((id, e.to_string())),
            }
        }
        report
    }

    /// The extension after the commit, if its version changed.
    fn commit_update(&mut self, staged: StagedInstall) -> Result<Option<InstalledExtension>, Error> {
        let before = self.row(&staged.id)?.map(|row| row.version);
        Ok(self.commit(staged)?.filter(|ext| before.as_deref() != Some(ext.version.as_str())))
    }
}

impl UpdateCheck {
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Blocking. Call off the UI thread. Asks each store which of its extensions have a
    /// newer version, then fetches and verifies those, each as its install would. "Newer"
    /// is decided here, never by the store's status alone. One extension's failure, or one
    /// store's, never stops the others.
    pub fn run(self) -> Updates {
        let UpdateCheck { candidates, stores, chrome_version } = self;
        let agent = stores.agent();
        let mut newer = Vec::new();
        let mut failed = Vec::new();
        let mut decide = |candidate: Candidate, offered: Result<Option<&String>, InstallError>| match offered {
            Ok(Some(version)) if cmp_versions(version, &candidate.version) == Ordering::Greater => newer.push((candidate.id, candidate.job)),
            Ok(_) => {}
            Err(e) => failed.push((candidate.id, e)),
        };

        let (mut cws, others): (Vec<Candidate>, Vec<Candidate>) = candidates.into_iter().partition(|c| c.store == StoreRef::ChromeWebStore);
        let (edge, amo): (Vec<Candidate>, Vec<Candidate>) = others.into_iter().partition(|c| c.store == StoreRef::EdgeAddons);
        while !cws.is_empty() {
            let batch: Vec<Candidate> = cws.drain(..cws.len().min(CWS_BATCH)).collect();
            let apps: Vec<(&ExtensionId, &str)> = batch.iter().map(|c| (&c.id, c.version.as_str())).collect();
            let offered = cws_check(&agent, &stores, &apps, &chrome_version);
            for candidate in batch {
                let found = offered.as_ref().map(|versions| versions.get(&candidate.id)).map_err(retell);
                decide(candidate, found);
            }
        }
        if !edge.is_empty() {
            let apps: Vec<(&ExtensionId, &str)> = edge.iter().map(|c| (&c.id, c.version.as_str())).collect();
            let offered = install::edge_update_check(&agent, &stores, &apps, &chrome_version).and_then(|body| install::parse_edge_updates(&body));
            for candidate in edge {
                let found = offered.as_ref().map(|versions| versions.get(&candidate.id)).map_err(retell);
                decide(candidate, found);
            }
        }
        for candidate in amo {
            let InstallSource::Amo { slug_or_guid } = candidate.job.source() else { continue };
            match install::amo_addon(&agent, &stores, slug_or_guid) {
                Ok(addon) => decide(candidate, Ok(Some(&addon.current_version.version))),
                Err(e) => decide(candidate, Err(e)),
            }
        }

        let mut staged = Vec::new();
        for (id, job) in newer {
            match job.run(&mut |_| {}) {
                Ok(update) => staged.push(update),
                Err(e) => failed.push((id, e)),
            }
        }
        Updates { staged, failed }
    }
}

/// The same failure again, for each extension one failed store request covered.
fn retell(e: &InstallError) -> InstallError {
    match e {
        InstallError::Http(status) => InstallError::Http(*status),
        InstallError::Network(message) => InstallError::Network(message.clone()),
        InstallError::BadStoreResponse(message) => InstallError::BadStoreResponse(message.clone()),
        other => InstallError::BadStoreResponse(other.to_string()),
    }
}

/// How long until the next check: [`FIRST_CHECK_DELAY`] on a device that never checked,
/// else until [`UPDATE_INTERVAL`] after the last check, at least [`FIRST_CHECK_DELAY`] (an
/// overdue check still waits out the startup) and at most [`UPDATE_INTERVAL`] (a clock
/// that went backwards).
fn update_wait(last_checked_ms: Option<u64>, now_ms: u64) -> Duration {
    let Some(last) = last_checked_ms else { return FIRST_CHECK_DELAY };
    let due_ms = last.saturating_add(UPDATE_INTERVAL.as_millis() as u64);
    Duration::from_millis(due_ms.saturating_sub(now_ms)).clamp(FIRST_CHECK_DELAY, UPDATE_INTERVAL)
}

/// `<update service>?response=xml&prodversion=<v>&acceptformat=crx3&x=id%3D<id>%26v%3D<version>%26uc&x=...`
fn cws_check_url(stores: &Stores, apps: &[(&ExtensionId, &str)], chrome_version: &str) -> Url {
    let mut url = stores.cws_update_url.clone();
    {
        let mut query = url.query_pairs_mut();
        query.clear().append_pair("response", "xml").append_pair("prodversion", chrome_version).append_pair("acceptformat", "crx3");
        for (id, version) in apps {
            query.append_pair("x", &format!("id={}&v={version}&uc", id.as_str()));
        }
    }
    url
}

fn cws_check(
    agent: &ureq::Agent,
    stores: &Stores,
    apps: &[(&ExtensionId, &str)],
    chrome_version: &str,
) -> Result<HashMap<ExtensionId, String>, InstallError> {
    let mut response = agent.get(cws_check_url(stores, apps, chrome_version).as_str()).call().map_err(network_error)?;
    let body = response.body_mut().with_config().limit(MAX_API_RESPONSE_BYTES).read_to_vec().map_err(network_error)?;
    parse_gupdate(&body)
}

/// The version the Chrome Web Store offers for each app of a gupdate answer:
/// `<gupdate><app appid=".." status="ok"><updatecheck status="ok" version=".."/></app></gupdate>`.
/// An app whose status or update check is not `ok` (`error-unknownApplication`, `noupdate`)
/// is left out.
fn parse_gupdate(body: &[u8]) -> Result<HashMap<ExtensionId, String>, InstallError> {
    let bad = |e: String| InstallError::BadStoreResponse(e);
    let text = std::str::from_utf8(body).map_err(|e| bad(e.to_string()))?;
    let doc = roxmltree::Document::parse(text).map_err(|e| bad(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "gupdate" {
        return Err(bad(format!("<{}> instead of <gupdate>", root.tag_name().name())));
    }
    let ok = |node: &roxmltree::Node<'_, '_>| node.attribute("status") == Some("ok");
    Ok(root
        .children()
        .filter(|app| app.tag_name().name() == "app" && ok(app))
        .filter_map(|app| {
            let check = app.children().find(|n| n.tag_name().name() == "updatecheck").filter(ok)?;
            Some((ExtensionId::parse(app.attribute("appid")?).ok()?, check.attribute("version")?.to_owned()))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::extensions::Verification;
    use crate::extensions::manifest::Manifest;
    use crate::extensions::permissions::PermissionMessage;

    const A: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";
    const B: &str = "gcllgfdnfnllodcaambdaknbipemelie";

    fn id(s: &str) -> ExtensionId {
        ExtensionId::parse(s).unwrap()
    }

    #[test]
    fn checks_wait_out_the_startup_then_follow_the_interval() {
        let interval = UPDATE_INTERVAL.as_millis() as u64;
        let now = 1_000 * interval;
        assert_eq!(update_wait(None, now), FIRST_CHECK_DELAY, "never checked");
        assert_eq!(update_wait(Some(now - interval / 5), now), UPDATE_INTERVAL * 4 / 5);
        assert_eq!(update_wait(Some(now), now), UPDATE_INTERVAL);
        assert_eq!(update_wait(Some(now - interval - 1), now), FIRST_CHECK_DELAY, "overdue");
        assert_eq!(update_wait(Some(now - interval + 1_000), now), FIRST_CHECK_DELAY, "due within the startup delay");
        assert_eq!(update_wait(Some(now + interval), now), UPDATE_INTERVAL, "a clock that went backwards");
        assert_eq!(update_wait(Some(u64::MAX), 0), UPDATE_INTERVAL);
    }

    #[test]
    fn cws_checks_name_every_extension_and_its_version() {
        let (a, b) = (id(A), id(B));
        let url = cws_check_url(&Stores::default(), &[(&a, "1.2.3"), (&b, "4.0")], "150.0.7000.1");
        assert_eq!(
            url.as_str(),
            format!(
                "https://clients2.google.com/service/update2/crx?response=xml&prodversion=150.0.7000.1&acceptformat=crx3\
                 &x=id%3D{A}%26v%3D1.2.3%26uc&x=id%3D{B}%26v%3D4.0%26uc"
            )
        );
    }

    #[test]
    fn gupdate_answers() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="prod">
              <daystart elapsed_days="6000" elapsed_seconds="100"/>
              <app appid="{A}" cohort="1::" status="ok">
                <updatecheck codebase="https://x/a.crx" hash_sha256="00" size="10" status="ok" version="2.0.1"/>
              </app>
              <app appid="{B}" status="ok"><updatecheck status="noupdate"/></app>
              <app appid="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" status="error-unknownApplication"/>
              <app appid="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" status="ok"><updatecheck status="ok"/></app>
              <app appid="cccccccccccccccccccccccccccccccc" status="error-internal"><updatecheck status="ok" version="9"/></app>
            </gupdate>"#
        );
        assert_eq!(parse_gupdate(xml.as_bytes()).unwrap(), HashMap::from([(id(A), "2.0.1".to_owned())]));
        assert!(parse_gupdate(br#"<gupdate protocol="2.0"/>"#).unwrap().is_empty());
        for malformed in [&b"<gupdate><app"[..], b"<html><body>Sorry</body></html>", b"", b"\xff\xfe"] {
            assert!(matches!(parse_gupdate(malformed), Err(InstallError::BadStoreResponse(_))), "{malformed:?}");
        }
    }

    #[test]
    fn a_failed_store_request_fails_each_of_its_extensions_the_same_way() {
        assert!(matches!(retell(&InstallError::Http(404)), InstallError::Http(404)));
        let bad = retell(&InstallError::BadStoreResponse("no app".into()));
        assert_eq!(bad.to_string(), "unexpected store response: no app");
    }

    fn installed(name: &str, withheld: &[&str]) -> InstalledExtension {
        let manifest = Manifest::parse(&format!(r#"{{"manifest_version": 3, "name": "{name}", "version": "2.0"}}"#), &|_| None).unwrap();
        InstalledExtension {
            id: id(A),
            version: manifest.version.clone(),
            dir: PathBuf::from("x"),
            manifest,
            enabled: withheld.is_empty(),
            withheld: withheld.iter().map(|text| PermissionMessage { text: (*text).to_owned(), details: Vec::new() }).collect(),
            source: InstallSource::ChromeWebStore { id: id(A) },
            verification: Verification::ChromeWebStore,
            engine_id: None,
        }
    }

    #[test]
    fn summaries_read_the_same_in_both_shells() {
        let report = |updated: Vec<InstalledExtension>, failed: usize| UpdateReport {
            updated,
            failed: (0..failed).map(|_| (id(B), "server returned HTTP 404".to_owned())).collect(),
        };
        assert_eq!(report(vec![], 0).summary(), "Your extensions are up to date");
        assert_eq!(report(vec![installed("A", &[])], 0).summary(), "Updated 1 extension");
        assert_eq!(report(vec![installed("A", &[]), installed("B", &[])], 0).summary(), "Updated 2 extensions");
        assert_eq!(report(vec![installed("A", &[])], 1).summary(), "Updated 1 extension; 1 could not be updated");
        assert_eq!(report(vec![], 2).summary(), "2 extensions could not be updated");
        assert_eq!(
            report(
                vec![
                    installed("Ad Blocker", &["Read and change all your data on all websites", "Display notifications"]),
                    installed("B", &[]),
                    installed("Notes", &["Read your browsing history"]),
                ],
                1
            )
            .summary(),
            "Updated 3 extensions; 1 could not be updated. \
             The newest version of the extension “Ad Blocker” requires more permissions, so it has been disabled. \
             It can now: Read and change all your data on all websites; Display notifications. \
             The newest version of the extension “Notes” requires more permissions, so it has been disabled. \
             It can now: Read your browsing history"
        );
    }
}
