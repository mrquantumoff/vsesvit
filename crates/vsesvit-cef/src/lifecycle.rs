//! Owned by W3.8.
//! `set_frozen` (`Page.setWebLifecycleState`), the document-start scripts and the audio script (Q9,
//! Q11). The bodies below are W1.3's dispatch stubs (design section 8.1).

use vsesvit_engine::TabId;

/// `Engine::set_frozen`.
pub fn set_frozen(_tab: TabId, _frozen: bool) {}

/// `Engine::set_document_start_scripts`.
pub fn set_document_start_scripts(_scripts: Vec<String>) {}
