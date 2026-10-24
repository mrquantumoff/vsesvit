//! Owned by W1.3.
//! What every CEF process of Vsesvit shares (design section 1.1): the process arguments, the custom
//! scheme table, the child-process `App` and its `RenderProcessHandler`, which binds the page-message
//! channel (`vsesvit.post`) in the renderer. It depends on `cef` only, so nothing else of Vsesvit is
//! linked into a renderer.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cef::*;

/// A custom scheme every process registers, with the same options (API section 3.4).
pub struct CustomScheme {
    /// The scheme, without `:`.
    pub name: &'static str,
    /// `cef_scheme_options_t` bits.
    pub options: i32,
}

/// The schemes Vsesvit serves itself: `vsesvit://` for the new tab page and the error and warning pages.
pub const SCHEMES: &[CustomScheme] = &[CustomScheme {
    name: "vsesvit",
    options: sys::cef_scheme_options_t::CEF_SCHEME_OPTION_STANDARD as i32
        | sys::cef_scheme_options_t::CEF_SCHEME_OPTION_SECURE as i32
        | sys::cef_scheme_options_t::CEF_SCHEME_OPTION_CORS_ENABLED as i32,
}];

/// Registers [`SCHEMES`]. Every process's `App::on_register_custom_schemes` calls it.
pub fn register_schemes(registrar: Option<&mut SchemeRegistrar>) {
    let Some(registrar) = registrar else { return };
    for scheme in SCHEMES {
        registrar.add_custom_scheme(Some(&CefString::from(scheme.name)), scheme.options);
    }
}

/// The key of the browser-creation `extra_info` that carries the page channels' allow lists: a
/// dictionary from a channel's process-message name to the list of origins that may post on it.
pub const CHANNELS_KEY: &str = "vsesvit.page_channels";

/// The process-message name of the Web Store shim's channel (design section 1.5, Page messages).
pub const STORE_MESSAGE: &str = "vsesvit.store";

/// The process's arguments, owned for its whole life (G9: `cef::args::Args::new` panics on a non-UTF-8
/// argument on Linux, and its clones dangle). Built once, on the main thread, from `args_os`.
pub struct OwnedArgs {
    #[cfg(not(windows))]
    _strings: Vec<std::ffi::CString>,
    #[cfg(not(windows))]
    _ptrs: Vec<*mut std::os::raw::c_char>,
    #[cfg(not(windows))]
    main_args: MainArgs,
    // Windows: CEF reads the command line itself; the args are only the module handle.
    #[cfg(windows)]
    args: cef::args::Args,
}

thread_local! {
    static ARGS: OnceCell<&'static OwnedArgs> = const { OnceCell::new() };
}

impl OwnedArgs {
    /// The process's arguments. `execute_process` and `initialize` both run on the main thread, which
    /// is the only one that builds them; the value lives until the process exits.
    pub fn get() -> &'static OwnedArgs {
        ARGS.with(|cell| *cell.get_or_init(|| Box::leak(Box::new(OwnedArgs::from_env()))))
    }

    #[cfg(not(windows))]
    fn from_env() -> OwnedArgs {
        use std::os::unix::ffi::OsStringExt;
        let strings: Vec<std::ffi::CString> = std::env::args_os()
            .map(|arg| {
                let mut bytes = arg.into_vec();
                bytes.retain(|b| *b != 0);
                std::ffi::CString::new(bytes).expect("NUL bytes were removed")
            })
            .collect();
        let mut ptrs: Vec<*mut std::os::raw::c_char> = strings.iter().map(|s| s.as_ptr() as *mut _).collect();
        ptrs.push(std::ptr::null_mut());
        let main_args = MainArgs { argc: strings.len() as i32, argv: ptrs.as_mut_ptr() };
        OwnedArgs { _strings: strings, _ptrs: ptrs, main_args }
    }

    #[cfg(windows)]
    fn from_env() -> OwnedArgs {
        OwnedArgs { args: cef::args::Args::new() }
    }

    /// The arguments as CEF takes them.
    pub fn main_args(&self) -> &MainArgs {
        #[cfg(not(windows))]
        return &self.main_args;
        #[cfg(windows)]
        return self.args.as_main_args();
    }
}

/// `api_hash` (G1), then `execute_process` with [`ChildApp`]. `Some(code)` in a CEF sub-process, `None`
/// in the browser process. `sandbox_info` is the Windows sandbox pointer `RunWinMain` received, or null.
pub fn execute_subprocess(sandbox_info: *mut u8) -> Option<i32> {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let mut app = ChildApp::create();
    let code = execute_process(Some(OwnedArgs::get().main_args()), Some(&mut app), sandbox_info);
    (code >= 0).then_some(code)
}

/// The origin of a URL as the browser process serializes it (`scheme://host[:port]`, the port only when
/// it is not the scheme's default), or `None` for a URL without a host. The renderer has no URL parser
/// of its own; the browser process checks every message again with a real one.
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
    let (host, port) = if host_port.starts_with('[') {
        match host_port.split_once("]:") {
            Some((host, port)) => (&host_port[..host.len() + 1], Some(port)),
            None => (host_port, None),
        }
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if port.is_some_and(|p| !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    if host.is_empty() {
        return None;
    }
    let default_port = match scheme.as_str() {
        "http" | "ws" => Some("80"),
        "https" | "wss" => Some("443"),
        _ => None,
    };
    let host = host.to_ascii_lowercase();
    Some(match port {
        Some(port) if !port.is_empty() && Some(port) != default_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// Each browser's page channels: message name and the origins allowed on it, from `extra_info`.
type Channels = Arc<Mutex<HashMap<i32, Vec<(String, Vec<String>)>>>>;

wrap_app! {
    pub struct ChildApp {
        render: RenderProcessHandler,
    }

    impl App {
        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            register_schemes(registrar);
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(self.render.clone())
        }
    }
}

impl ChildApp {
    /// The `App` every CEF sub-process runs.
    pub fn create() -> App {
        ChildApp::new(ChildRender::new(Channels::default()))
    }
}

wrap_render_process_handler! {
    pub struct ChildRender {
        channels: Channels,
    }

    impl RenderProcessHandler {
        fn on_browser_created(&self, browser: Option<&mut Browser>, extra_info: Option<&mut DictionaryValue>) {
            let (Some(browser), Some(extra)) = (browser, extra_info) else { return };
            let Some(dict) = extra.dictionary(Some(&CefString::from(CHANNELS_KEY))) else { return };
            let mut keys = CefStringList::new();
            dict.keys(Some(&mut keys));
            let mut channels = Vec::new();
            for name in keys.into_iter() {
                let Some(list) = dict.list(Some(&CefString::from(name.as_str()))) else { continue };
                let origins = (0..list.size()).map(|i| CefString::from(&list.string(i)).to_string()).collect();
                channels.push((name, origins));
            }
            if let Ok(mut map) = self.channels.lock() {
                map.insert(browser.identifier(), channels);
            }
        }

        fn on_browser_destroyed(&self, browser: Option<&mut Browser>) {
            if let (Some(browser), Ok(mut map)) = (browser, self.channels.lock()) {
                map.remove(&browser.identifier());
            }
        }

        fn on_context_created(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, context: Option<&mut V8Context>) {
            let (Some(browser), Some(frame), Some(context)) = (browser, frame, context) else { return };
            if frame.is_main() == 0 {
                return;
            }
            let Some(origin) = origin_of(&CefString::from(&frame.url()).to_string()) else { return };
            let message = self.channels.lock().ok().and_then(|map| {
                map.get(&browser.identifier())?
                    .iter()
                    .find(|(_, origins)| origins.contains(&origin))
                    .map(|(name, _)| name.clone())
            });
            let Some(message) = message else { return };
            let Some(global) = context.global() else { return };
            let mut handler = PostHandler::new(message);
            let (Some(mut object), Some(mut post)) = (
                v8_value_create_object(None, None),
                v8_value_create_function(Some(&CefString::from("post")), Some(&mut handler)),
            ) else {
                return;
            };
            object.set_value_bykey(Some(&CefString::from("post")), Some(&mut post), V8Propertyattribute::default());
            global.set_value_bykey(Some(&CefString::from("vsesvit")), Some(&mut object), V8Propertyattribute::default());
        }
    }
}

wrap_v8_handler! {
    pub struct PostHandler {
        message: String,
    }

    impl V8Handler {
        fn execute(&self, _name: Option<&CefString>, _object: Option<&mut V8Value>, arguments: Option<&[Option<V8Value>]>,
                   _retval: Option<&mut Option<V8Value>>, exception: Option<&mut CefString>) -> i32 {
            let payload = arguments
                .and_then(|a| a.first().cloned().flatten())
                .filter(|v| v.is_string() != 0)
                .map(|v| CefString::from(&v.string_value()).to_string());
            let Some(payload) = payload else {
                if let Some(e) = exception {
                    e.try_set("vsesvit.post(json) takes a string");
                }
                return 1;
            };
            let frame = v8_context_get_current_context().and_then(|c| c.frame());
            let message = process_message_create(Some(&CefString::from(self.message.as_str())));
            if let (Some(frame), Some(mut message)) = (frame, message) {
                if let Some(args) = message.argument_list() {
                    args.set_string(0, Some(&CefString::from(payload.as_str())));
                }
                frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_match_the_browser_side_serialization() {
        assert_eq!(origin_of("https://Example.com/a?b#c").as_deref(), Some("https://example.com"));
        assert_eq!(origin_of("https://example.com:443/").as_deref(), Some("https://example.com"));
        assert_eq!(origin_of("http://127.0.0.1:8123/x").as_deref(), Some("http://127.0.0.1:8123"));
        assert_eq!(origin_of("http://user:pw@host:80/").as_deref(), Some("http://host"));
        assert_eq!(origin_of("http://[::1]:8080/").as_deref(), Some("http://[::1]:8080"));
        assert_eq!(origin_of("http://[::1]/").as_deref(), Some("http://[::1]"));
        assert_eq!(origin_of("about:blank"), None);
        assert_eq!(origin_of("data:text/html,x"), None);
        assert_eq!(origin_of("file:///tmp/x"), None);
    }

    #[test]
    fn the_scheme_table_names_vsesvit() {
        assert_eq!(SCHEMES.iter().map(|s| s.name).collect::<Vec<_>>(), ["vsesvit"]);
    }
}
