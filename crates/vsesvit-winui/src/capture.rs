//! In-app screenshots that never activate or focus a window.
//!
//! - Web content: `CoreWebView2.CapturePreviewAsync`, rendered by the engine.
//! - The whole window: Windows.Graphics.Capture on our own HWND. It reads the window's DWM
//!   composition, so it includes the WebView2 visuals that `PrintWindow` misses, and works while
//!   the window is behind others.

use std::time::{Duration, Instant};

use windows_core::{IInspectable, Interface, Result};

use crate::bindings::*;
use crate::exec;

pub(crate) async fn web_png(core: &CoreWebView2) -> Result<Vec<u8>> {
    let stream = InMemoryRandomAccessStream::new()?;
    core.CapturePreviewAsync(
        CoreWebView2CapturePreviewImageFormat::Png,
        &stream.cast::<IRandomAccessStream>()?,
    )?
    .await?;
    read_all(&stream.cast()?).await
}

/// A window capture: PNG bytes plus what the self-test needs to know about the pixels.
pub(crate) struct WindowShot {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub flat: bool,
}

pub(crate) async fn window_png(hwnd: HWND) -> Result<WindowShot> {
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
    let pixels = bgra_pixels(&bitmap, width, height)?;
    Ok(WindowShot {
        png: encode_png(&bitmap).await?,
        width: width as u32,
        height: height as u32,
        flat: is_flat(&pixels),
    })
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

async fn encode_png(bitmap: &SoftwareBitmap) -> Result<Vec<u8>> {
    let stream = InMemoryRandomAccessStream::new()?;
    let encoder = BitmapEncoder::CreateAsync(
        BitmapEncoder::PngEncoderId()?,
        &stream.cast::<IRandomAccessStream>()?,
    )?
    .await?;
    encoder
        .cast::<IBitmapEncoderWithSoftwareBitmap>()?
        .SetSoftwareBitmap(bitmap)?;
    encoder.FlushAsync()?.await?;
    read_all(&stream.cast()?).await
}

async fn read_all(stream: &IRandomAccessStream) -> Result<Vec<u8>> {
    let size = u32::try_from(stream.Size()?).map_err(|_| windows_core::Error::empty())?;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.await?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
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
