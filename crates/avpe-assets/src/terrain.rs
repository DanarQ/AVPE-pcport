//! CRendMap geometry, reconstructed from Xbox 0x131ef0, 0x125e70,
//! 0x13d4d0 and 0x129900. Material layers remain explicit raw source words.
use crate::{catalog::Catalog, objects::{BlockData,MAP},u16_at,u32_at};
use anyhow::{Context,Result,ensure};
use serde::{Deserialize,Serialize};
use std::path::Path;

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct TerrainCell {pub flags:u8,pub overlay:u8,pub height:f32,pub layers:[u32;2]}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Terrain {
    pub container:String,pub block:usize,pub object:u32,
    pub width:u32,pub height:u32,pub origin:[f32;2],pub spacing:f32,
    pub cells:Vec<TerrainCell>,
}
impl Terrain {
    pub fn decode(data:&BlockData,pointer:u32)->Result<Self> {
        let object=data.at(pointer)?;
        let width=u32::from(u16_at(object,0x1c)?);let height=u32::from(u16_at(object,0x1e)?);
        let stride=usize::from(u16_at(object,0x20)?);
        let spacing=f32::from(u16_at(object,0x26)?);
        let origin=[f32::from_bits(u32_at(object,0x10)?),f32::from_bits(u32_at(object,0x14)?)];
        ensure!((2..=2048).contains(&width) && (2..=2048).contains(&height),"invalid terrain dimensions");
        ensure!(stride==16,"unsupported terrain cell stride {stride}");
        ensure!(spacing>0.0 && origin.iter().all(|n|n.is_finite()),"invalid terrain coordinate system");
        ensure!(u32_at(object,0x2c)?==width*stride as u32,"inconsistent terrain row stride");
        let bytes=data.at(u32_at(object,0x38)?)?.get(..width as usize*height as usize*stride).context("truncated terrain cells")?;
        let source_diagonals=u32_at(object,0x0c)?&1!=0;
        let mut cells=Vec::with_capacity((width*height) as usize);
        for (i,cell) in bytes.chunks_exact(stride).enumerate() {
            let mut flags=cell[0];
            if !source_diagonals && ((i as u32%width)^(i as u32/width))&1!=0 {flags|=8;}
            cells.push(TerrainCell{flags,overlay:cell[1],height:(u16_at(cell,2)? as i16) as f32/16.0,layers:[u32_at(cell,4)?,u32_at(cell,8)?]});
        }
        Ok(Self{container:String::new(),block:0,object:pointer,width,height,origin,spacing,cells})
    }
    pub fn position(&self,x:u32,y:u32)->[f32;3] {
        [self.origin[0]+x as f32*self.spacing,self.origin[1]+y as f32*self.spacing,self.cells[(y*self.width+x) as usize].height]
    }
    pub fn indices(&self)->Vec<u32> {
        let mut indices=Vec::with_capacity(((self.width-1)*(self.height-1)*6) as usize);
        for y in 0..self.height-1 {for x in 0..self.width-1 {
            let a=y*self.width+x;let b=a+1;let c=a+self.width;let d=c+1;
            if self.cells[a as usize].layers[0]==0 {continue;}
            if self.cells[a as usize].flags&8==0 {indices.extend([a,b,c,b,d,c]);} else {indices.extend([a,b,d,a,d,c]);}
        }}
        indices
    }
}

pub fn load(catalog:&Catalog,cache:&Path,scene_id:&str)->Result<Vec<Terrain>> {
    let scene=catalog.scenes.iter().find(|s|s.id.eq_ignore_ascii_case(scene_id)).context("scene not found")?;
    let container=catalog.containers.iter().find(|c|c.source_path==scene.background).context("scene container missing")?;
    let mut maps=vec![];
    for block in &container.blocks {for ty in &block.types {if ty.type_hash==MAP {maps.push((block,ty));}}}
    ensure!(!maps.is_empty(),"scene has no terrain");
    maps.into_iter().map(|(block,ty)| {
        let data=BlockData::load(cache,block)?;
        let mut terrain=Terrain::decode(&data,ty.encoded_offset)?;
        terrain.container=container.source_path.clone();terrain.block=block.index;
        Ok(terrain)
    }).collect()
}

#[cfg(test)] mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn fixture()->BlockData {
        let mut bytes=vec![0;0x80+64];
        for (o,v) in [(0x0c,1u32),(0x1c,0x00020002),(0x20,16),(0x24,0x10000),(0x2c,32),(0x38,0x80)] {bytes[o..o+4].copy_from_slice(&v.to_le_bytes());}
        for i in 0..4 {bytes[0x80+i*16+4]=1;}
        bytes[0x82..0x84].copy_from_slice(&(-32i16).to_le_bytes());
        BlockData{regions:BTreeMap::from([(0,bytes)]),..Default::default()}
    }
    #[test] fn signed_height_and_source_diagonals() {
        let mut data=fixture();let a=Terrain::decode(&data,0).unwrap();assert_eq!(a.cells[0].height,-2.0);assert_eq!(a.indices(),vec![0,1,2,1,3,2]);
        data.regions.get_mut(&0).unwrap()[0x80]=8;assert_eq!(Terrain::decode(&data,0).unwrap().indices(),vec![0,1,3,0,3,2]);
    }
    #[test] fn corrupt_terrain_is_rejected() {
        let mut data=fixture();data.regions.get_mut(&0).unwrap().truncate(140);assert!(Terrain::decode(&data,0).is_err());
        let mut data=fixture();data.regions.get_mut(&0).unwrap()[0x2c]=1;assert!(Terrain::decode(&data,0).is_err());
    }
}
