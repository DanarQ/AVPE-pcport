//! Xbox AVP: Extinction formats. Every source address refers to the recorded XBE hash.
pub mod archive;
pub mod catalog;
pub mod compression;
pub mod xbe;
pub mod objects;
pub mod visuals;
pub mod terrain;
pub mod document;
pub mod mesh;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub const FORMAT_VERSION: u32 = 1;
pub const SOURCE_XBE_SHA256: &str = "d5162e3e3e47ba87099e8518505f388f5f8e62ac6f9ffebedc11de5584ec15da";

pub fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset.checked_add(2).context("u16 offset overflow")?;
    let value = bytes.get(offset..end).context("truncated u16")?;
    Ok(u16::from_le_bytes(value.try_into()?))
}

pub fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset.checked_add(4).context("u32 offset overflow")?;
    let value = bytes.get(offset..end).context("truncated u32")?;
    Ok(u32::from_le_bytes(value.try_into()?))
}

pub fn sha256(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 { break; }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Xbox routine 0x000deb50..0x000deb7c, table at 0x0026afa0.
/// Input is normalized to ASCII uppercase by the asset loader before hashing.
pub fn name_hash(name: &str) -> u32 {
    string_hash(&name.to_ascii_uppercase())
}

/// Type and symbol hashes are case-sensitive; only filenames are uppercased.
pub fn string_hash(name: &str) -> u32 {
    let mut crc = u32::MAX;
    for byte in name.bytes() {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    crc
}

pub fn safe_relative(path: &str) -> Result<&Path> {
    let p = Path::new(path);
    ensure!(!path.contains(':') && !p.is_absolute(), "absolute asset path");
    ensure!(p.components().all(|c| matches!(c, std::path::Component::Normal(_))), "unsafe asset path");
    ensure!(!path.split(['/', '\\']).any(|s| s == ".." || s == "." || s.is_empty()), "unsafe asset components");
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xbox_names_match_files() {
        for (name, value) in [("master.tbd",0x370bc447), ("mainmenu.tbd",0xb073ed74), ("background.tbd",0xfd090663), ("logo.tbd",0x06f4f187)] {
            assert_eq!(name_hash(name), value);
        }
    }
    #[test]
    fn rejects_unsafe_paths() {
        for p in ["../x", "a/../../x", "C:\\x", "/x", "a\\..\\x", "a//b"] { assert!(safe_relative(p).is_err()); }
    }
}
