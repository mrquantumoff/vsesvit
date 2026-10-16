use crate::client::s;
use cef::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Renderer-thread state: pending promises keyed by request id.
#[derive(Default)]
pub struct Pending {
    next_id: i32,
    promises: HashMap<i32, (V8Context, V8Value)>,
}

wrap_render_process_handler! {
    pub struct VsesvitRenderProcessHandler {
        pending: Arc<Mutex<Pending>>,
    }

    impl RenderProcessHandler {
        // TID_RENDERER. Expose `window.vsesvit.echo(text) -> Promise<string>`.
        fn on_context_created(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let (Some(frame), Some(ctx)) = (frame, context) else { return };
            if frame.is_main() == 0 {
                return;
            }
            let Some(global) = ctx.global() else { return };
            let mut handler = EchoHandler::new(self.pending.clone());
            let (Some(mut obj), Some(mut func)) = (
                v8_value_create_object(None, None),
                v8_value_create_function(Some(&s("echo")), Some(&mut handler)),
            ) else {
                return;
            };
            obj.set_value_bykey(Some(&s("echo")), Some(&mut func), V8Propertyattribute::default());
            global.set_value_bykey(Some(&s("vsesvit")), Some(&mut obj), V8Propertyattribute::default());
        }

        fn on_context_released(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let Some(ctx) = context else { return };
            let mut p = self.pending.lock().unwrap();
            p.promises.retain(|_, (c, _)| c.is_same(Some(&mut ctx.clone())) == 0);
        }

        // Browser -> renderer IPC.
        fn on_process_message_received(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> i32 {
            let Some(msg) = message else { return 0 };
            if CefString::from(&msg.name()).to_string() != "vsesvit.echo.reply" {
                return 0;
            }
            let Some(args) = msg.argument_list() else { return 0 };
            let id = args.int(0);
            let text = CefString::from(&args.string(1)).to_string();
            let entry = self.pending.lock().unwrap().promises.remove(&id);
            if let Some((ctx, promise)) = entry {
                if ctx.enter() == 1 {
                    let mut v = v8_value_create_string(Some(&s(&text)));
                    promise.resolve_promise(v.as_mut());
                    ctx.exit();
                }
            }
            1
        }
    }
}

impl VsesvitRenderProcessHandler {
    pub fn new_default() -> RenderProcessHandler {
        Self::new(Arc::new(Mutex::new(Pending::default())))
    }
}

wrap_v8_handler! {
    pub struct EchoHandler {
        pending: Arc<Mutex<Pending>>,
    }

    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            retval: Option<&mut Option<V8Value>>,
            exception: Option<&mut CefString>,
        ) -> i32 {
            let text = arguments
                .and_then(|a| a.first().cloned().flatten())
                .filter(|v| v.is_string() != 0)
                .map(|v| CefString::from(&v.string_value()).to_string());
            let Some(text) = text else {
                if let Some(e) = exception {
                    e.try_set("echo(text: string) expects a string"); // must use try_set
                }
                return 1;
            };
            let (Some(ctx), Some(promise)) = (v8_context_get_current_context(), v8_value_create_promise()) else {
                return 0;
            };
            let id = {
                let mut p = self.pending.lock().unwrap();
                p.next_id += 1;
                let id = p.next_id;
                p.promises.insert(id, (ctx.clone(), promise.clone()));
                id
            };
            if let (Some(frame), Some(mut msg)) = (ctx.frame(), process_message_create(Some(&s("vsesvit.echo")))) {
                if let Some(args) = msg.argument_list() {
                    args.set_int(0, id);
                    args.set_string(1, Some(&s(&text)));
                }
                frame.send_process_message(ProcessId::BROWSER, Some(&mut msg));
            }
            if let Some(r) = retval {
                *r = Some(promise);
            }
            1
        }
    }
}
