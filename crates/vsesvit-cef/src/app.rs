//! Owned by W1.3.
//! The browser process's `App` (design section 3.3): the switch table on the browser process's command
//! line, the custom schemes (the same table every process registers, from `vsesvit-cef-child`), and a
//! `BrowserProcessHandler` built once: pump wakes, `default_client` for browsers Chromium creates
//! itself, and the singleton relaunch fallback. Sub-processes run `vsesvit_cef_child::ChildApp`.

use std::sync::Arc;
use std::time::Duration;

use cef::*;
use futures::channel::mpsc::UnboundedSender;
use vsesvit_engine::{BrowserEvent, EngineConfig, EngineVersions};

use crate::events::Sink;
use crate::settings::{self, Os, Switch};
use crate::strings;

/// The engine's versions, from the headers this build was made with (G2: `CEF_PATH` matches them).
pub fn versions() -> EngineVersions {
    let engine = std::str::from_utf8(sys::CEF_VERSION).unwrap_or_default().trim_end_matches('\0');
    let chromium = format!(
        "{}.{}.{}.{}",
        sys::CHROME_VERSION_MAJOR,
        sys::CHROME_VERSION_MINOR,
        sys::CHROME_VERSION_BUILD,
        sys::CHROME_VERSION_PATCH
    );
    EngineVersions { engine: format!("CEF {engine}"), chromium }
}

/// Builds the browser-process `App`.
pub fn browser_app(config: &EngineConfig, wake: UnboundedSender<Duration>, sink: Sink) -> App {
    let switches = Arc::new(settings::switches(config, Os::CURRENT));
    let handler = Process::new(wake, sink.clone(), crate::client::build(None, sink));
    BrowserApp::new(handler, switches, Arc::new(config.clone()))
}

fn append(command_line: &mut CommandLine, switch: &Switch) {
    let name = CefString::from(switch.name.as_str());
    match &switch.value {
        Some(value) => command_line.append_switch_with_value(Some(&name), Some(&CefString::from(value.as_str()))),
        None => command_line.append_switch(Some(&name)),
    }
}

wrap_app! {
    pub struct BrowserApp {
        process: BrowserProcessHandler,
        switches: Arc<Vec<Switch>>,
        config: Arc<EngineConfig>,
    }

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let browser = process_type.map(|p| p.to_string().is_empty()).unwrap_or(true);
            let Some(command_line) = command_line else { return };
            if !browser {
                return;
            }
            for switch in self.switches.iter() {
                append(command_line, switch);
            }
            let key = CefString::from("disable-features");
            let existing = strings::userfree(&command_line.switch_value(Some(&key)));
            let merged = settings::disable_features(&existing, &self.config, Os::CURRENT);
            if !merged.is_empty() {
                command_line.append_switch_with_value(Some(&key), Some(&CefString::from(merged.as_str())));
            }
            log::info!("vsesvit-cef: browser command line: {}", strings::userfree(&command_line.command_line_string()));
        }

        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            vsesvit_cef_child::register_schemes(registrar);
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(self.process.clone())
        }
    }
}

wrap_browser_process_handler! {
    pub struct Process {
        wake: UnboundedSender<Duration>,
        sink: Sink,
        client: Client,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            crate::schemes::register_handlers();
        }

        fn on_already_running_app_relaunch(&self, command_line: Option<&mut CommandLine>, current_directory: Option<&CefString>) -> i32 {
            let mut argv = CefStringList::new();
            if let Some(command_line) = command_line {
                command_line.argv(Some(&mut argv));
            }
            let argv = argv.into_iter().map(std::ffi::OsString::from).collect();
            let cwd = std::path::PathBuf::from(strings::text(current_directory));
            self.sink.send(BrowserEvent::AppRelaunch { argv, cwd });
            1
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            let _ = self.wake.unbounded_send(Duration::from_millis(delay_ms.max(0) as u64));
        }

        fn default_client(&self) -> Option<Client> {
            Some(self.client.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_name_cef_and_chromium() {
        let v = super::versions();
        assert!(v.engine.starts_with("CEF 154.0.34+"), "{}", v.engine);
        assert_eq!(v.chromium, "154.0.8037.98");
    }
}
