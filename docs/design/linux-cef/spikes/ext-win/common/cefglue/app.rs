//! `CefApp` for the browser process and for sub-processes.
use super::{CefEvent, Shared};
use cef::*;
use std::sync::{Arc, OnceLock};

fn sw(cl: &mut CommandLine, name: &str, value: Option<&str>) {
    match value {
        Some(v) => cl.append_switch_with_value(Some(&CefString::from(name)), Some(&CefString::from(v))),
        None => cl.append_switch(Some(&CefString::from(name))),
    }
}

/// Windows: the `--remote-debugging-io-pipes` value (Chromium's two pipe handles), set by
/// the platform layer before `cef::initialize`.
pub static IO_PIPES: OnceLock<String> = OnceLock::new();

/// Switches for the browser process.
pub fn browser_switches(cl: &mut CommandLine) {
    let existing = CefString::from(&cl.switch_value(Some(&CefString::from("disable-features")))).to_string();
    let mut features: Vec<String> = existing.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
    // The fixture hosts are http://*.vsesvit.test; no HTTPS upgrade attempts against them.
    features.push("HttpsUpgrades".into());
    #[cfg(windows)]
    {
        // Off-screen window on a private desktop: Chromium would treat it as occluded.
        features.push("CalculateNativeWinOcclusion".into());
        // Never a Windows toast (the Action Center is the user's screen). Toasts are off
        // anyway without a registered activator (notification_platform_bridge_win.cc).
        features.push("NativeNotifications".into());
    }
    features.extend(std::env::var("SPIKE_DISABLE_FEATURES").unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(String::from));
    sw(cl, "disable-features", Some(&features.join(",")));
    #[cfg(not(windows))]
    {
        sw(cl, "ozone-platform", Some("x11"));
        sw(cl, "password-store", Some("basic"));
        if let Ok(scale) = std::env::var("SPIKE_SCALE") {
            sw(cl, "force-device-scale-factor", Some(&scale));
        }
    }
    sw(cl, "noerrdialogs", None);
    if let Ok(port) = std::env::var("SPIKE_REPORT_PORT") {
        // Every profile reaches its own report/update/fixture server under the same names.
        sw(cl, "host-resolver-rules", Some(&format!("MAP *.vsesvit.test 127.0.0.1:{port}")));
    }
    if std::env::var("SPIKE_CDP_PIPE").is_ok_and(|v| v == "1") {
        sw(cl, "remote-debugging-pipe", None);
        if let Some(p) = IO_PIPES.get() {
            sw(cl, "remote-debugging-io-pipes", Some(p));
        }
    }
    if let Ok(dirs) = std::env::var("SPIKE_LOAD_EXT") {
        if !dirs.trim().is_empty() {
            sw(cl, "load-extension", Some(dirs.trim()));
        }
    }
    if let Ok(extra) = std::env::var("SPIKE_CEF_ARGS") {
        for arg in extra.split_whitespace() {
            let arg = arg.trim_start_matches("--");
            match arg.split_once('=') {
                Some((k, v)) => sw(cl, k, Some(v)),
                None => sw(cl, arg, None),
            }
        }
    }
    crate::log(&format!("cef: browser command line: {}", CefString::from(&cl.command_line_string())));
}

wrap_app! {
    pub struct BrowserApp {
        bph: BrowserProcessHandler,
    }

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let is_browser = process_type.map(|p| p.to_string().is_empty()).unwrap_or(true);
            if let (true, Some(cl)) = (is_browser, command_line) {
                browser_switches(cl);
            }
        }
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(self.bph.clone())
        }
    }
}

impl BrowserApp {
    pub fn create(shared: Arc<Shared>) -> App {
        let client = super::client::new_client_kind(shared.clone(), "default");
        BrowserApp::new(SpikeBrowserProcessHandler::new(shared, client))
    }
}

// The App for `execute_process` (renderer, GPU, utility): no shell state.
wrap_app! {
    pub struct ChildApp;

    impl App {
        fn on_before_command_line_processing(&self, _process_type: Option<&CefString>, _command_line: Option<&mut CommandLine>) {}
    }
}

wrap_browser_process_handler! {
    pub struct SpikeBrowserProcessHandler {
        shared: Arc<Shared>,
        client: Client,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            self.shared.log("cef: on_context_initialized");
            self.shared.send(CefEvent::ContextInitialized);
        }
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            self.shared.pump_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = self.shared.pump.unbounded_send(delay_ms);
        }
        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            if let Some(cl) = command_line {
                let t = CefString::from(&cl.switch_value(Some(&CefString::from("type")))).to_string();
                self.shared.log(format!("cef: launching child process type={t}"));
            }
        }
        // Browsers Chrome creates itself (extension tabs.create, windows.create, ...) come
        // through here: ext.rs neutralizes them in on_after_created.
        fn default_client(&self) -> Option<Client> {
            Some(self.client.clone())
        }
    }
}
