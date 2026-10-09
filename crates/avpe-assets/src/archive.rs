use crate::{compression, u32_at};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk { pub tag: String, pub offset: usize, pub size: usize }

impl Chunk {
    pub fn bytes<'a>(&self, source: &'a [u8]) -> Result<&'a [u8]> {
        source.get(self.offset..self.offset.checked_add(self.size).context("chunk overflow")?).context("chunk outside file")
    }
    pub fn decoded(&self, source: &[u8]) -> Result<Vec<u8>> {
        let data = self.bytes(source)?;
        if matches!(self.tag.as_str(), "DATX" | "TEMX" | "PLAX") { compression::decode(data) } else { Ok(data.to_vec()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectBlock { pub index: usize, pub offset: usize, pub chunks: Vec<Chunk> }

pub fn parse(bytes: &[u8]) -> Result<Vec<ObjectBlock>> {
    ensure!(bytes.get(..4) == Some(b"RIFF"), "not RIFF");
    ensure!(bytes.get(8..12) == Some(b"TBD2"), "not TBD2");
    ensure!(u32_at(bytes,4)? as usize + 8 == bytes.len(), "RIFF size mismatch");
    let mut result = vec![];
    let mut offset = 12;
    while offset < bytes.len() {
        ensure!(bytes.get(offset..offset+4) == Some(b"LIST"), "expected LIST at {offset}");
        let size = u32_at(bytes,offset+4)? as usize;
        let end = offset.checked_add(8 + size).context("LIST size overflow")?;
        ensure!(size >= 4 && end <= bytes.len(), "LIST outside file");
        ensure!(bytes.get(offset+8..offset+12) == Some(b"XBOX"), "unsupported platform");
        let mut cursor = offset + 12;
        let mut chunks = vec![];
        while cursor < end {
            ensure!(end - cursor >= 8, "truncated chunk header");
            let n = u32_at(bytes,cursor+4)? as usize;
            let next = cursor.checked_add(8 + n + (n&1)).context("chunk size overflow")?;
            ensure!(next <= end, "chunk exceeds LIST");
            let tag = std::str::from_utf8(&bytes[cursor..cursor+4])?.to_string();
            chunks.push(Chunk { tag, offset: cursor+8, size:n });
            cursor = next;
        }
        result.push(ObjectBlock { index:result.len(), offset, chunks });
        offset = end + (size&1);
    }
    ensure!(offset == bytes.len(), "RIFF padding exceeds file");
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeEntry { pub type_hash: u32, pub encoded_offset: u32 }

pub fn type_entries(data: &[u8]) -> Result<Vec<TypeEntry>> {
    let count = u32_at(data, 0)? as usize;
    ensure!(count.checked_mul(8).and_then(|n| n.checked_add(4)) == Some(data.len()), "TYPE table size mismatch");
    (0..count).map(|i| Ok(TypeEntry { type_hash:u32_at(data,4+i*8)?, encoded_offset:u32_at(data,8+i*8)? })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_and_bad_sizes() {
        assert!(parse(b"RIFF").is_err());
        assert!(parse(b"RIFF\x04\0\0\0TBD2").unwrap().is_empty());
        assert!(parse(b"RIFF\x05\0\0\0TBD2").is_err());
        assert!(type_entries(&[1,0,0,0]).is_err());
    }
}
