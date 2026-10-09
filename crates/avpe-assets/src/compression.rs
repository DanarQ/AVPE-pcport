//! Word-oriented LZ decoder reconstructed from Xbox VA 0x00133340..0x001334fd.
use crate::{u16_at, u32_at};
use anyhow::{Result, ensure};

pub const MAX_DECODED_BYTES: usize = 128 * 1024 * 1024;

pub fn decode(data: &[u8]) -> Result<Vec<u8>> {
    let size = u32_at(data, 0)? as usize;
    ensure!(size <= MAX_DECODED_BYTES && size.is_multiple_of(2), "invalid decoded size {size}");
    let window = u16_at(data, 4)?;
    ensure!(window > 0 && window < 0x8000, "invalid LZ window {window}");
    let shift = window.leading_zeros();
    let mask = (1u16 << shift) - 1;
    let mut out = Vec::with_capacity(size);
    let mut cursor = 6;
    loop {
        let flags = u16_at(data, cursor)?;
        cursor += 2;
        for bit in (0..16).rev() {
            let token = u16_at(data, cursor)?;
            cursor += 2;
            if flags & (1 << bit) == 0 {
                ensure!(out.len() + 2 <= size, "literal exceeds decoded size");
                out.extend_from_slice(&token.to_le_bytes());
            } else {
                if token == 0 {
                    ensure!(out.len() == size, "decoded size mismatch: {} != {size}", out.len());
                    ensure!(data[cursor..].iter().all(|b| *b == 0), "nonzero trailing compressed data");
                    return Ok(out);
                }
                let length = usize::from(token & mask) * 2;
                let distance = usize::from(token >> shift) * 2;
                ensure!(length > 0 && distance > 0 && distance <= out.len(), "invalid back-reference");
                ensure!(out.len() + length <= size, "back-reference exceeds decoded size");
                // The source encoder bounds matches at the current output, so valid
                // streams never depend on architecture-specific overlapping rep movsd.
                ensure!(length <= distance, "overlapping back-reference is unsupported");
                let start = out.len() - distance;
                out.extend_from_within(start..start + length);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stream(size: u32, words: &[u16]) -> Vec<u8> {
        let mut b = size.to_le_bytes().to_vec();
        b.extend_from_slice(&0x07ffu16.to_le_bytes());
        for w in words { b.extend_from_slice(&w.to_le_bytes()); }
        b
    }
    #[test]
    fn literals_and_backreference() {
        assert_eq!(decode(&stream(8, &[0x3000,0x4241,0x4443,0x42,0])).unwrap(), b"ABCDABCD");
    }
    #[test]
    fn malformed_streams_fail() {
        for b in [vec![], stream(3,&[]), stream(2,&[0x8000,0]), stream(4,&[0x8000,0x21]), stream(0,&[0,1]), stream(2,&[0,0x4241])] {
            assert!(decode(&b).is_err());
        }
    }
}
