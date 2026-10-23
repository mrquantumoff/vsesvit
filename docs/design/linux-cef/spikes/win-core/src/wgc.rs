//! Windows.Graphics.Capture of one of our own toplevel windows (never the screen).
//! DWM renders the window's whole composition tree into the frame, including child
//! HWNDs of other processes (Chromium's GPU-process "Intermediate D3D Window"), which
//! `PrintWindow` cannot see.
use anyhow::{Context as _, Result, anyhow};
use std::time::{Duration, Instant};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::Interface as _;

/// One frame of `hwnd` as RGBA. Returns (width, height, pixels, time to first frame).
pub fn capture(hwnd: isize) -> Result<(u32, u32, Vec<u8>, Duration)> {
    let t0 = Instant::now();
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().context("IGraphicsCaptureItemInterop")?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(hwnd as *mut _)) }.context("CreateForWindow")?;
    let mut size = item.Size()?;
    let item_size = (size.Width, size.Height);
    if size.Width <= 0 || size.Height <= 0 {
        let r = crate::win::window_rect(hwnd);
        size.Width = r.w;
        size.Height = r.h;
    }
    let mut device: Option<ID3D11Device> = None;
    unsafe {
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, windows::Win32::Foundation::HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, None)
            .context("D3D11CreateDevice")?
    };
    let device = device.ok_or_else(|| anyhow!("no d3d11 device"))?;
    let dxgi: IDXGIDevice = device.cast()?;
    let winrt_device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }?.cast()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 1, size).with_context(|| format!("frame pool (item size {item_size:?})"))?;
    let session = pool.CreateCaptureSession(&item).context("capture session")?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let border = session.SetIsBorderRequired(false);
    session.StartCapture().context("StartCapture")?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let frame = loop {
        if let Ok(f) = pool.TryGetNextFrame() {
            break f;
        }
        if Instant::now() > deadline {
            let _ = session.Close();
            let _ = pool.Close();
            return Err(anyhow!("no frame within 3 s (border_required_off={border:?})"));
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let tex: ID3D11Texture2D = unsafe { access.GetInterface() }?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { tex.GetDesc(&mut desc) };
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;
    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }?;
    let staging = staging.ok_or_else(|| anyhow!("no staging texture"))?;
    let ctx = unsafe { device.GetImmediateContext() }?;
    unsafe { ctx.CopyResource(&staging, &tex) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }?;
    let (w, h) = (desc.Width, desc.Height);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    unsafe {
        for y in 0..h {
            let row = std::slice::from_raw_parts((mapped.pData as *const u8).add((y * mapped.RowPitch) as usize), (w * 4) as usize);
            let dst = &mut rgba[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
            for (d, s) in dst.chunks_exact_mut(4).zip(row.chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }
        }
        ctx.Unmap(&staging, 0);
    }
    let _ = session.Close();
    let _ = pool.Close();
    Ok((w, h, rgba, t0.elapsed()))
}
