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
pub(crate) const STALLED_FILE_SIZE: u64 = 1_000_000;
pub(crate) const STALLED_FILE_SENT: u64 = 1_000;

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
    /// A 200 response with this content type and body.
    Body(&'static str, Vec<u8>),
    /// Accepts the request and never answers, so the navigation stays provisional.
    Hang,
    /// Closes the connection without a response: a network error.
    Drop,
    /// A file of [`STALLED_FILE_SIZE`] bytes to download, of which only the first
    /// [`STALLED_FILE_SENT`] ever arrive.
    StalledFile,
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
    let html = "text/html; charset=utf-8";
    let (status, content_type, body) = match route(&path) {
        Reply::Page(title) => ("200 OK", html, format!("<!doctype html><title>{title}</title><p>{title}").into_bytes()),
        Reply::Body(content_type, body) => ("200 OK", content_type, body),
        Reply::NotFound => ("404 Not Found", html, Vec::new()),
        Reply::Drop => return,
        Reply::StalledFile => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                 Content-Length: {STALLED_FILE_SIZE}\r\nConnection: close\r\n\r\n"
            );
            let _ = (&stream).write_all(head.as_bytes());
            let _ = (&stream).write_all(&[0; STALLED_FILE_SENT as usize]);
            loop {
                std::thread::park();
            }
        }
        Reply::Hang => loop {
            std::thread::park();
        },
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = (&stream).write_all(&[head.as_bytes(), &body].concat());
}
