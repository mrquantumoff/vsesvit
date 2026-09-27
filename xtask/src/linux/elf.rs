//! Reads the `DT_NEEDED` entries of an ELF64 little-endian file, so the AppImage bundler can
//! walk library dependencies without `ldd`, which would resolve against the build host instead
//! of the extracted sysroot.

use std::path::Path;

use crate::Result;

use super::util;

const SHT_DYNAMIC: u32 = 6;
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;

/// The sonames the file links against, or `None` when it is not an ELF64 little-endian file.
pub fn needed(path: &Path) -> Result<Option<Vec<String>>> {
    let data = util::read(path)?;
    let Some(header) = data.get(..64) else { return Ok(None) };
    if &header[..4] != b"\x7fELF" || header[4] != 2 || header[5] != 1 {
        return Ok(None);
    }
    let err = || format!("{}: malformed ELF", path.display());
    let shoff = u64_at(&data, 0x28).ok_or_else(err)? as usize;
    let shentsize = u16_at(&data, 0x3a).ok_or_else(err)? as usize;
    let shnum = u16_at(&data, 0x3c).ok_or_else(err)? as usize;

    let section = |index: usize| -> Option<(u32, usize, usize, usize)> {
        let base = shoff + index * shentsize;
        Some((
            u32_at(&data, base + 4)?,
            u64_at(&data, base + 0x18)? as usize,
            u64_at(&data, base + 0x20)? as usize,
            u32_at(&data, base + 0x28)? as usize,
        ))
    };
    let mut sonames = Vec::new();
    for index in 0..shnum {
        let (kind, offset, size, link) = section(index).ok_or_else(err)?;
        if kind != SHT_DYNAMIC {
            continue;
        }
        let (_, str_offset, str_size, _) = section(link).ok_or_else(err)?;
        let strtab = data.get(str_offset..str_offset + str_size).ok_or_else(err)?;
        let dynamic = data.get(offset..offset + size).ok_or_else(err)?;
        for entry in dynamic.as_chunks::<16>().0 {
            let tag = i64::from_le_bytes(entry[..8].try_into().map_err(|_| err())?);
            let value = u64::from_le_bytes(entry[8..].try_into().map_err(|_| err())?) as usize;
            if tag == DT_NULL {
                break;
            }
            if tag == DT_NEEDED {
                let rest = strtab.get(value..).ok_or_else(err)?;
                let end = rest.iter().position(|b| *b == 0).ok_or_else(err)?;
                sonames.push(String::from_utf8_lossy(&rest[..end]).into_owned());
            }
        }
    }
    Ok(Some(sonames))
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes)
}
