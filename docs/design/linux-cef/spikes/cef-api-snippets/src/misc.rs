use crate::client::s;
use cef::*;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------- strings

/// Read a CefStringList that CEF lent you (callback parameter) without
/// cloning it (cloning a borrowed list loses the pointer; see gotchas).
pub fn borrowed_list_to_vec(list: &mut CefStringList) -> Vec<String> {
    std::mem::take(list).into_iter().collect()
}

// ---------------------------------------------------------------- tasks

wrap_task! {
    pub struct FnTask {
        f: Arc<Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>>,
    }

    impl Task {
        fn execute(&self) {
            if let Some(f) = self.f.lock().unwrap().take() {
                f();
            }
        }
    }
}

/// Run `f` on a CEF thread (UI, IO, FILE_*...). Returns false if it could not be posted.
pub fn post_on<F: FnOnce() + Send + 'static>(thread: ThreadId, f: F) -> bool {
    let mut task = FnTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    post_task(thread, Some(&mut task)) != 0
}

pub fn post_on_delayed<F: FnOnce() + Send + 'static>(thread: ThreadId, delay_ms: i64, f: F) -> bool {
    let mut task = FnTask::new(Arc::new(Mutex::new(Some(Box::new(f)))));
    post_delayed_task(thread, Some(&mut task), delay_ms) != 0
}

// ---------------------------------------------------------------- request contexts

/// Persistent profile: cache_path must be root_cache_path or a child of it.
pub fn profile_context(root_cache_path: &str, profile_dir: &str) -> Option<RequestContext> {
    let settings = RequestContextSettings {
        cache_path: CefString::from(format!("{root_cache_path}/{profile_dir}").as_str()),
        persist_session_cookies: 1,
        accept_language_list: CefString::from("en-US,en"),
        ..Default::default()
    };
    request_context_create_context(Some(&settings), None)
}

/// Incognito: empty cache_path => in-memory profile ("off the record").
pub fn incognito_context() -> Option<RequestContext> {
    request_context_create_context(Some(&RequestContextSettings::default()), None)
}

/// Chrome pref, e.g. "download.default_directory". UI thread only.
pub fn set_string_pref(ctx: &RequestContext, name: &str, value: &str) -> Result<(), String> {
    let mut v = value_create().ok_or("value_create failed")?;
    v.set_string(Some(&s(value)));
    // NOTE: CefString::default() would be passed as NULL and CEF would return 0.
    let mut err = CefString::from("");
    if ctx.set_preference(Some(&s(name)), Some(&mut v), Some(&mut err)) == 1 {
        Ok(())
    } else {
        Err(err.to_string())
    }
}

pub fn get_pref_json(ctx: &RequestContext, name: &str) -> Option<String> {
    let mut v = ctx.preference(Some(&s(name)))?;
    Some(CefString::from(&write_json(Some(&mut v), JsonWriterOptions::default())).to_string())
}

// ---------------------------------------------------------------- cookies

wrap_cookie_visitor! {
    pub struct CollectCookies {
        out: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    impl CookieVisitor {
        fn visit(
            &self,
            cookie: Option<&Cookie>,
            _count: i32,
            _total: i32,
            _delete_cookie: Option<&mut i32>,
        ) -> i32 {
            if let Some(c) = cookie {
                self.out.lock().unwrap().push((
                    c.domain.to_string(),
                    c.name.to_string(),
                    c.value.to_string(),
                ));
            }
            1 // continue
        }
    }
}

pub fn dump_cookies(ctx: &RequestContext) -> Arc<Mutex<Vec<(String, String, String)>>> {
    let out = Arc::new(Mutex::new(Vec::new()));
    if let Some(cm) = ctx.cookie_manager(None) {
        let mut visitor = CollectCookies::new(out.clone());
        cm.visit_all_cookies(Some(&mut visitor));
    }
    out
}

pub fn set_cookie(ctx: &RequestContext) -> bool {
    let Some(cm) = ctx.cookie_manager(None) else { return false };
    let cookie = Cookie {
        name: s("session"),
        value: s("abc"),
        domain: s(".example.org"),
        path: s("/"),
        secure: 1,
        httponly: 1,
        ..Default::default()
    };
    cm.set_cookie(Some(&s("https://example.org/")), Some(&cookie), None) == 1
}

// ---------------------------------------------------------------- scheme handler

wrap_scheme_handler_factory! {
    pub struct VsesvitSchemeFactory {}

    impl SchemeHandlerFactory {
        // IO thread.
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let body = format!("<h1>vsesvit page for {url}</h1>").into_bytes();
            Some(BytesResource::new(Arc::new(body), Arc::new(Mutex::new(0))))
        }
    }
}

wrap_resource_handler! {
    pub struct BytesResource {
        data: Arc<Vec<u8>>,
        offset: Arc<Mutex<usize>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            _request: Option<&mut Request>,
            handle_request: Option<&mut i32>,
            _callback: Option<&mut Callback>,
        ) -> i32 {
            if let Some(h) = handle_request {
                *h = 1; // handled synchronously
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            if let Some(r) = response {
                r.set_status(200);
                r.set_mime_type(Some(&s("text/html")));
            }
            if let Some(len) = response_length {
                *len = self.data.len() as i64;
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: i32,
            bytes_read: Option<&mut i32>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> i32 {
            let mut off = self.offset.lock().unwrap();
            let rest = &self.data[*off..];
            let n = rest.len().min(bytes_to_read.max(0) as usize);
            if n == 0 {
                if let Some(b) = bytes_read {
                    *b = 0;
                }
                return 0; // done
            }
            unsafe { std::ptr::copy_nonoverlapping(rest.as_ptr(), data_out, n) };
            *off += n;
            if let Some(b) = bytes_read {
                *b = n as i32;
            }
            1
        }

        fn cancel(&self) {}
    }
}

pub fn register_scheme(ctx: Option<&RequestContext>) -> bool {
    let mut factory = VsesvitSchemeFactory::new();
    match ctx {
        Some(ctx) => ctx.register_scheme_handler_factory(Some(&s("vsesvit")), None, Some(&mut factory)) == 1,
        None => register_scheme_handler_factory(Some(&s("vsesvit")), None, Some(&mut factory)) == 1,
    }
}

// ---------------------------------------------------------------- devtools

wrap_dev_tools_message_observer! {
    pub struct CdpObserver {
        results: Arc<Mutex<Vec<(i32, bool, String)>>>,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: i32,
            success: i32,
            result: Option<&[u8]>,
        ) {
            let json = result.map(|r| String::from_utf8_lossy(r).into_owned()).unwrap_or_default();
            self.results.lock().unwrap().push((message_id, success != 0, json));
        }

        fn on_dev_tools_event(
            &self,
            _browser: Option<&mut Browser>,
            _method: Option<&CefString>,
            _params: Option<&[u8]>,
        ) {
        }
    }
}

/// Returns the Registration: dropping it unregisters the observer.
pub fn cdp_navigate(browser: &Browser, url: &str) -> Option<Registration> {
    let host = browser.host()?;
    let mut observer = CdpObserver::new(Arc::new(Mutex::new(Vec::new())));
    let reg = host.add_dev_tools_message_observer(Some(&mut observer));
    let mut params = dictionary_value_create()?;
    params.set_string(Some(&s("url")), Some(&s(url)));
    let _id = host.execute_dev_tools_method(0 /* 0 = auto id */, Some(&s("Page.navigate")), Some(&mut params));
    // Raw JSON alternative:
    let raw = br#"{"id":42,"method":"Runtime.evaluate","params":{"expression":"1+1"}}"#;
    host.send_dev_tools_message(Some(&raw[..]));
    reg
}

// ---------------------------------------------------------------- JS from the browser process

pub fn run_js(browser: &Browser, code: &str) {
    if let Some(frame) = browser.main_frame() {
        frame.execute_java_script(Some(&s(code)), Some(&s("vsesvit://internal/inject.js")), 0);
    }
}

// ---------------------------------------------------------------- out-param strings

/// A CefString whose buffer is *owned by whoever receives it* (CEF frees it via
/// the embedded dtor). Use this when assigning string fields of a struct that
/// CEF handed you as an out-param (WindowInfo.window_name in on_before_popup,
/// LinuxWindowProperties.wayland_app_id, ...). A plain `CefString::from(&str)`
/// assigned there is written back as an EMPTY string (see gotchas).
pub fn cef_owned_string(s: &str) -> CefString {
    let mut raw: sys::cef_string_utf16_t = unsafe { std::mem::zeroed() };
    unsafe { sys::cef_string_utf8_to_utf16(s.as_ptr().cast(), s.len(), &mut raw) };
    CefString::from(raw) // "Borrowed" variant: Rust will not free it
}

wrap_window_delegate! {
    pub struct AppIdWindowDelegate {
        app_id: String,
    }

    impl ViewDelegate {}

    impl PanelDelegate {}

    impl WindowDelegate {
        fn linux_window_properties(
            &self,
            _window: Option<&mut Window>,
            properties: Option<&mut LinuxWindowProperties>,
        ) -> i32 {
            let Some(p) = properties else { return 0 };
            p.wayland_app_id = cef_owned_string(&self.app_id);
            p.wm_class_class = cef_owned_string(&self.app_id);
            p.wm_class_name = cef_owned_string(&self.app_id);
            1
        }
    }
}

// ---------------------------------------------------------------- argv

/// `cef::args::Args::new()` uses `std::env::args()` (panics on non-UTF-8 argv)
/// and its derived `Clone` copies raw pointers into the original buffers.
/// This variant uses args_os and must simply be kept alive for the process.
pub struct OwnedArgs {
    _strings: Vec<std::ffi::CString>,
    _ptrs: Vec<*mut std::os::raw::c_char>,
    main_args: MainArgs,
}

impl OwnedArgs {
    pub fn from_env() -> Self {
        use std::os::unix::ffi::OsStringExt;
        let strings: Vec<std::ffi::CString> = std::env::args_os()
            .map(|a| std::ffi::CString::new(a.into_vec()).expect("NUL in argv"))
            .collect();
        let mut ptrs: Vec<*mut std::os::raw::c_char> =
            strings.iter().map(|s| s.as_ptr() as *mut _).collect();
        ptrs.push(std::ptr::null_mut()); // argv[argc] == NULL like the C runtime
        let main_args = MainArgs { argc: strings.len() as i32, argv: ptrs.as_mut_ptr() };
        Self { _strings: strings, _ptrs: ptrs, main_args }
    }
    pub fn main_args(&self) -> &MainArgs {
        &self.main_args
    }
}
