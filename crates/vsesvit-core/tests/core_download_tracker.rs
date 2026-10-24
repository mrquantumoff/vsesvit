//! The download tracker: reservations, the unconfirmed hold, the progress throttle on an
//! injected clock, the engine's states, the toolbar indicator, the private session and the
//! park question.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vsesvit_core::downloads::{
    DownloadId, Indicator, PROGRESS_INTERVAL, Placement, Reported, Started, State, Tracker, state_for, unconfirmed_path,
};
use vsesvit_core::private::Browsing;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> TempDir {
    let dir = std::env::temp_dir().join(format!("vsesvit-tracker-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

type T = Tracker<&'static str, u32>;

fn started(handle: &'static str, browsing: Browsing, tab: Option<u32>, placement: Placement) -> Started<&'static str, u32> {
    Started { handle, browsing, tab, placement, total: Some(1000) }
}

fn plain(tracker: &mut T, dir: &Path, name: &str) -> Placement {
    let reserved = tracker.reserve(dir, name);
    tracker.place(&reserved, None)
}

#[test]
fn two_downloads_at_once_never_share_a_name() {
    let dir = tmp();
    let mut tracker = T::new();
    let first = tracker.reserve(&dir.0, "a.txt");
    let second = tracker.reserve(&dir.0, "a.txt");
    assert_eq!(first, dir.0.join("a.txt"));
    assert_eq!(second, dir.0.join("a (1).txt"));
    tracker.release(&first);
    assert_eq!(tracker.reserve(&dir.0, "a.txt"), first, "a released name is free again");
}

#[test]
fn a_running_download_and_an_existing_file_keep_their_names() {
    let dir = tmp();
    let mut tracker = T::new();
    std::fs::write(dir.0.join("a.txt"), b"x").unwrap();
    let placement = plain(&mut tracker, &dir.0, "a.txt");
    assert_eq!(placement.destination, dir.0.join("a (1).txt"));
    tracker.track(DownloadId(1), started("h", Browsing::Normal, None, placement), Instant::now());
    assert_eq!(tracker.reserve(&dir.0, "a.txt"), dir.0.join("a (2).txt"), "the running one holds its name, not just a reservation");
}

#[test]
fn a_file_that_runs_code_is_written_under_its_unconfirmed_name() {
    let dir = tmp();
    let mut tracker = T::new();
    let path = tracker.reserve(&dir.0, "tool.jar");
    let placement = tracker.place(&path, None);
    assert!(placement.dangerous);
    assert_eq!(placement.destination, dir.0.join("tool.jar"));
    assert_eq!(placement.written, unconfirmed_path(&dir.0.join("tool.jar")));
    tracker.track(DownloadId(1), started("h", Browsing::Normal, None, placement), Instant::now());
    // Another download of the same name goes to a free one, even though `tool.jar` itself does not exist.
    assert_eq!(tracker.reserve(&dir.0, "tool.jar"), dir.0.join("tool (1).jar"));
}

#[test]
fn a_destination_whose_unconfirmed_name_is_held_is_numbered() {
    let dir = tmp();
    let mut tracker = T::new();
    // The user picked this path in a dialog, so nothing numbered it; another download holds its hold name.
    let held = tracker.place(&dir.0.join("tool.jar"), None);
    tracker.track(DownloadId(1), started("h", Browsing::Normal, None, held), Instant::now());
    let picked = tracker.place(&dir.0.join("tool.jar"), None);
    assert_eq!(picked.destination, dir.0.join("tool (1).jar"));
    assert_eq!(picked.written, dir.0.join("tool (1).jar.unconfirmed"));
}

#[test]
fn a_type_that_does_not_run_code_is_written_in_place() {
    let dir = tmp();
    let mut tracker = T::new();
    let placement = plain(&mut tracker, &dir.0, "notes.txt");
    assert!(!placement.dangerous);
    assert_eq!(placement.written, placement.destination);
}

#[test]
fn a_dangerous_type_is_known_by_its_mime_type_too() {
    let mut tracker = T::new();
    let by_mime = if cfg!(windows) { "application/x-msdownload" } else { "application/x-shellscript" };
    assert!(tracker.place(Path::new("download"), Some(by_mime)).dangerous);
    assert!(!tracker.place(Path::new("download"), Some("text/plain")).dangerous);
}

#[test]
fn progress_reaches_the_views_at_most_every_250_ms() {
    let dir = tmp();
    let mut tracker = T::new();
    let t0 = Instant::now();
    let placement = plain(&mut tracker, &dir.0, "a.bin");
    tracker.track(DownloadId(1), started("h", Browsing::Normal, None, placement), t0);
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    assert!(!tracker.progressed(DownloadId(1), at(100), 100, Some(1000)));
    assert!(!tracker.progressed(DownloadId(1), at(249), 249, Some(1000)));
    assert!(tracker.progressed(DownloadId(1), t0 + PROGRESS_INTERVAL, 250, Some(1000)));
    assert!(!tracker.progressed(DownloadId(1), at(400), 400, Some(1000)), "the interval counts from the last notice");
    assert!(tracker.progressed(DownloadId(1), at(500), 500, Some(1000)));
    assert!(!tracker.progressed(DownloadId(9), at(900), 1, None), "an unknown download is ignored");
    let progress = tracker.progress(DownloadId(1), at(500)).unwrap();
    assert_eq!((progress.received, progress.total), (500, Some(1000)));
}

#[test]
fn the_engine_states_map_to_list_states() {
    use Reported::*;
    let cases = [
        (Running, false, State::InProgress),
        (Completed, false, State::Completed),
        (Completed, true, State::Unconfirmed),
        (Paused, false, State::Paused),
        (Cancelled, false, State::Cancelled),
        (Interrupted { resumable: true }, false, State::Interrupted),
        (Interrupted { resumable: false }, false, State::Failed),
        (Interrupted { resumable: false }, true, State::Failed),
        (Paused, true, State::Paused),
    ];
    for (reported, dangerous, state) in cases {
        assert_eq!(state_for(reported, dangerous), state, "{reported:?}, dangerous {dangerous}");
    }
}

#[test]
fn a_state_change_is_reported_once_and_an_ended_download_leaves() {
    let dir = tmp();
    let mut tracker = T::new();
    let now = Instant::now();
    let placement = tracker.place(&dir.0.join("tool.jar"), None);
    let written = placement.written.clone();
    tracker.track(DownloadId(1), started("h", Browsing::Normal, Some(4), placement), now);
    assert_eq!(tracker.reported(DownloadId(1), Reported::Running, now), None, "no change");
    let paused = tracker.reported(DownloadId(1), Reported::Paused, now).unwrap();
    assert_eq!((paused.state, paused.tab, paused.handle), (State::Paused, Some(4), "h"));
    assert!(tracker.handle(DownloadId(1)).is_some(), "a paused download is still held");
    assert_eq!(tracker.reported(DownloadId(1), Reported::Paused, now), None);
    let done = tracker.reported(DownloadId(1), Reported::Completed, now).unwrap();
    assert_eq!((done.state, done.written), (State::Unconfirmed, written));
    assert!(tracker.handle(DownloadId(1)).is_none());
    assert_eq!(tracker.reported(DownloadId(1), Reported::Completed, now), None);
    assert!(tracker.progress(DownloadId(1), now).is_none());
}

#[test]
fn a_paused_download_has_no_speed() {
    let dir = tmp();
    let mut tracker = T::new();
    let t0 = Instant::now();
    let placement = plain(&mut tracker, &dir.0, "a.bin");
    tracker.track(DownloadId(1), started("h", Browsing::Normal, None, placement), t0);
    tracker.progressed(DownloadId(1), t0 + Duration::from_secs(1), 1000, Some(2000));
    tracker.reported(DownloadId(1), Reported::Paused, t0 + Duration::from_secs(2));
    let paused = tracker.progress(DownloadId(1), t0 + Duration::from_secs(5)).unwrap();
    assert_eq!(paused.per_second, 0, "nothing arrives while paused");
}

#[test]
fn the_downloads_button_follows_what_the_window_lists() {
    let dir = tmp();
    let mut tracker = T::new();
    let now = Instant::now();
    assert_eq!(tracker.indicator(Browsing::Normal), Indicator::Hidden);
    assert_eq!(tracker.indicator(Browsing::Private), Indicator::Hidden);

    let placement = plain(&mut tracker, &dir.0, "p.bin");
    tracker.track(DownloadId(-1), started("private", Browsing::Private, None, placement), now);
    assert_eq!(tracker.indicator(Browsing::Normal), Indicator::Hidden, "a normal window never lists a private download");
    assert_eq!(tracker.indicator(Browsing::Private), Indicator::Busy);

    let placement = plain(&mut tracker, &dir.0, "n.bin");
    tracker.track(DownloadId(1), started("normal", Browsing::Normal, None, placement), now);
    assert_eq!(tracker.indicator(Browsing::Normal), Indicator::Busy);

    tracker.reported(DownloadId(1), Reported::Paused, now);
    assert_eq!(tracker.indicator(Browsing::Normal), Indicator::Idle, "paused is not busy, but the button stays");
    assert_eq!(tracker.indicator(Browsing::Private), Indicator::Busy);

    tracker.reported(DownloadId(1), Reported::Completed, now);
    tracker.reported(DownloadId(-1), Reported::Completed, now);
    assert_eq!(tracker.indicator(Browsing::Normal), Indicator::Idle);
    assert_eq!(tracker.indicator(Browsing::Private), Indicator::Idle);
}

#[test]
fn ending_the_private_session_hides_its_button_and_hands_back_its_downloads() {
    let dir = tmp();
    let mut tracker = T::new();
    let now = Instant::now();
    let placement = plain(&mut tracker, &dir.0, "p.bin");
    tracker.track(DownloadId(-1), started("private", Browsing::Private, None, placement), now);
    let placement = plain(&mut tracker, &dir.0, "n.bin");
    tracker.track(DownloadId(1), started("normal", Browsing::Normal, None, placement), now);
    assert_eq!(tracker.end_private(), vec!["private"]);
    // The engine reports the cancel; until then the download is still held.
    assert!(tracker.handle(DownloadId(-1)).is_some());
    tracker.reported(DownloadId(-1), Reported::Cancelled, now);
    assert!(tracker.handle(DownloadId(-1)).is_none());
    tracker.reported(DownloadId(1), Reported::Completed, now);
    assert_eq!(tracker.indicator(Browsing::Private), Indicator::Idle, "the normal download is listed in private windows too");
    let mut fresh = T::new();
    let placement = plain(&mut fresh, &dir.0, "q.bin");
    fresh.track(DownloadId(-2), started("private", Browsing::Private, None, placement), now);
    fresh.end_private();
    fresh.reported(DownloadId(-2), Reported::Cancelled, now);
    assert_eq!(fresh.indicator(Browsing::Private), Indicator::Hidden);
}

#[test]
fn a_tab_with_a_running_normal_download_is_parked_not_closed() {
    let dir = tmp();
    let mut tracker = T::new();
    let now = Instant::now();
    assert!(!tracker.has_running_for(&1));

    let placement = plain(&mut tracker, &dir.0, "a.bin");
    tracker.track(DownloadId(1), started("h", Browsing::Normal, Some(1), placement), now);
    assert!(tracker.has_running_for(&1));
    assert!(!tracker.has_running_for(&2));

    tracker.reported(DownloadId(1), Reported::Paused, now);
    assert!(!tracker.has_running_for(&1), "a paused download does not need the tab's browser");
    tracker.reported(DownloadId(1), Reported::Running, now);
    assert!(tracker.has_running_for(&1));
    tracker.reported(DownloadId(1), Reported::Completed, now);
    assert!(!tracker.has_running_for(&1));

    let placement = plain(&mut tracker, &dir.0, "p.bin");
    tracker.track(DownloadId(-1), started("h", Browsing::Private, Some(3), placement), now);
    assert!(!tracker.has_running_for(&3), "private downloads are never parked");

    let placement = plain(&mut tracker, &dir.0, "u.bin");
    tracker.track(DownloadId(2), started("h", Browsing::Normal, None, placement), now);
    assert!(!tracker.has_running_for(&0), "a download whose tab is unknown belongs to no tab");
}

#[test]
fn a_failed_start_gives_its_name_back() {
    let dir = tmp();
    let mut tracker = T::new();
    let placement = tracker.place(&dir.0.join("tool.jar"), None);
    tracker.release(&placement.destination);
    assert_eq!(tracker.reserve(&dir.0, "tool.jar"), dir.0.join("tool.jar"));
}
