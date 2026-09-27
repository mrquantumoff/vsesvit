//! Helpers for tests that drive real GTK widgets and WebKit views. Such tests use
//! `#[gtk::test]`, which runs every one of them on a single GTK thread, so they need a
//! display (WSLg or a desktop session). The extension runtime allows one per process, so
//! those tests share one [`Browser`] on a scratch profile.

use std::cell::OnceCell;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::{OpenOptions, Profile};

use crate::browser::Browser;

const WAIT: Duration = Duration::from_secs(15);

thread_local! {
    static BROWSER: OnceCell<Browser> = const { OnceCell::new() };
}

/// The browser shared by this process's GTK tests.
pub(crate) fn browser() -> Browser {
    BROWSER.with(|cell| {
        cell.get_or_init(|| {
            let root = scratch_dir("shared");
            let profile = Profile::open(&root, OpenOptions::default()).expect("a scratch profile");
            Browser::new(&registered_app(), profile)
        })
        .clone()
    })
}

/// An application that windows can be added to, without a bus name of its own.
pub(crate) fn registered_app() -> adw::Application {
    let app = adw::Application::builder()
        .application_id("dev.mrquantumoff.vsesvit.Tests")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gio::Cancellable>).expect("the test application registers");
    app
}

/// An empty directory for this process, cleared of what earlier runs left behind.
pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join("vsesvit-gtk-tests");
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let pid = entry.file_name().to_string_lossy().split('-').next().map(str::to_owned);
            let alive = pid.is_some_and(|pid| std::path::Path::new("/proc").join(pid).exists());
            if !alive {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let dir = base.join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Runs the main loop until `done` holds, failing the test after 15 s.
pub(crate) fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Runs the main loop for `duration`, for effects that have no event to wait for.
pub(crate) fn settle(duration: Duration) {
    let deadline = Instant::now() + duration;
    wait_until("nothing", || Instant::now() >= deadline);
}

/// What the test server does with a request.
pub(crate) enum Reply {
    /// A 200 HTML page with this title.
    Page(&'static str),
    /// Accepts the request and never answers, so the navigation stays provisional.
    Hang,
    /// Closes the connection without a response: a network error.
    Drop,
    NotFound,
}

/// A plain HTTP server on a loopback address, answering by path.
pub(crate) struct Server {
    host: &'static str,
    port: u16,
}

impl Server {
    /// `host` is a loopback address: `127.0.0.1`, or another in `127/8` to give a page a
    /// different site.
    pub(crate) fn start(
        host: &'static str,
        route: impl Fn(&str) -> Reply + Send + Sync + 'static,
    ) -> Server {
        let listener = TcpListener::bind((host, 0)).expect("a loopback port");
        let port = listener.local_addr().expect("a bound address").port();
        let route = Arc::new(route);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let route = route.clone();
                std::thread::spawn(move || serve(stream, &*route));
            }
        });
        Server { host, port }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}:{}{path}", self.host, self.port)
    }
}

fn serve(stream: TcpStream, route: &(dyn Fn(&str) -> Reply + Send + Sync)) {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|n| n > 0) && !header.trim_end().is_empty() {
        header.clear();
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let (status, body) = match route(&path) {
        Reply::Page(title) => ("200 OK", format!("<!doctype html><title>{title}</title><p>{title}")),
        Reply::NotFound => ("404 Not Found", String::new()),
        Reply::Drop => return,
        Reply::Hang => loop {
            std::thread::park();
        },
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = (&stream).write_all(response.as_bytes());
}
