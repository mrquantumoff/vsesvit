//! Owned by W0.2.
//! The engine contract between the shell and the browser engine: types, the `Engine` trait, events,
//! prompts, hooks and `FakeEngine` (design section 1.5, UA section 4.2). It names no GPUI and no CEF type.
//!
//! Threads (design section 1.6): the engine is a main-thread object, held as `Rc<dyn Engine>`. Every
//! `Engine` and `ShellHooks` method runs on the main thread, which is the engine's UI thread. The one
//! exception is [`RequestFilter`], which runs on the engine's IO thread and is `Send + Sync`. Events are
//! `Send`, so engine threads can queue them; the shell drains them on main after each pump step.
#![deny(missing_docs)]

mod command;
mod event;
pub mod extension;
mod factory;
mod focus;
mod hooks;
mod ids;
mod prompt;
mod types;

pub use command::Engine;
#[cfg(feature = "self-test")]
pub use command::TestShutdown;
pub use event::{BrowserEvent, TabEvent};
pub use factory::{EngineConfig, EngineDriver, EngineFactory, EngineInitError, EngineParts, PasswordStore};
pub use focus::{KeyboardOwner, PageFocus};
pub use hooks::{NeutralizeFailed, ShellHooks};
pub use ids::{EngineDownloadId, MenuCommandId, NativeHandle, ParkedId, PromptId, RequestId, TabId};
pub use prompt::{
    AcceptFilter, AuthChallenger, FileChooserMode, JsDialogKind, MenuEntry, MenuModel, PromptAnswer, PromptEnd,
    PromptError, PromptKind, PromptLedger, PromptRequest, PromptScope, Refused, ScopeState,
};
pub use types::*;
