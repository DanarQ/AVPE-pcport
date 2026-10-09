use crate::{catalog::{self,Catalog}, objects::{BlockData,TEXTURE,LOGO},sha256};
use anyhow::Result;
use serde::{Deserialize,Serialize};
use std::{collections::BTreeSet,fs,path::Path};

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct VisualAsset {
    pub container:String, pub block:usize, pub object:u32, pub kind:String,
    pub name:Option<String>, pub width:u32, pub height:u32, pub image:String,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct UnsupportedVisual {pub container:String,pub block:usize,pub object:u32,pub reason:String}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct VisualReport {pub version:u32,pub assets:Vec<VisualAsset>,pub unsupported:Vec<UnsupportedVisual>}

pub fn export(catalog:&Catalog,cache:&Path)->Result<VisualReport> {
    let dest=cache.join("visuals");fs::create_dir_all(&dest)?;
    let mut report=VisualReport{version:1,assets:vec![],unsupported:vec![]};
    let mut written=BTreeSet::new();
    for container in &catalog.containers {for block in &container.blocks {
        let data=BlockData::load(cache,block)?;
        for entry in &block.types {
            if ![TEXTURE,LOGO].contains(&entry.type_hash) {continue;}
            let is_logo=entry.type_hash==LOGO;
            match if is_logo {data.logo(entry.encoded_offset)} else {data.texture(entry.encoded_offset)} {
                Ok(image)=> {
                    let hash=sha256(image.as_raw());
                    let filename=format!("visuals/{}x{}-{hash}.png",image.width(),image.height());
                    if written.insert(filename.clone()) {image.save(cache.join(&filename))?;}
                    report.assets.push(VisualAsset{container:container.source_path.clone(),block:block.index,object:entry.encoded_offset,
                        kind:if is_logo {"logo"} else {"texture"}.into(),name:data.name_at(entry.encoded_offset).map(str::to_string),
                        width:image.width(),height:image.height(),image:filename});
                }
                Err(e)=>report.unsupported.push(UnsupportedVisual{container:container.source_path.clone(),block:block.index,object:entry.encoded_offset,reason:e.to_string()}),
            }
        }
    }}
    catalog::write_json(&cache.join("visuals.json"),&report)?;
    Ok(report)
}

pub fn load(cache:&Path)->Result<VisualReport>{Ok(serde_json::from_slice(&fs::read(cache.join("visuals.json"))?)?)}
