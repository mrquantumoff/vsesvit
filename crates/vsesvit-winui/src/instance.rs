//! One process per profile. The process that holds the profile lock registers an `AppInstance`
//! key derived from the profile path. A later process whose `Profile::open` fails with `Locked`
//! redirects its activation (which carries its command line) to that instance and exits.
//!
//! `RedirectActivationToAsync` must not be awaited on an STA thread, so the forwarding side runs
//! on its own MTA thread. The receiving side gets `Activated` on a background thread and hands
//! the arguments to the UI thread's dispatcher; activations that arrive before the dispatcher
//! exists wait in the inbox.

use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows_core::{HSTRING, Interface, PCWSTR};

use crate::bindings::*;

/// How long a second process keeps trying to reach the owner of the profile.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(8);
const REGISTER_TIMEOUT: Duration = Duration::from_secs(2);

/// The `AppInstance` key for a profile: case-insensitive in the path, like the file system.
pub(crate) fn key(profile_root: &Path) -> String {
    let path = profile_root.to_string_lossy().to_lowercase();
    let path = path.trim_end_matches(['\\', '/']);
    // FNV-1a: stable across processes and builds, unlike `DefaultHasher`.
    let hash = path.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("vsesvit-profile-{hash:016x}")
}

/// Claims the profile's key for this process. Another process may hold it for a moment while it
/// looks for the owner, so a failed claim is retried briefly.
pub(crate) fn register(profile_root: &Path) -> Option<AppInstance> {
    let key = key(profile_root);
    let deadline = Instant::now() + REGISTER_TIMEOUT;
    loop {
        match AppInstance::FindOrRegisterForKey(&key) {
            Ok(instance) if instance.IsCurrent().unwrap_or(false) => return Some(instance),
            Ok(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Ok(_) => {
                log::warn!("instance key {key} is held by another process; not registered");
                return None;
            }
            Err(e) => {
                log::warn!("instance key {key}: {e}");
                return None;
            }
        }
    }
}

type Deliver = fn(Vec<String>);

struct Inbox {
    queue: Option<(DispatcherQueue, Deliver)>,
    pending: Vec<Vec<String>>,
}

static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    queue: None,
    pending: Vec::new(),
});

/// Subscribes to redirected activations. Call right after `register`, before the UI exists.
pub(crate) fn listen(instance: &AppInstance) {
    let subscribed = instance.Activated(|_, args| {
        let Some(args) = args.as_ref() else { return };
        match launch_arguments(args) {
            Ok(line) => post(command_line_args(&line)),
            Err(e) => log::warn!("redirected activation: {e}"),
        }
    });
    match subscribed {
        // The subscription lives as long as the process.
        Ok(revoker) => std::mem::forget(revoker),
        Err(e) => log::warn!("subscribing to redirected activations: {e}"),
    }
}

/// Starts delivering forwarded command lines to `deliver` on the calling (UI) thread,
/// including any that arrived earlier.
pub(crate) fn deliver_on_this_thread(deliver: Deliver) -> windows_core::Result<()> {
    let queue = DispatcherQueue::GetForCurrentThread()?;
    let pending = {
        let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
        inbox.queue = Some((queue.clone(), deliver));
        std::mem::take(&mut inbox.pending)
    };
    for args in pending {
        enqueue(&queue, deliver, args);
    }
    Ok(())
}

fn post(args: Vec<String>) {
    let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
    match &inbox.queue {
        Some((queue, deliver)) => enqueue(queue, *deliver, args),
        None => inbox.pending.push(args),
    }
}

fn enqueue(queue: &DispatcherQueue, deliver: Deliver, args: Vec<String>) {
    let args = std::cell::Cell::new(Some(args));
    let handler = DispatcherQueueHandler::new(move || {
        if let Some(args) = args.take() {
            deliver(args);
        }
    });
    if !matches!(queue.TryEnqueue(&handler), Ok(true)) {
        log::warn!("forwarded command line dropped: the UI thread is shutting down");
    }
}

/// Hands this process's activation to the process that owns `profile_root`.
pub(crate) fn forward(profile_root: &Path) -> Result<(), String> {
    let key = key(profile_root);
    std::thread::Builder::new()
        .name("vsesvit-forward".into())
        .spawn(move || forward_on_mta(&key))
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the forwarding thread panicked".to_owned())?
}

fn forward_on_mta(key: &str) -> Result<(), String> {
    unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) }
        .ok()
        .map_err(|e| format!("COM: {e}"))?;
    let deadline = Instant::now() + FORWARD_TIMEOUT;
    loop {
        let owner = AppInstance::FindOrRegisterForKey(key).map_err(|e| e.to_string())?;
        if !owner.IsCurrent().map_err(|e| e.to_string())? {
            let args = AppInstance::GetCurrent()
                .and_then(|me| me.GetActivatedEventArgs())
                .map_err(|e| e.to_string())?;
            if let Ok(pid) = owner.ProcessId() {
                // Lets the owner bring its window forward, as this launch had the right to.
                let _ = unsafe { AllowSetForegroundWindow(pid) };
            }
            return owner
                .RedirectActivationToAsync(&args)
                .and_then(|redirect| redirect.join())
                .map_err(|e| e.to_string());
        }
        // Nobody had registered, so this process just did: the owner holds the profile lock
        // but has not registered its key yet. Give the key back and look again.
        let _ = owner.UnregisterKey();
        if Instant::now() >= deadline {
            return Err("the Vsesvit process that has this profile open did not respond".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn launch_arguments(args: &AppActivationArguments) -> windows_core::Result<String> {
    if args.Kind()? != ExtendedActivationKind::Launch {
        return Ok(String::new());
    }
    let data: LaunchActivatedEventArgs = args.Data()?.cast()?;
    unsafe {
        let mut text = std::ptr::null_mut();
        (Interface::vtable(&data).arguments)(Interface::as_raw(&data), &mut text).ok()?;
        let text: HSTRING = std::mem::transmute(text);
        Ok(text.to_string_lossy())
    }
}

/// Splits a command line the way the C runtime does. For an unpackaged launch the arguments
/// start with the executable, which is dropped.
pub(crate) fn command_line_args(line: &str) -> Vec<String> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let wide = HSTRING::from(line);
    let mut count = 0;
    let mut args = Vec::new();
    unsafe {
        let argv = CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut count);
        if argv.is_null() {
            return args;
        }
        for i in 0..usize::try_from(count).unwrap_or(0) {
            args.push((*argv.add(i)).to_string().unwrap_or_default());
        }
        LocalFree(argv.cast());
    }
    if args
        .first()
        .is_some_and(|first| first.to_ascii_lowercase().ends_with(".exe"))
    {
        args.remove(0);
    }
    args
}

// `Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs`. Declared here because its
// name collides with XAML's interface of the same name in the flat generated bindings.
windows_core::imp::define_interface!(
    LaunchActivatedEventArgs,
    LaunchActivatedEventArgs_Vtbl,
    0xfbc93e26_a14a_4b4f_82b0_33bed920af52
);

#[repr(C)]
pub struct LaunchActivatedEventArgs_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    arguments: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> windows_core::HRESULT,
    tile_id: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_case_and_trailing_separators() {
        let a = key(Path::new(r"C:\Users\A\Profile"));
        assert_eq!(a, key(Path::new(r"c:\users\a\profile\")));
        assert_ne!(a, key(Path::new(r"C:\Users\A\Other")));
        assert!(a.starts_with("vsesvit-profile-") && a.len() == 32);
    }

    #[test]
    fn command_lines_split_and_drop_the_executable() {
        assert_eq!(
            command_line_args(
                r#""C:\Program Files\Vsesvit\vsesvit.exe" --profile-dir "C:\p q" https://a.test/"#
            ),
            ["--profile-dir", r"C:\p q", "https://a.test/"]
        );
        assert_eq!(
            command_line_args("https://a.test/ b"),
            ["https://a.test/", "b"]
        );
        assert!(command_line_args("   ").is_empty());
    }
}
