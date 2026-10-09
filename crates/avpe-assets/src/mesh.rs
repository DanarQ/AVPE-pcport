//! Xbox mesh buffers: 0x13e7d0, 0x128cb0 and 0x128e30.
//! Static frame zero only; skeleton assembly and runtime material overrides are separate.
use crate::{catalog::Catalog,document::AssetId,objects::{BlockData,MESH},u16_at,u32_at};
use anyhow::{Context,Result,ensure};
use serde::{Deserialize,Serialize};
use std::path::Path;

#[derive(Debug,Clone)]
pub struct Vertex {pub position:[f32;3],pub normal:[f32;3],pub uv:[f32;2]}
pub struct Mesh {pub vertices:Vec<Vertex>,pub indices:Vec<u32>,pub texture:Option<image::RgbaImage>,pub color:[f32;3],pub frame_count:usize}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct MeshEntry {pub id:AssetId,pub name:Option<String>,pub lod_count:usize}

fn signed(value:u32,bits:u32)->f32 {((value<<(32-bits)) as i32>>(32-bits)) as f32}
pub fn unpack(value:u32)->[f32;3] {[signed(value,11)/1023.0,signed(value>>11,11)/1023.0,signed(value>>22,10)/511.0]}
fn finite(bytes:&[u8],offset:usize)->Result<f32> {let value=f32::from_bits(u32_at(bytes,offset)?);ensure!(value.is_finite(),"non-finite mesh coordinate");Ok(value)}
pub fn triangle_strip(indices:&[u16],vertex_count:usize)->Result<Vec<u32>> {
    ensure!(indices.iter().all(|i|usize::from(*i)<vertex_count),"mesh index outside vertex buffer");
    let mut output=Vec::with_capacity(indices.len().saturating_sub(2)*3);
    for (i,tri) in indices.windows(3).enumerate() {
        if tri[0]==tri[1] || tri[1]==tri[2] || tri[0]==tri[2] {continue;}
        let [a,b,c]=[u32::from(tri[0]),u32::from(tri[1]),u32::from(tri[2])];
        if i%2==0 {output.extend([a,b,c]);} else {output.extend([b,a,c]);}
    }
    Ok(output)
}

pub fn decode(data:&BlockData,pointer:u32,lod:usize,with_texture:bool)->Result<Mesh> {
    let object=data.at(pointer)?;let lod_count=usize::from(u16_at(object,0x48)?);
    ensure!(lod<lod_count && lod_count<256,"invalid mesh LOD");
    let lods=data.at(u32_at(object,0x4c)?)?;let lod=lods.get(lod*20..lod*20+20).context("truncated LOD")?;
    let material_count=usize::from(u16_at(lod,8)?);let primitive_count=usize::from(u16_at(lod,10)?);
    ensure!(material_count==1 && (2..=1024).contains(&primitive_count),"unsupported mesh material/primitive arrangement");
    let primitives=data.at(u32_at(lod,16)?)?.get(..primitive_count*52).context("truncated mesh primitives")?;
    ensure!(u16_at(primitives,0)?==6,"unsupported mesh index topology");
    let index_count=u32_at(primitives,4)? as usize;
    ensure!(index_count<=4_000_000 && u32_at(primitives,8)? as usize==index_count*2,"invalid mesh index buffer size");
    let index_bytes=data.at(u32_at(primitives,48)?)?.get(..index_count*2).context("truncated index buffer")?;
    let indices=(0..index_count).map(|i|u16_at(index_bytes,i*2)).collect::<Result<Vec<_>>>()?;
    let mut frames=vec![];
    for primitive in primitives[52..].chunks_exact(52) {
        ensure!(u16_at(primitive,0)?==5,"unsupported mesh vertex primitive");
        let count=u32_at(primitive,4)? as usize;
        ensure!(count>0 && count<=1_000_000 && u32_at(primitive,8)? as usize==count*12,"unsupported mesh vertex stride");
        let raw=data.at(u32_at(primitive,48)?)?.get(..count*12).context("truncated vertex buffer")?;
        let scale=[finite(primitive,12)?,finite(primitive,16)?,finite(primitive,20)?];
        let mut vertices=Vec::with_capacity(count);
        for v in raw.chunks_exact(12) {
            let p=unpack(u32_at(v,0)?);let normal=unpack(u32_at(v,4)?);
            vertices.push(Vertex{position:[p[0]*scale[0],p[1]*scale[1],p[2]*scale[2]],normal,uv:[(u16_at(v,8)? as i16) as f32/2048.0,(u16_at(v,10)? as i16) as f32/2048.0]});
        }
        ensure!(indices.iter().all(|i|usize::from(*i)<count),"index outside animation vertex frame");frames.push(vertices);
    }
    let materials=data.at(u32_at(lod,12)?)?;let material_pointer=u32_at(materials,4)?;
    let material=data.at(material_pointer)?;
    let color=[finite(material,12)?,finite(material,16)?,finite(material,20)?];
    let texture=if with_texture && data.offsets.contains(&(material_pointer+0x40)) {Some(data.texture(u32_at(material,0x40)?)?)} else {None};
    if with_texture {ensure!(!data.external.contains_key(&(material_pointer+0x40)),"external mesh texture requires scope resolution");}
    let frame_count=frames.len();let vertices=frames.remove(0);let indices=triangle_strip(&indices,vertices.len())?;
    Ok(Mesh{vertices,indices,texture,color,frame_count})
}

pub fn inventory(catalog:&Catalog,cache:&Path)->Result<Vec<MeshEntry>> {
    let mut entries=vec![];
    for container in &catalog.containers {for block in &container.blocks {
        if !block.types.iter().any(|t|t.type_hash==MESH) {continue;}
        let data=BlockData::load(cache,block)?;
        for ty in &block.types {if ty.type_hash==MESH {
            entries.push(MeshEntry{id:AssetId{container:container.source_path.clone(),block:block.index,offset:ty.encoded_offset},name:data.name_at(ty.encoded_offset).map(str::to_owned),lod_count:usize::from(u16_at(data.at(ty.encoded_offset)?,0x48)?)});
        }}
    }}
    Ok(entries)
}
pub fn load(catalog:&Catalog,cache:&Path,entry:&MeshEntry)->Result<Mesh> {
    let c=catalog.containers.iter().find(|c|c.source_path==entry.id.container).context("mesh container missing")?;
    let block=c.blocks.get(entry.id.block).context("mesh block missing")?;
    decode(&BlockData::load(cache,block)?,entry.id.offset,0,true)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn packed_signed_xyz() {let p=unpack(0x7fecd401);assert!((p[0]+1.0).abs()<0.001);assert!((p[1]+0.6002).abs()<0.001);assert_eq!(p[2],1.0);}
    #[test] fn strip_parity_survives_degenerate_triangles() {assert_eq!(triangle_strip(&[0,1,2,2,3,4],5).unwrap(),vec![0,1,2,3,2,4]);assert!(triangle_strip(&[0,1,9],3).is_err());}
}
