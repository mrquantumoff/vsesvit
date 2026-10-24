//! Owned by W0.2.
//! The engine contract between the shell and the browser engine: types, the `Engine` trait, events,
//! prompts, hooks and `FakeEngine` (design section 1.5, UA section 4.2). It names no GPUI and no CEF type.
#![deny(missing_docs)]

pub mod extension;

/// Builds the engine in the browser process. W0.2 freezes its methods (UA section 4.2).
pub trait EngineFactory {}

/// A tab's id. The shell allocates them (UA section 4.1, rule 6).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(std::num::NonZeroU32);
