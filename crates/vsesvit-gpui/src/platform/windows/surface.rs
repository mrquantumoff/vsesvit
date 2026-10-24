//! Owned by W1.2.
//! The content host of one browser window: a plain child HWND of the GPUI window that parents every
//! tab's CEF child (spike-win §5, §14.3). GPUI's DirectComposition target is topmost, so the page
//! shows only through pixels GPUI leaves transparent, and overlays above it need no cut-out to paint;
//! interactive ones need a window-region hole to take the mouse (spike-win §9.1).
//!
//! Everything here acts on child windows, which sends no message to the GPUI window that GPUI handles,
//! so it runs synchronously, also inside a GPUI update. The calls that do touch a GPUI window are the
//! desktop's, and those are deferred (`windows::defer`).

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, RGN_DIFF, SetWindowRgn};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetParent, HWND_TOP, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetParent, SetWindowPos,
    ShowWindow, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_NOPARENTNOTIFY, WS_VISIBLE,
};

use super::{
    AuxHost, Class, FocusOutcome, FocusTarget, Hole, NativeHandle, NativeSurface, PhysRect, PlatformError, create,
    error, focus, frame, handle, hwnd, last_error, window_attached, window_detached, window_hwnd,
};

pub struct WinSurface {
    /// The GPUI toplevel.
    window: HWND,
    host: HWND,
    content: PhysRect,
    /// The page shown at 0,0 of the host; it follows the content rect.
    shown: Option<HWND>,
    /// Interactive holes, in client pixels.
    holes: Vec<PhysRect>,
    /// The region last applied to the host, as host-pixel cut-outs.
    cutouts: Vec<PhysRect>,
    aux: Vec<HWND>,
}

const HOST_STYLE: u32 = WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | WS_VISIBLE;

impl WinSurface {
    /// The GPUI toplevel this surface lives in.
    pub fn window(&self) -> NativeHandle {
        handle(self.window)
    }

    fn apply_region(&mut self) {
        let cutouts = frame::host_cutouts(self.content, frame::resize_band(self.window), &self.holes);
        if cutouts == self.cutouts {
            return;
        }
        unsafe {
            let region = if cutouts.is_empty() {
                std::ptr::null_mut()
            } else {
                let region = CreateRectRgn(0, 0, self.content.width as i32, self.content.height as i32);
                for c in &cutouts {
                    let hole = CreateRectRgn(c.x, c.y, c.x + c.width as i32, c.y + c.height as i32);
                    CombineRgn(region, region, hole, RGN_DIFF);
                    DeleteObject(hole);
                }
                region
            };
            // The system owns the region from here on.
            if SetWindowRgn(self.host, region, 1) == 0 {
                log::error!("platform: {}", last_error("SetWindowRgn"));
                if !region.is_null() {
                    DeleteObject(region);
                }
                return;
            }
        }
        self.cutouts = cutouts;
    }

    fn fill(&self, page: HWND) {
        let PhysRect { width, height, .. } = self.content;
        unsafe { SetWindowPos(page, std::ptr::null_mut(), 0, 0, width as i32, height as i32, SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    fn release(&mut self, page: HWND) {
        if self.shown == Some(page) {
            self.shown = None;
        }
    }

    fn owns(&self, page: HWND) -> Result<(), PlatformError> {
        match unsafe { GetParent(page) } {
            parent if parent == self.host => Ok(()),
            _ => Err(error(format!("{page:?} is not a page of this window"))),
        }
    }
}

fn reparent(page: HWND, to: HWND) -> Result<(), PlatformError> {
    unsafe {
        if SetParent(page, to).is_null() {
            return Err(last_error("SetParent"));
        }
        if GetParent(page) != to {
            return Err(error("SetParent did not move the page"));
        }
    }
    Ok(())
}

impl NativeSurface for WinSurface {
    const TRANSPARENT_CONTENT: bool = true;

    fn attach(window: &gpui::Window) -> Result<Self, PlatformError> {
        let window = window_hwnd(window)?;
        let host = create(Class::Host, WS_EX_NOPARENTNOTIFY, HOST_STYLE, PhysRect::default(), window)?;
        window_attached(window);
        Ok(WinSurface { window, host, content: PhysRect::default(), shown: None, holes: Vec::new(), cutouts: Vec::new(), aux: Vec::new() })
    }

    fn host(&self) -> NativeHandle {
        handle(self.host)
    }

    fn set_content_rect(&mut self, rect: PhysRect) {
        if rect != self.content {
            self.content = rect;
            let PhysRect { x, y, width, height } = rect;
            unsafe { SetWindowPos(self.host, std::ptr::null_mut(), x, y, width as i32, height as i32, SWP_NOZORDER | SWP_NOACTIVATE) };
            if let Some(page) = self.shown {
                self.fill(page);
            }
        }
        self.apply_region();
    }

    fn show_page(&mut self, page: NativeHandle, visible: bool) {
        let page = hwnd(page);
        if visible {
            self.fill(page);
            unsafe { ShowWindow(page, SW_SHOWNA) };
            self.shown = Some(page);
        } else {
            unsafe { ShowWindow(page, SW_HIDE) };
            self.release(page);
        }
    }

    fn set_holes(&mut self, holes: &[Hole]) {
        self.holes = holes.iter().filter(|h| h.interactive).map(|h| h.rect).collect();
        self.apply_region();
    }

    fn focus(&mut self, target: FocusTarget) -> FocusOutcome {
        focus::focus(self.window, target)
    }

    fn move_page_to(&mut self, page: NativeHandle, to: &mut Self) -> Result<(), PlatformError> {
        let page = hwnd(page);
        self.owns(page)?;
        reparent(page, to.host)?;
        to.fill(page);
        if self.shown == Some(page) {
            self.shown = None;
            to.shown = Some(page);
        }
        Ok(())
    }

    fn park_page(&mut self, page: NativeHandle, hidden_host: NativeHandle) -> Result<(), PlatformError> {
        let page = hwnd(page);
        self.owns(page)?;
        unsafe { ShowWindow(page, SW_HIDE) };
        self.release(page);
        reparent(page, hwnd(hidden_host))
    }

    fn open_aux(&mut self, rect: PhysRect) -> Result<AuxHost, PlatformError> {
        let aux = create(Class::Host, WS_EX_NOPARENTNOTIFY, HOST_STYLE, rect, self.window)?;
        unsafe { SetWindowPos(aux, HWND_TOP, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE) };
        self.aux.push(aux);
        Ok(AuxHost(handle(aux)))
    }

    fn set_aux_rect(&mut self, aux: &AuxHost, rect: PhysRect) {
        let PhysRect { x, y, width, height } = rect;
        unsafe { SetWindowPos(hwnd(aux.0), std::ptr::null_mut(), x, y, width as i32, height as i32, SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    fn close_aux(&mut self, aux: AuxHost) {
        let aux = hwnd(aux.0);
        self.aux.retain(|a| *a != aux);
        unsafe { DestroyWindow(aux) };
    }

    fn capture(&self) -> Result<image::RgbaImage, PlatformError> {
        super::capture::client(self.window)
    }

    fn detach(self) {}
}

/// `detach`, and a surface dropped without it: the hidden host leaves this window first, so the hidden
/// browsers and parked pages outlive it, then the hosts go.
impl Drop for WinSurface {
    fn drop(&mut self) {
        window_detached(self.window);
        unsafe {
            for aux in self.aux.drain(..) {
                DestroyWindow(aux);
            }
            DestroyWindow(self.host);
        }
    }
}
