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

/// Switches for the browser process.
pub fn browser_switches(cl: &mut CommandLine) {
    // The spike window sits off-screen; without this Chromium treats it as occluded and
    // stops rendering. CEF already sets --disable-features, so append to its list.
    let existing = CefString::from(&cl.switch_value(Some(&CefString::from("disable-features")))).to_string();
    let mut features: Vec<&str> = existing.split(',').filter(|s| !s.is_empty()).collect();
    features.push("CalculateNativeWinOcclusion");
    // SPIKE_DISABLE_FEATURES=A,B: more features to disable (experiments).
    let extra = std::env::var("SPIKE_DISABLE_FEATURES").unwrap_or_default();
    features.extend(extra.split(',').filter(|s| !s.is_empty()));
    sw(cl, "disable-features", Some(&features.join(",")));
    if let Ok(scale) = std::env::var("SPIKE_CEF_SCALE") {
        // Experiment: force Chromium's scale regardless of the monitor DPI.
        sw(cl, "force-device-scale-factor", Some(&scale));
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
        let client = super::client::new_client(shared.clone());
        BrowserApp::new(SpikeBrowserProcessHandler::new(shared, client))
    }
}

// The App used by `execute_process` (renderer, GPU, utility). Kept free of shell state
// so the helper executable can use it without the browser-side code.
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
            self.shared.log(format!("cef: on_context_initialized on_ui_thread={}", currently_on(ThreadId::UI)));
            // SPIKE_PREFS="name=value;...": profile prefs on the global request context
            // (true/false become booleans, anything else a string).
            if let (Ok(prefs), Some(ctx)) = (std::env::var("SPIKE_PREFS"), request_context_get_global_context()) {
                for kv in prefs.split(';').filter(|s| !s.is_empty()) {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    let Some(mut val) = value_create() else { continue };
                    match v {
                        "true" | "false" => { val.set_bool((v == "true") as i32); }
                        _ => { val.set_string(Some(&CefString::from(v))); }
                    }
                    let mut err = CefString::default();
                    let ok = ctx.set_preference(Some(&CefString::from(k)), Some(&mut val), Some(&mut err));
                    self.shared.log(format!("cef: set_preference {k}={v} -> {ok} {}", err.to_string()));
                }
            }
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
        // Chrome creates windows on its own for some UI (DevTools, Ctrl+N...). Returning
        // our client keeps them managed (and shutdown unblocked).
        fn default_client(&self) -> Option<Client> {
            Some(self.client.clone())
        }
    }
}
