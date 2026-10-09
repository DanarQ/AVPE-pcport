use crate::{sha256, u16_at, u32_at};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section { pub name:String, pub flags:u32, pub virtual_address:u32, pub virtual_size:u32, pub file_offset:u32, pub file_size:u32 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library { pub name:String, pub major:u16, pub minor:u16, pub build:u16 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceString { pub address:u32, pub file_offset:usize, pub text:String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XbeInfo { pub sha256:String, pub title:String, pub title_id:u32, pub base_address:u32, pub entry_point:u32, pub sections:Vec<Section>, pub libraries:Vec<Library>, pub strings:Vec<SourceString> }

fn c_string(data:&[u8], offset:usize) -> Result<String> {
    let tail = data.get(offset..).context("string outside XBE")?;
    let end = tail.iter().position(|b| *b == 0).context("unterminated string")?;
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}

impl XbeInfo {
    pub fn parse(data:&[u8]) -> Result<Self> {
        ensure!(data.get(..4) == Some(b"XBEH") && data.len() >= 0x178, "invalid XBE header");
        let base = u32_at(data,0x104)?;
        let headers = u32_at(data,0x108)? as usize;
        ensure!(headers <= data.len(), "headers exceed file");
        let header_offset = |va:u32| -> Result<usize> {
            let o = va.checked_sub(base).context("address below XBE base")? as usize;
            ensure!(o < headers, "address outside XBE headers"); Ok(o)
        };
        let cert = header_offset(u32_at(data,0x118)?)?;
        let title_bytes = data.get(cert+12..cert+92).context("truncated certificate")?;
        let title = String::from_utf16_lossy(&title_bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0],b[1]])).take_while(|c| *c != 0).collect::<Vec<_>>());
        let start = header_offset(u32_at(data,0x120)?)?;
        let count = u32_at(data,0x11c)? as usize;
        ensure!(count <= 4096 && start + count*56 <= headers, "invalid section table");
        let mut sections=vec![];
        for i in 0..count {
            let o=start+i*56;
            let s=Section { name:c_string(data,header_offset(u32_at(data,o+20)?)?)?, flags:u32_at(data,o)?, virtual_address:u32_at(data,o+4)?, virtual_size:u32_at(data,o+8)?, file_offset:u32_at(data,o+12)?, file_size:u32_at(data,o+16)? };
            ensure!(u64::from(s.file_offset)+u64::from(s.file_size) <= data.len() as u64, "section outside XBE");
            ensure!(s.file_size <= s.virtual_size, "section raw size exceeds virtual size");
            sections.push(s);
        }
        let encoded=u32_at(data,0x128)?;
        let entry_point=[encoded^0xa8fc57ab,encoded^0x94859d4b].into_iter().find(|v| sections.iter().any(|s| s.flags&4 != 0 && *v>=s.virtual_address && u64::from(*v)<u64::from(s.virtual_address)+u64::from(s.virtual_size))).context("cannot decode XBE entry point")?;
        let count=u32_at(data,0x160)? as usize;
        ensure!(count<=4096, "invalid library count");
        let mut libraries=vec![];
        if count>0 {
            let start=header_offset(u32_at(data,0x164)?)?;
            ensure!(start+count*16<=headers, "library table exceeds headers");
            for i in 0..count {
                let o=start+i*16;
                libraries.push(Library{name:String::from_utf8_lossy(&data[o..o+8]).trim_end_matches('\0').into(),major:u16_at(data,o+8)?,minor:u16_at(data,o+10)?,build:u16_at(data,o+12)?});
            }
        }
        let mut strings=vec![];
        for s in &sections {
            if !matches!(s.name.as_str(), ".rdata"|".data") { continue; }
            let raw=s.file_offset as usize;
            let region=&data[raw..raw+s.file_size as usize];
            let mut i=0;
            while i<region.len() {
                let start=i;
                while i<region.len() && (32..=126).contains(&region[i]) { i+=1; }
                if i-start>=4 && region.get(i)==Some(&0) {
                    strings.push(SourceString{address:s.virtual_address+start as u32,file_offset:raw+start,text:String::from_utf8_lossy(&region[start..i]).into()});
                }
                i+=1;
            }
        }
        Ok(Self{sha256:sha256(data),title,title_id:u32_at(data,cert+8)?,base_address:base,entry_point,sections,libraries,strings})
    }
    pub fn va_to_offset(&self, address:u32) -> Result<usize> {
        for s in &self.sections {
            if let Some(delta)=address.checked_sub(s.virtual_address) {
                if delta<s.file_size { return Ok((s.file_offset+delta) as usize); }
            }
        }
        anyhow::bail!("unbacked XBE address {address:08x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn truncated_xbe_is_rejected() { assert!(XbeInfo::parse(b"XBEH").is_err()); }
}
