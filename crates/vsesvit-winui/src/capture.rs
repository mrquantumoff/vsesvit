//! In-app screenshots that never activate or focus a window.
//!
//! - Web content: `CoreWebView2.CapturePreviewAsync`, rendered by the engine.
//! - The whole window: Windows.Graphics.Capture on our own HWND. It reads the window's DWM
//!   composition, so it includes the WebView2 visuals that `PrintWindow` misses, and works while
//!   the window is behind others. Menus that may extend past the window are windows of their
//!   own; each open one is captured the same way and drawn over the window where it is on
//!   screen.

use std::time::{Duration, Instant};

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
}

pub(crate) async fn window_png(hwnd: HWND) -> Result<WindowShot> {
    let main = capture_window(hwnd).await?;
    let flat = is_flat(&main.pixels);
    let mut layers = vec![main];
    for popup in popups_of(hwnd) {
        match capture_window(popup).await {
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
    let deadline = Instant::now() + Duration::from_secs(5);
    let frame = loop {
        if let Ok(frame) = pool.TryGetNextFrame() {
            break frame;
        }
        if Instant::now() > deadline {
            let _ = session.cast::<IClosable>().and_then(|s| s.Close());
            let _ = pool.cast::<IClosable>().and_then(|p| p.Close());
            return Err(windows_core::Error::new(
                E_FAIL,
                "no frame from Windows.Graphics.Capture within 5 s",
            ));
        }
        exec::sleep(Duration::from_millis(16)).await;
    };
    let bitmap = SoftwareBitmap::CreateCopyFromSurfaceAsync(&frame.Surface()?)?.await;
    let _ = frame.cast::<IClosable>().and_then(|f| f.Close());
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

/// The visible windows of this process that `owner` owns: its open popups and menus.
fn popups_of(owner: HWND) -> Vec<HWND> {
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
            owner_pid == pid
                && hwnd != owner
                && unsafe { IsWindowVisible(hwnd) }.as_bool()
                && unsafe { GetAncestor(hwnd, GA_ROOTOWNER as u32) } == owner
        })
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
    use super::is_flat;

    #[test]
    fn flat_and_varied_buffers() {
        assert!(is_flat(&[]));
        assert!(is_flat(&[1, 2, 3, 255, 1, 2, 3, 255]));
        assert!(!is_flat(&[1, 2, 3, 255, 1, 2, 4, 255]));
    }
}
