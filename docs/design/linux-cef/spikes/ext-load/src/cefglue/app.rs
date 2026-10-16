//! `CefApp` for the browser process and for sub-processes.
use super::{CefEvent, Shared};
use cef::*;
use std::sync::Arc;

fn sw(cl: &mut CommandLine, name: &str, value: Option<&str>) {
    match value {
        Some(v) => cl.append_switch_with_value(Some(&CefString::from(name)), Some(&CefString::from(v))),
        None => cl.append_switch(Some(&CefString::from(name))),
    }
}

/// Switches for the browser process. Chrome-style children need Ozone/X11 (CEF #2804).
pub fn browser_switches(cl: &mut CommandLine) {
    sw(cl, "ozone-platform", Some("x11"));
    // On KDE Chromium picks KWallet for cookie encryption (OSCrypt). Until the wallet
    // answers, every network request stalls (seen: data: URLs load, http never starts).
    let store = std::env::var("SPIKE_PASSWORD_STORE").unwrap_or_else(|_| "basic".into());
    if store != "auto" {
        sw(cl, "password-store", Some(&store));
    }
    if let Ok(scale) = std::env::var("SPIKE_SCALE") {
        // Make Chromium agree with GPUI on the device scale factor.
        sw(cl, "force-device-scale-factor", Some(&scale));
    }
    if std::env::var("SPIKE_CDP_PIPE").is_ok_and(|v| v == "1") {
        sw(cl, "remote-debugging-pipe", None);
    }
    // --load-extension=<dir1>,<dir2> (comma-separated; paths may not contain spaces here).
    if let Ok(dirs) = std::env::var("SPIKE_LOAD_EXT") {
        if !dirs.trim().is_empty() {
            sw(cl, "load-extension", Some(dirs.trim()));
        }
    }
    // Free-form extra switches for experiments: SPIKE_CEF_ARGS="--disable-gpu --foo=bar".
    if let Ok(extra) = std::env::var("SPIKE_CEF_ARGS") {
        for arg in extra.split_whitespace() {
            let arg = arg.trim_start_matches("--");
            match arg.split_once('=') {
                Some((k, v)) => sw(cl, k, Some(v)),
                None => sw(cl, arg, None),
            }
        }
    }
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
                crate::log(&format!("cef: browser command line: {}", CefString::from(&cl.command_line_string())));
            }
        }
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(self.bph.clone())
        }
    }
}

impl BrowserApp {
    pub fn create(shared: Arc<Shared>) -> App {
        let client = super::client::new_client(shared.clone());
        BrowserApp::new(SpikeBrowserProcessHandler::new(shared, client))
    }
}

// The App used by `execute_process` (renderer, GPU, utility, zygote). Kept free of
// shell state so the helper executable can use it without the browser-side code.
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
        // ANY thread. Forward to the GPUI pump task (futures channel wakes the GPUI loop).
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
        // Chrome creates windows on its own for some UI (DevTools, Ctrl+N, extension tabs.create...).
        // Returning our client keeps them managed (and shutdown unblocked).
        fn default_client(&self) -> Option<Client> {
            Some(self.client.clone())
        }
    }
}
