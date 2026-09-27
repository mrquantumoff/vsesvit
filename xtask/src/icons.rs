//! Renders the app icon from `packaging/icons/<APP_ID>.svg`, so the SVG is the only icon source.

use std::path::Path;

use crate::Result;
use crate::ctx::APP_ID;

/// Sizes of the hicolor PNGs installed on Linux.
pub const PNG_SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
/// Sizes packed into the Windows `.ico`.
const ICO_SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

fn svg_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../packaging/icons/{APP_ID}.svg"))
}

pub fn svg() -> Result<Vec<u8>> {
    let path = svg_path();
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The icon as a square PNG of `size` pixels.
pub fn png(size: u32) -> Result<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(&svg()?, &resvg::usvg::Options::default()).map_err(|e| format!("icon svg: {e}"))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("icon size")?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("icon png: {e}"))
}

/// The icon as a Windows `.ico` with the usual sizes.
pub fn ico() -> Result<Vec<u8>> {
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in ICO_SIZES {
        let image = ico::IconImage::read_png(png(size)?.as_slice()).map_err(|e| format!("icon {size}: {e}"))?;
        dir.add_entry(ico::IconDirEntry::encode(&image).map_err(|e| format!("icon {size}: {e}"))?);
    }
    let mut out = Vec::new();
    dir.write(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Writes `<APP_ID>.svg`, `<size>.png` for every Linux size, and `<APP_ID>.ico` into `out`.
pub fn write_all(out: &Path) -> Result {
    let write = |name: String, bytes: Vec<u8>| std::fs::write(out.join(&name), bytes).map_err(|e| format!("{name}: {e}"));
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    write(format!("{APP_ID}.svg"), svg()?)?;
    for size in PNG_SIZES {
        write(format!("{size}.png"), png(size)?)?;
    }
    write(format!("{APP_ID}.ico"), ico()?)
}
