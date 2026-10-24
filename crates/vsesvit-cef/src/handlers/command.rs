//! Owned by W1.3.
//! `CommandHandler::on_chrome_command` (UI thread): Chrome's own commands run only on `idc.rs`'s allow
//! list; every other one is refused, so no Chrome-native UI opens from an accelerator (UA section 1.5,
//! prior-art pitfall P16). The shell's shortcuts arrive through `keyboard.rs`, not here.

use std::sync::Arc;

use cef::*;

use crate::client::HandlerCtx;
use crate::idc;

wrap_command_handler! {
    pub struct Command {
        ctx: Arc<HandlerCtx>,
    }

    impl CommandHandler {
        fn on_chrome_command(&self, browser: Option<&mut Browser>, command_id: i32, _disposition: WindowOpenDisposition) -> i32 {
            if idc::admit(command_id) {
                return 0;
            }
            let id = browser.map(|b| b.identifier()).unwrap_or(-1);
            log::debug!("vsesvit-cef: refused Chrome command {} on browser {id}", idc::name_of(command_id).unwrap_or("?"));
            1
        }
    }
}

impl Command {
    /// The handler for a client.
    pub fn handler(ctx: &Arc<HandlerCtx>) -> CommandHandler {
        Command::new(ctx.clone())
    }
}
