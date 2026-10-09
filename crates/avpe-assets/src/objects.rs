//! Relocatable object views. Guest offsets are never cast to host pointers.
use crate::{catalog::AssetBlock, safe_relative, u16_at, u32_at};
use anyhow::{Context, Result, ensure};
use image::{Rgba,RgbaImage};
use std::{collections::{BTreeMap,BTreeSet},fs,path::Path};

pub const TEXTURE:u32=0xc47ce8a6;
pub const LOGO:u32=0x4376c04a;
pub const LABEL:u32=0xbbdc6c7d;
pub const MESH:u32=0x68a8b2db;
pub const MAP:u32=0x175b1fde;

#[derive(Default)]
pub struct BlockData { pub regions:BTreeMap<u32,Vec<u8>>, pub labels:BTreeMap<u32,String>, pub public:BTreeMap<u32,u32>,pub offsets:BTreeSet<u32>,pub external:BTreeMap<u32,u32> }

impl BlockData {
    pub fn load(cache:&Path, block:&AssetBlock)->Result<Self> {
        let mut regions=BTreeMap::new(); let mut public=BTreeMap::new();let mut offsets=BTreeSet::new();let mut external=BTreeMap::new();
        for c in &block.chunks {
            let region=match c.tag.as_str() { "DATA"|"DATX"=>Some(0),"TEMP"|"TEMX"=>Some(1),"EXTA"=>Some(2),"PLAT"|"PLAX"=>Some(3),_=>None };
            if let Some(r)=region { regions.insert(r,fs::read(cache.join(safe_relative(&c.cache_path)?))?); }
            if c.tag=="PUBL" {
                let b=fs::read(cache.join(safe_relative(&c.cache_path)?))?;
                let count=u32_at(&b,0)? as usize;
                ensure!(count.checked_mul(8).and_then(|v|v.checked_add(4))==Some(b.len()), "invalid PUBL size");
                for i in 0..count {public.insert(u32_at(&b,4+i*8)?,u32_at(&b,8+i*8)?);}
            }
            if c.tag=="OFFS" || c.tag=="EXTN" {
                let b=fs::read(cache.join(safe_relative(&c.cache_path)?))?;
                let count=u32_at(&b,0)? as usize;let stride=if c.tag=="OFFS" {4} else {8};
                ensure!(count.checked_mul(stride).and_then(|n|n.checked_add(4))==Some(b.len()),"invalid {} size",c.tag);
                for i in 0..count {
                    if c.tag=="OFFS" {offsets.insert(u32_at(&b,4+i*4)?);} else {external.insert(u32_at(&b,8+i*8)?,u32_at(&b,4+i*8)?);}
                }
            }
        }
        let mut result=Self{regions,labels:BTreeMap::new(),public,offsets,external};
        for &field in result.offsets.iter().chain(result.external.keys()) {u32_at(result.at(field)?,0).with_context(||format!("reference field {field:08x} outside object regions"))?;}
        for t in &block.types {
            if t.type_hash==LABEL {
                let bytes=result.at(t.encoded_offset)?;
                let hash=u32_at(bytes,4)?;
                let text=bytes.get(8..).context("short label")?;
                let end=text.iter().position(|b|*b==0).context("unterminated label")?;
                result.labels.insert(hash,String::from_utf8_lossy(&text[..end]).into());
            }
        }
        Ok(result)
    }
    pub fn at(&self, pointer:u32)->Result<&[u8]> {
        let region=pointer>>28;
        let offset=(pointer&0x0fff_ffff) as usize;
        self.regions.get(&region).context("missing pointer region")?.get(offset..).context("pointer outside region")
    }
    pub fn name_at(&self, pointer:u32)->Option<&str> {
        self.public.iter().find(|(_,p)| **p==pointer).and_then(|(h,_)|self.labels.get(h)).map(String::as_str)
    }
    pub fn texture(&self, pointer:u32)->Result<RgbaImage> {
        self.texture_inner(pointer,0)
    }
    fn texture_inner(&self, pointer:u32,depth:u8)->Result<RgbaImage> {
        ensure!(depth<16,"cyclic texture alias");
        let object=self.at(pointer)?;
        if u32_at(object,8)?!=0 {return self.texture_inner(u32_at(object,4)?,depth+1);}
        let bitmap=self.at(u32_at(object,4)?)?;
        let width=u16_at(bitmap,0)? as u32; let height=u16_at(bitmap,2)? as u32;
        let bits=u16_at(bitmap,4)?; let mip_levels=u16_at(bitmap,6)?; let flags=u32_at(bitmap,8)?;
        ensure!(width>0 && height>0 && width<=4096 && height<=4096 && width.is_power_of_two() && height.is_power_of_two(), "unsupported texture dimensions");
        // Xbox 0x137e00 passes header+6 as the D3D mip level count, not a pixel format.
        ensure!(bits==32 && (1..=13).contains(&mip_levels), "unsupported texture encoding {bits}/{mip_levels}");
        let pixels=bitmap.get(20..20+(width*height*4) as usize).context("truncated bitmap")?;
        let mut out=RgbaImage::new(width,height);
        for y in 0..height {for x in 0..width {
            let p=morton_index(x,y,width,height)*4;
            let alpha=if flags&6!=0 {pixels[p+3]} else {255};
            out.put_pixel(x,y,Rgba([pixels[p+2],pixels[p+1],pixels[p],alpha]));
        }}
        Ok(out)
    }
    /// CLogo serialized layout: flags, u16 texture/quad counts, two local pointers.
    /// Quad coordinates are signed 12.4 fixed point; UVs use a 4096 unit scale.
    pub fn logo(&self, pointer:u32)->Result<RgbaImage> {
        let logo=self.at(pointer)?;
        let texture_count=u16_at(logo,8)? as usize; let quad_count=u16_at(logo,10)? as usize;
        ensure!(texture_count<=4096 && quad_count<=4096, "invalid logo counts");
        let texture_ptrs=self.at(u32_at(logo,12)?)?; let quads=self.at(u32_at(logo,16)?)?;
        ensure!(quads.len()>=quad_count*20 && texture_ptrs.len()>=texture_count*4,"truncated logo");
        let textures=(0..texture_count).map(|i|self.texture(u32_at(texture_ptrs,i*4)?)).collect::<Result<Vec<_>>>()?;
        let mut out=RgbaImage::new(640,480);
        for q in 0..quad_count {
            let data=&quads[q*20..q*20+20];
            let index=u16_at(data,2)? as usize; let tex=textures.get(index).context("logo texture index out of range")?;
            let f=|o|->Result<f32>{Ok(u16_at(data,o)? as i16 as f32/16.0)};
            let (x0,x1,y0,y1)=(f(4)?,f(6)?,f(8)?,f(10)?);
            ensure!(x1>x0 && y1>y0, "invalid logo rectangle");
            let (u0,u1,v0,v1)=(f(12)?/256.0,f(14)?/256.0,f(16)?/256.0,f(18)?/256.0);
            for y in (y0.floor().max(0.0) as u32)..(y1.ceil().min(480.0) as u32) {
                for x in (x0.floor().max(0.0) as u32)..(x1.ceil().min(640.0) as u32) {
                    let u=u0+(u1-u0)*((x as f32+0.5-x0)/(x1-x0));
                    let v=v0+(v1-v0)*((y as f32+0.5-y0)/(y1-y0));
                    let sx=(u*tex.width() as f32).clamp(0.0,tex.width() as f32-1.0) as u32;
                    let sy=(v*tex.height() as f32).clamp(0.0,tex.height() as f32-1.0) as u32;
                    out.put_pixel(x,y,*tex.get_pixel(sx,sy));
                }
            }
        }
        Ok(out)
    }
}

pub fn morton_index(x:u32,y:u32,width:u32,height:u32)->usize {
    let mut result=0; let mut target_bit=0; let mut bit=1;
    while bit<width || bit<height {
        if bit<width {if x&bit!=0 {result|=1<<target_bit;} target_bit+=1;}
        if bit<height {if y&bit!=0 {result|=1<<target_bit;} target_bit+=1;}
        bit<<=1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn non_square_swizzle_is_bijective() {
        let set=(0..4).flat_map(|y|(0..8).map(move |x|morton_index(x,y,8,4))).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(set,(0..32).collect());
    }
    #[test] fn pointers_are_bounds_checked() {
        let b=BlockData{regions:BTreeMap::from([(0,vec![0;12])]),..Default::default()};
        assert!(b.at(13).is_err()); assert!(b.at(0x40000000).is_err());
    }
}
