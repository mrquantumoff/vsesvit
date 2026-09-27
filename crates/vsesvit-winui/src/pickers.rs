//! File and folder pickers (Windows App SDK `Microsoft.Windows.Storage.Pickers`), which work for
//! unpackaged apps given the owner window's id.

use std::path::PathBuf;

use windows_core::{HSTRING, Interface, Result};

use crate::bindings::*;

/// A file with one of `extensions` (`".crx"`), or `None` when the user cancels.
pub(crate) async fn pick_file(owner: WindowId, extensions: &[&str]) -> Result<Option<PathBuf>> {
    let factory = windows_core::factory::<FileOpenPicker, IFileOpenPickerFactory>()?;
    let picker: FileOpenPicker = unsafe {
        let mut picker = std::ptr::null_mut();
        (Interface::vtable(&factory).CreateInstance)(
            Interface::as_raw(&factory),
            owner,
            &mut picker,
        )
        .ok()?;
        FileOpenPicker::from_raw(picker)
    };
    let filter = picker.FileTypeFilter()?;
    for extension in extensions {
        filter.Append(&HSTRING::from(*extension))?;
    }
    chosen(picker.PickSingleFileAsync()?.await.map(|r| r.Path()))
}

/// A folder, or `None` when the user cancels.
pub(crate) async fn pick_folder(owner: WindowId) -> Result<Option<PathBuf>> {
    let factory = windows_core::factory::<FolderPicker, IFolderPickerFactory>()?;
    let picker: FolderPicker = unsafe {
        let mut picker = std::ptr::null_mut();
        (Interface::vtable(&factory).CreateInstance)(
            Interface::as_raw(&factory),
            owner,
            &mut picker,
        )
        .ok()?;
        FolderPicker::from_raw(picker)
    };
    chosen(picker.PickSingleFolderAsync()?.await.map(|r| r.Path()))
}

/// A cancelled picker completes with no result object.
fn chosen(result: Result<Result<String>>) -> Result<Option<PathBuf>> {
    match result {
        Ok(path) => Ok(Some(PathBuf::from(path?)).filter(|p| !p.as_os_str().is_empty())),
        Err(e) if e.code().is_ok() => Ok(None),
        Err(e) => Err(e),
    }
}
