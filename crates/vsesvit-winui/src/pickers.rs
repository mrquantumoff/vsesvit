//! File and folder pickers (Windows App SDK `Microsoft.Windows.Storage.Pickers`), which work for
//! unpackaged apps given the owner window's id.

use std::path::{Path, PathBuf};

use windows_core::{HSTRING, Result};

use crate::bindings::*;

/// A file with one of `extensions` (`".crx"`), or `None` when the user cancels.
pub(crate) async fn pick_file(owner: WindowId, extensions: &[&str]) -> Result<Option<PathBuf>> {
    let picker = FileOpenPicker::CreateInstance(owner)?;
    let filter = picker.FileTypeFilter()?;
    for extension in extensions {
        filter.Append(&HSTRING::from(*extension))?;
    }
    chosen(picker.PickSingleFileAsync()?.await.map(|r| r.Path()))
}

/// A folder, or `None` when the user cancels.
pub(crate) async fn pick_folder(owner: WindowId) -> Result<Option<PathBuf>> {
    let picker = FolderPicker::CreateInstance(owner)?;
    chosen(picker.PickSingleFolderAsync()?.await.map(|r| r.Path()))
}

/// Where to save a file: the save dialog opens in `folder` with `name` filled in. `None` when
/// the user cancels.
pub(crate) async fn pick_save_file(
    owner: WindowId,
    folder: &Path,
    name: &str,
) -> Result<Option<PathBuf>> {
    let picker = FileSavePicker::CreateInstance(owner)?;
    picker.SetSuggestedFolder(&folder.to_string_lossy())?;
    picker.SetSuggestedFileName(name)?;
    chosen(picker.PickSaveFileAsync()?.await.map(|r| r.Path()))
}

/// A cancelled picker completes with no result object.
fn chosen(result: Result<Result<String>>) -> Result<Option<PathBuf>> {
    match result {
        Ok(path) => Ok(Some(PathBuf::from(path?)).filter(|p| !p.as_os_str().is_empty())),
        Err(e) if e.code().is_ok() => Ok(None),
        Err(e) => Err(e),
    }
}
