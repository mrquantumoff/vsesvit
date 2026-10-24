//! Owned by W1.3.
//! The browsers that are not tabs (design section 3.7): all live in `Desktop`'s hidden host, so no
//! browser window's close affects them. The maintenance browser starts when the shell sets the hidden
//! host and lives until shutdown phase two; later units create the bridge and the short-lived
//! management browsers with [`create`].

use vsesvit_engine::{Browsing, EngineError, NativeHandle, PhysRect};

use crate::handlers::life_span::{self, Request};
use crate::registry::{self, AfterCreate, BrowserId, BrowserKind, CreationId, HiddenRole, Style};

/// The size hidden pages lay out at.
const HIDDEN_SIZE: PhysRect = PhysRect { x: 0, y: 0, width: 1024, height: 768 };

/// `Engine::set_hidden_host`: once, at app start. Starts the maintenance browser (Alloy, `about:blank`,
/// the normal context).
pub fn set_hidden_host(host: NativeHandle) {
    let first = registry::with(|r| r.hidden_host.replace(host).is_none());
    if first != Some(true) {
        log::warn!("vsesvit-cef: the hidden host was set twice; the first one stays in use for live browsers");
        return;
    }
    if let Err(e) = create(HiddenRole::Maintenance, Style::Alloy, "about:blank", Browsing::Normal) {
        log::error!("vsesvit-cef: the maintenance browser was not created: {e}");
    }
}

/// Creates a hidden browser. Refused once shutdown began, and before the hidden host is set.
pub fn create(role: HiddenRole, style: Style, url: &str, browsing: Browsing) -> Result<CreationId, EngineError> {
    let host = registry::with(|r| r.hidden_host)
        .flatten()
        .ok_or_else(|| EngineError::Failed("the hidden host is not set".into()))?;
    life_span::request(Request {
        kind: BrowserKind::Hidden(role),
        browsing,
        style,
        parent: host,
        bounds: HIDDEN_SIZE,
        scale: 1.0,
        url: url.to_owned(),
        then: AfterCreate { hidden: true, ..AfterCreate::default() },
    })
}

/// The live browser of a role, if any.
pub fn browser(role: HiddenRole) -> Option<BrowserId> {
    registry::with(|r| r.entries.iter().find(|(_, e)| e.kind == BrowserKind::Hidden(role)).map(|(id, _)| *id)).flatten()
}

/// Closes every browser of a role (a management browser after its call).
pub fn close(role: HiddenRole) {
    let ids: Vec<BrowserId> = registry::with(|r| {
        r.entries.iter().filter(|(_, e)| e.kind == BrowserKind::Hidden(role)).map(|(id, _)| *id).collect()
    })
    .unwrap_or_default();
    for id in ids {
        life_span::close_browser(id, true);
    }
}
