//! Owned by W2.1.
//! Secondary windows (UA section 1.8): `host` opens a dialog window over its parent and closes it with
//! the parent, `prompts` asks the three small questions, `library` keeps one window of each kind. The
//! other files are the features' own dialogs and windows.

pub mod about;
pub mod bookmarks;
pub mod downloads;
pub mod extension_prompts;
pub mod extensions;
pub mod history;
pub mod host;
pub mod library;
pub mod passphrase;
pub mod prompts;
pub mod settings;
pub mod shortcuts_help;
pub mod welcome;
