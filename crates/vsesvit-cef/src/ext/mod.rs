//! Owned by W4.2.
//! The extension half of `CefEngine` (design section 5). Until W4.2 lands, every call fails and foreign
//! browsers stay neutralized until shutdown closes them.

pub mod external;
pub mod prefs_file;
pub mod storage_seed;

use vsesvit_engine::extension::{ExtensionCall, ForeignId};
use vsesvit_engine::{BrowserEvent, RequestId, RequestResult};

use crate::registry;

/// `Engine::extension_call`.
pub fn call(_call: ExtensionCall) -> RequestId {
    let id = registry::next_request();
    if let Some(sink) = registry::sink() {
        let result = RequestResult::Extension(Err("extensions are not available yet".to_owned()));
        sink.send(BrowserEvent::RequestFinished { id, result });
    }
    id
}

/// `Engine::close_foreign`.
pub fn close_foreign(_id: ForeignId) {}
