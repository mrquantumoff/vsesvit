//! In-app screenshots that never activate or focus a window.
//!
//! - Web content: `CoreWebView2.CapturePreviewAsync`, rendered by the engine.
//! - The whole window: Windows.Graphics.Capture on our own HWND, which reads the window's DWM
//!   composition and works while the window is behind others. It refuses windows on a desktop
//!   that is not the input desktop, such as the private one the self-test runs on, so there
//!   `PrintWindow(PW_RENDERFULLCONTENT)` asks DWM for the same composition instead. Menus that
//!   may extend past the window are windows of their own; each open one is captured the same way
//!   and drawn over the window where it is on screen.

use std::time::Duration;

use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::{exec, xaml};

pub(crate) async fn web_png(core: &CoreWebView2) -> Result<Vec<u8>> {
    let stream = InMemoryRandomAccessStream::new()?;
    core.CapturePreviewAsync(
        CoreWebView2CapturePreviewImageFormat::Png,
        &stream.cast::<IRandomAccessStream>()?,
    )?
    .await?;
    xaml::read_all(&stream.cast()?).await
}

/// A window capture: PNG bytes plus what the self-test needs to know about the pixels.
pub(crate) struct WindowShot {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub flat: bool,
    /// Premultiplied BGRA, `width` by `height`.
    pixels: Vec<u8>,
}

impl WindowShot {
    /// The spread of brightness inside a rectangle of physical pixels (0 for one flat color),
    /// or `None` if it is not all inside the shot.
    pub fn contrast(&self, x: u32, y: u32, width: u32, height: u32) -> Option<u8> {
        if width == 0 || height == 0 || x + width > self.width || y + height > self.height {
            return None;
        }
        let (mut low, mut high) = (u8::MAX, u8::MIN);
        for row in y..y + height {
            for column in x..x + width {
                let at = ((row * self.width + column) * 4) as usize;
                let [b, g, r] = [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]];
                let luma = ((u32::from(r) * 3 + u32::from(g) * 6 + u32::from(b)) / 10) as u8;
                low = low.min(luma);
                high = high.max(luma);
            }
        }
        Some(high - low)
    }
}

pub(crate) async fn window_png(hwnd: HWND) -> Result<WindowShot> {
    let main = capture_window(hwnd).await?;
    let flat = is_flat(&main.pixels);
    let mut layers = vec![main];
    for popup in popups_of(hwnd) {
        match capture_popup(popup).await {
            Ok(layer) => layers.push(layer),
            Err(e) => log::debug!("popup capture: {e}"),
        }
    }
    let shot = composite(layers);
    Ok(WindowShot {
        png: encode_png(&shot.pixels, shot.width, shot.height).await?,
        width: shot.width,
        height: shot.height,
        flat,
        pixels: shot.pixels,
    })
}

/// Premultiplied BGRA pixels and where they are on the screen.
struct Layer {
    pixels: Vec<u8>,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

async fn capture_window(hwnd: HWND) -> Result<Layer> {
    if on_input_desktop() {
        graphics_capture(hwnd).await
    } else {
        print_window(hwnd)
    }
}

/// A menu or other popup. `PrintWindow` draws the transparent margin around its body, where
/// Windows.Graphics.Capture gives its shadow, opaque black; that margin is made transparent.
async fn capture_popup(hwnd: HWND) -> Result<Layer> {
    if on_input_desktop() {
        graphics_capture(hwnd).await
    } else {
        let mut layer = print_window(hwnd)?;
        clear_margin(&mut layer);
        Ok(layer)
    }
}

/// Makes transparent the black pixels that reach the layer's edge through other black pixels:
/// a popup's margin, but not black drawn inside its body, such as in a favicon.
fn clear_margin(layer: &mut Layer) {
    let (width, height) = (layer.width as usize, layer.height as usize);
    let black = |pixels: &[u8], at: usize| pixels[at * 4..at * 4 + 3] == [0, 0, 0];
    let mut seen = vec![false; width * height];
    let mut todo: Vec<usize> = (0..width)
        .flat_map(|x| [x, height.saturating_sub(1) * width + x])
        .chain((0..height).flat_map(|y| [y * width, y * width + width.saturating_sub(1)]))
        .filter(|&at| at < width * height)
        .collect();
    while let Some(at) = todo.pop() {
        if seen[at] || !black(&layer.pixels, at) {
            continue;
        }
        seen[at] = true;
        layer.pixels[at * 4 + 3] = 0;
        let (x, y) = (at % width, at / width);
        if x > 0 {
            todo.push(at - 1);
        }
        if x + 1 < width {
            todo.push(at + 1);
        }
        if y > 0 {
            todo.push(at - width);
        }
        if y + 1 < height {
            todo.push(at + width);
        }
    }
}

/// Whether this thread's desktop is the one that gets input. Windows.Graphics.Capture's
/// `CreateForWindow` fails with E_INVALIDARG for a window on any other. A locked session's input
/// desktop cannot be opened, and counts as another.
fn on_input_desktop() -> bool {
    let input = unsafe { OpenInputDesktop(0, false.into(), DESKTOP_READOBJECTS as u32) };
    if input.is_null() {
        return false;
    }
    let ours = desktop_name(unsafe { GetThreadDesktop(GetCurrentThreadId()) });
    let same = ours.is_some() && ours == desktop_name(input);
    let _ = unsafe { CloseDesktop(input) };
    same
}

fn desktop_name(desktop: HDESK) -> Option<String> {
    let mut name = [0u16; 256];
    let mut needed = 0;
    let read = unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            size_of_val(&name) as u32,
            &mut needed,
        )
    };
    read.as_bool().then(|| {
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        String::from_utf16_lossy(&name[..len])
    })
}

/// The window as DWM composes it, XAML's and WebView2's visuals included, drawn into a bitmap of
/// our own. Works on any desktop.
fn print_window(hwnd: HWND) -> Result<Layer> {
    let mut window = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut window) }.ok()?;
    let (width, height) = (window.right - window.left, window.bottom - window.top);
    if width <= 0 || height <= 0 {
        return Err(windows_core::Error::new(E_FAIL, "the window has no area"));
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: rows top to bottom, as Windows.Graphics.Capture gives them.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB as u32,
            ..BITMAPINFOHEADER::default()
        },
        ..BITMAPINFO::default()
    };
    let pixels = unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS as u32,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        let printed = if bitmap.is_null() {
            Err(windows_core::Error::from_thread())
        } else {
            let previous = SelectObject(dc, bitmap);
            let printed = PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT as u32)
                .ok()
                .map(|()| {
                    std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize)
                        .to_vec()
                });
            SelectObject(dc, previous);
            let _ = DeleteObject(bitmap);
            printed
        };
        let _ = DeleteDC(dc);
        printed?
    };
    let layer = Layer {
        pixels,
        left: window.left,
        top: window.top,
        width: width as u32,
        height: height as u32,
    };
    Ok(crop(layer, frame_bounds(hwnd)))
}

/// The part of `layer` inside `bounds`, both on screen: a window's rectangle includes the
/// invisible resize borders that Windows.Graphics.Capture leaves out.
fn crop(layer: Layer, bounds: RECT) -> Layer {
    let (right, bottom) = (
        layer.left + layer.width as i32,
        layer.top + layer.height as i32,
    );
    let left = bounds.left.clamp(layer.left, right);
    let top = bounds.top.clamp(layer.top, bottom);
    let (width, height) = (
        (bounds.right.clamp(left, right) - left) as usize,
        (bounds.bottom.clamp(top, bottom) - top) as usize,
    );
    if width == 0 || height == 0 {
        return layer;
    }
    let (x, y) = ((left - layer.left) as usize, (top - layer.top) as usize);
    let row = layer.width as usize * 4;
    let pixels = (y..y + height)
        .flat_map(|r| &layer.pixels[r * row + x * 4..r * row + (x + width) * 4])
        .copied()
        .collect();
    Layer {
        pixels,
        left,
        top,
        width: width as u32,
        height: height as u32,
    }
}

async fn graphics_capture(hwnd: HWND) -> Result<Layer> {
    let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
    let size = item.Size()?;
    let device = direct3d_device()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    // Windows 11 lets an app capture its own window without the yellow border.
    if let Ok(session) = session.cast::<IGraphicsCaptureSession3>() {
        let _ = session.SetIsBorderRequired(false);
    }
    if let Ok(session) = session.cast::<IGraphicsCaptureSession2>() {
        let _ = session.SetIsCursorCaptureEnabled(false);
    }
    session.StartCapture()?;
    let frame = exec::wait_for(Duration::from_secs(5), Duration::from_millis(16), || {
        pool.TryGetNextFrame().ok()
    })
    .await;
    let bitmap = match &frame {
        Some(frame) => SoftwareBitmap::CreateCopyFromSurfaceAsync(&frame.Surface()?)?.await,
        None => Err(windows_core::Error::new(
            E_FAIL,
            "no frame from Windows.Graphics.Capture within 5 s",
        )),
    };
    if let Some(frame) = &frame {
        let _ = frame.cast::<IClosable>().and_then(|f| f.Close());
    }
    let _ = session.cast::<IClosable>().and_then(|s| s.Close());
    let _ = pool.cast::<IClosable>().and_then(|p| p.Close());
    let bitmap = bitmap?;
    let (width, height) = (bitmap.PixelWidth()?, bitmap.PixelHeight()?);
    let bounds = frame_bounds(hwnd);
    Ok(Layer {
        pixels: bgra_pixels(&bitmap, width, height)?,
        left: bounds.left,
        top: bounds.top,
        width: width as u32,
        height: height as u32,
    })
}

/// The window's bounds on screen without its invisible resize borders, as captures see it.
fn frame_bounds(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    rect
}

/// A visible top-level window of this process titled `title`, such as a system dialog it
/// opened.
pub(crate) fn find_window(title: &str) -> Option<HWND> {
    process_windows().into_iter().find(|&hwnd| {
        let mut text = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, windows_core::PWSTR(text.as_mut_ptr()), 256) };
        String::from_utf16_lossy(&text[..len.max(0) as usize]) == title
    })
}

/// One window on its own, without its owned popups.
pub(crate) async fn single_window_png(hwnd: HWND) -> Result<Vec<u8>> {
    let layer = capture_window(hwnd).await?;
    encode_png(&layer.pixels, layer.width, layer.height).await
}

/// The visible top-level windows of this process.
fn process_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> windows_core::BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        windows_core::BOOL(1)
    }
    let mut windows: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(collect), (&raw mut windows) as LPARAM) };
    let pid = std::process::id();
    windows
        .into_iter()
        .filter(|&hwnd| {
            let mut owner_pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut owner_pid) };
            owner_pid == pid && unsafe { IsWindowVisible(hwnd) }.as_bool()
        })
        .collect()
}

/// The visible windows of this process that `owner` owns: its open popups and menus.
fn popups_of(owner: HWND) -> Vec<HWND> {
    process_windows()
        .into_iter()
        .filter(|&hwnd| hwnd != owner && unsafe { GetAncestor(hwnd, GA_ROOTOWNER as u32) } == owner)
        .collect()
}

/// What shows behind menus in window captures: the desktop and the blur of their backdrop are
/// not captured, and on this dark grey their text stays legible.
const BACKDROP: [u8; 4] = [0x2C, 0x2C, 0x2C, 0xFF];

/// The columns and rows a popup's border encloses: those of its pixels not faint enough to
/// be its shadow.
fn opaque_bounds(layer: &Layer) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let (width, height) = (layer.width as usize, layer.height as usize);
    let solid = |x: usize, y: usize| layer.pixels[(y * width + x) * 4 + 3] >= 0x30;
    let xs: Vec<usize> = (0..width)
        .filter(|&x| (0..height).any(|y| solid(x, y)))
        .collect();
    let ys: Vec<usize> = (0..height)
        .filter(|&y| (0..width).any(|x| solid(x, y)))
        .collect();
    Some((*xs.first()?..*xs.last()? + 1, *ys.first()?..*ys.last()? + 1))
}

/// The layers drawn in order, each over those before it, on one canvas that holds them all.
fn composite(mut layers: Vec<Layer>) -> Layer {
    if layers.len() == 1 {
        return layers.remove(0);
    }
    let left = layers.iter().map(|l| l.left).min().unwrap_or(0);
    let top = layers.iter().map(|l| l.top).min().unwrap_or(0);
    let right = layers
        .iter()
        .map(|l| l.left + l.width as i32)
        .max()
        .unwrap_or(0);
    let bottom = layers
        .iter()
        .map(|l| l.top + l.height as i32)
        .max()
        .unwrap_or(0);
    let (width, height) = ((right - left).max(0) as u32, (bottom - top).max(0) as u32);
    let mut pixels = BACKDROP.repeat(width as usize * height as usize);
    for (index, layer) in layers.iter().enumerate() {
        let (dx, dy) = ((layer.left - left) as usize, (layer.top - top) as usize);
        let row = layer.width as usize * 4;
        if index == 0 {
            for y in 0..layer.height as usize {
                let dst = ((dy + y) * width as usize + dx) * 4;
                pixels[dst..dst + row].copy_from_slice(&layer.pixels[y * row..(y + 1) * row]);
            }
            continue;
        }
        let body = opaque_bounds(layer);
        for y in 0..layer.height as usize {
            for x in 0..layer.width as usize {
                let src = (y * layer.width as usize + x) * 4;
                let dst = ((dy + y) * width as usize + dx + x) * 4;
                let alpha = u32::from(layer.pixels[src + 3]);
                // A menu's acrylic backdrop is not in its capture, only what is drawn on it; the
                // dark grey stands in for it inside the menu's border, its shadow falls on what
                // is below.
                let inside = body
                    .as_ref()
                    .is_some_and(|(xs, ys)| xs.contains(&x) && ys.contains(&y));
                let under: [u8; 4] = if inside {
                    BACKDROP
                } else {
                    pixels[dst..dst + 4].try_into().unwrap_or(BACKDROP)
                };
                for c in 0..4 {
                    let below = u32::from(under[c]) * (255 - alpha) / 255;
                    pixels[dst + c] = (u32::from(layer.pixels[src + c]) + below).min(255) as u8;
                }
            }
        }
    }
    Layer {
        pixels,
        left,
        top,
        width,
        height,
    }
}

fn bgra_pixels(bitmap: &SoftwareBitmap, width: i32, height: i32) -> Result<Vec<u8>> {
    let length = u32::try_from(i64::from(width) * i64::from(height) * 4)
        .map_err(|_| windows_core::Error::empty())?;
    let buffer = Buffer::Create(length)?;
    bitmap.CopyToBuffer(&buffer.cast::<IBuffer>()?)?;
    let reader = DataReader::FromBuffer(&buffer.cast::<IBuffer>()?)?;
    let mut pixels = vec![0; reader.UnconsumedBufferLength()? as usize];
    reader.ReadBytes(&mut pixels)?;
    Ok(pixels)
}

fn direct3d_device() -> Result<IDirect3DDevice> {
    unsafe {
        let mut device = std::ptr::null_mut();
        D3D11CreateDevice(
            std::ptr::null_mut(),
            D3D_DRIVER_TYPE_HARDWARE,
            std::ptr::null_mut(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT as u32,
            std::ptr::null(),
            0,
            D3D11_SDK_VERSION as u32,
            &mut device,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
        .ok()?;
        let device = ID3D11Device::from_raw(device);
        let dxgi: IDXGIDevice = device.cast()?;
        let mut graphics = std::ptr::null_mut();
        CreateDirect3D11DeviceFromDXGIDevice(dxgi.as_raw(), &mut graphics).ok()?;
        IInspectable::from_raw(graphics).cast()
    }
}

async fn encode_png(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &CryptographicBuffer::CreateFromByteArray(bgra)?,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
        BitmapAlphaMode::Premultiplied,
    )?;
    let stream = InMemoryRandomAccessStream::new()?;
    let encoder = BitmapEncoder::CreateAsync(
        BitmapEncoder::PngEncoderId()?,
        &stream.cast::<IRandomAccessStream>()?,
    )?
    .await?;
    encoder
        .cast::<IBitmapEncoderWithSoftwareBitmap>()?
        .SetSoftwareBitmap(&bitmap)?;
    encoder.FlushAsync()?.await?;
    xaml::read_all(&stream.cast()?).await
}

/// Whether every pixel of a BGRA buffer is the same colour, which is what a window that did
/// not render looks like.
pub(crate) fn is_flat(bgra: &[u8]) -> bool {
    let (pixels, _) = bgra.as_chunks::<4>();
    let Some((first, rest)) = pixels.split_first() else {
        return true;
    };
    rest.iter().all(|pixel| pixel == first)
}

#[cfg(test)]
mod tests {
    use super::{Layer, RECT, clear_margin, crop, is_flat};

    const K: [u8; 4] = [0, 0, 0, 255];
    const G: [u8; 4] = [45, 45, 45, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn layer(rows: &[&[[u8; 4]]], left: i32, top: i32) -> Layer {
        Layer {
            pixels: rows.iter().flat_map(|r| r.iter().flatten().copied()).collect(),
            left,
            top,
            width: rows[0].len() as u32,
            height: rows.len() as u32,
        }
    }

    #[test]
    fn a_popups_black_margin_turns_clear_and_black_inside_its_body_stays() {
        let mut popup = layer(
            &[
                &[K, K, K, K, K],
                &[K, G, G, G, K],
                &[K, G, K, G, K],
                &[K, G, G, G, K],
                &[K, K, K, K, K],
            ],
            0,
            0,
        );
        clear_margin(&mut popup);
        let expected = layer(
            &[
                &[CLEAR, CLEAR, CLEAR, CLEAR, CLEAR],
                &[CLEAR, G, G, G, CLEAR],
                &[CLEAR, G, K, G, CLEAR],
                &[CLEAR, G, G, G, CLEAR],
                &[CLEAR, CLEAR, CLEAR, CLEAR, CLEAR],
            ],
            0,
            0,
        );
        assert_eq!(popup.pixels, expected.pixels);
    }

    #[test]
    fn a_window_is_cut_to_its_frame_bounds() {
        let window = layer(&[&[K, K, K, K], &[K, G, G, K], &[K, G, G, K], &[K, K, K, K]], 10, 20);
        let frame = crop(
            window,
            RECT {
                left: 11,
                top: 21,
                right: 13,
                bottom: 23,
            },
        );
        assert_eq!((frame.left, frame.top, frame.width, frame.height), (11, 21, 2, 2));
        assert_eq!(frame.pixels, [G, G, G, G].concat());
    }

    #[test]
    fn bounds_outside_the_window_leave_it_whole() {
        let window = layer(&[&[G, K], &[K, G]], 0, 0);
        let whole = crop(window, RECT::default());
        assert_eq!((whole.width, whole.height), (2, 2));
    }

    #[test]
    fn flat_and_varied_buffers() {
        assert!(is_flat(&[]));
        assert!(is_flat(&[1, 2, 3, 255, 1, 2, 3, 255]));
        assert!(!is_flat(&[1, 2, 3, 255, 1, 2, 4, 255]));
    }
}
