use crate::UiMsg;
use cef::*;
use std::sync::mpsc::Sender;

/// Custom scheme bits are a C int; combine the sys enum values.
fn scheme_bits(opts: &[SchemeOptions]) -> i32 {
    opts.iter().fold(0u32, |acc, o| acc | o.get_raw()) as i32
}

wrap_app! {
    pub struct VsesvitApp {
        tx: Sender<UiMsg>,
    }

    impl App {
        // Runs in EVERY process (browser + each sub-process) before Chromium parses switches.
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(cl) = command_line else { return };
            let is_browser = process_type.map(|p| p.to_string().is_empty()).unwrap_or(true);
            if is_browser {
                // Example switches; see the cheat-sheet for the vetted list.
                cl.append_switch_with_value(
                    Some(&CefString::from("ozone-platform")),
                    Some(&CefString::from("x11")),
                );
                cl.append_switch(Some(&CefString::from("no-default-browser-check")));
            }
        }

        // Runs in every process; must register the same schemes everywhere.
        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            let Some(r) = registrar else { return };
            r.add_custom_scheme(
                Some(&CefString::from("vsesvit")),
                scheme_bits(&[
                    SchemeOptions::STANDARD,
                    SchemeOptions::SECURE,
                    SchemeOptions::CORS_ENABLED,
                    SchemeOptions::FETCH_ENABLED,
                ]),
            );
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(VsesvitBrowserProcessHandler::new(self.tx.clone()))
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(crate::renderer::VsesvitRenderProcessHandler::new_default())
        }
    }
}

wrap_browser_process_handler! {
    pub struct VsesvitBrowserProcessHandler {
        tx: Sender<UiMsg>,
    }

    impl BrowserProcessHandler {
        // UI thread, after CefInitialize: safe to create browsers / request contexts.
        fn on_context_initialized(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let _ = self.tx.send(UiMsg::ContextReady);
        }

        // Called on ANY thread when external_message_pump = 1.
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            let _ = self.tx.send(UiMsg::PumpWork(delay_ms));
        }

        // Browser process, launcher thread: add switches to child command lines.
        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            if let Some(cl) = command_line {
                cl.append_switch_with_value(
                    Some(&CefString::from("vsesvit-profile")),
                    Some(&CefString::from("Default")),
                );
            }
        }

        // Second launch with same root_cache_path: return 1 to handle it (open window).
        fn on_already_running_app_relaunch(
            &self,
            command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> i32 {
            let _ = command_line.map(|cl| CefString::from(&cl.command_line_string()).to_string());
            1
        }

        fn on_register_custom_preferences(
            &self,
            _type_: PreferencesType,
            _registrar: Option<&mut PreferenceRegistrar>,
        ) {
        }
    }
}
