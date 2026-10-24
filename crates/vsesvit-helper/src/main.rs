//! Owned by W1.3.
//! The Linux CEF sub-process image (design section 1.1): `api_hash`, then `execute_process` with the
//! shared `ChildApp`, and nothing else. It links `vsesvit-cef-child` and `cef` only, so no GPUI, X11 or
//! Vulkan code and no thread reaches a renderer. Windows needs none: every process runs the browser's
//! own image there, so this is an empty `main` elsewhere.

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    let code = vsesvit_cef_child::execute_subprocess(std::ptr::null_mut()).unwrap_or(1);
    std::process::ExitCode::from(code.clamp(0, 255) as u8)
}

#[cfg(not(target_os = "linux"))]
fn main() {}
