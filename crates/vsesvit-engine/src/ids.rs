//! Owned by W0.2.
//! Ids and the native window handle (UA section 4.2 `ids.rs`, design section 1.5).

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};

/// A tab's id. The shell allocates them with [`TabId::next`] (UA section 4.1, rule 6); tabs the engine
/// creates for popups get theirs through `ShellHooks::popup_policy`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(NonZeroU32);

impl TabId {
    /// A new id, unique in this process: an atomic counter from 1.
    pub fn next() -> TabId {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        TabId(NonZeroU32::new(n).expect("tab ids exhausted"))
    }

    /// The id as a number, for logs and the engine's own maps.
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// Tags a slow operation's result, which arrives as `BrowserEvent::RequestFinished`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(pub u64);

/// The engine's id for a download.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineDownloadId(pub u32);

/// A question the engine waits on (design section 1.5, prompts). Issued only when the prompt is admitted.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PromptId(pub u64);

/// A browser that outlived its tab, parked in the hidden host until its downloads end (design section 4.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParkedId(pub u64);

/// A command of the engine's context menu. The engine's id, never invented by the shell.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MenuCommandId(pub i32);

/// A native window: an X11 XID on Linux, an HWND on Windows. Physical facts about it (geometry, parent,
/// visibility) are the shell's platform layer's business; the engine only creates browsers in it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct NativeHandle(u64);

impl NativeHandle {
    /// From the raw value, on any OS. For the glue's conversions and for fakes.
    pub const fn from_raw(raw: u64) -> NativeHandle {
        NativeHandle(raw)
    }

    /// The raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// An X11 window.
    #[cfg(target_os = "linux")]
    pub const fn from_xid(xid: u32) -> NativeHandle {
        NativeHandle(xid as u64)
    }

    /// The X11 window id.
    #[cfg(target_os = "linux")]
    pub const fn xid(self) -> u32 {
        self.0 as u32
    }

    /// A window handle.
    #[cfg(windows)]
    pub fn from_hwnd(hwnd: *mut std::ffi::c_void) -> NativeHandle {
        NativeHandle(hwnd as usize as u64)
    }

    /// The window handle.
    #[cfg(windows)]
    pub fn hwnd(self) -> *mut std::ffi::c_void {
        self.0 as usize as *mut std::ffi::c_void
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_ids_are_unique_and_nonzero() {
        let a = TabId::next();
        let b = TabId::next();
        assert_ne!(a, b);
        assert!(a.get() > 0 && b.get() > 0);
    }

    #[test]
    fn native_handles_round_trip() {
        assert_eq!(NativeHandle::from_raw(0x2a).raw(), 0x2a);
        #[cfg(target_os = "linux")]
        assert_eq!(NativeHandle::from_xid(0x0420_0001).xid(), 0x0420_0001);
        #[cfg(windows)]
        {
            let hwnd = 0x1234usize as *mut std::ffi::c_void;
            assert_eq!(NativeHandle::from_hwnd(hwnd).hwnd(), hwnd);
        }
    }
}
