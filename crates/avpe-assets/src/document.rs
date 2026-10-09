//! Source object properties consumed by Xbox 0x0d76e0. Retain unknown fields.
use crate::{catalog::{self,Catalog},objects::BlockData,u16_at,u32_at};
use anyhow::{Context,Result,ensure};
use serde::{Deserialize,Serialize};
use std::{collections::BTreeMap,path::Path};

pub const FILL_TYPES:[u32;3]=[0xbd148939,0xaf01b7d9,0x56a6b0fa];
#[derive(Debug,Clone,PartialEq,Eq,PartialOrd,Ord,Serialize,Deserialize)]
pub struct AssetId {pub container:String,pub block:usize,pub offset:u32}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Reference {pub field_offset:u32,pub symbol:Option<u32>,pub candidates:Vec<AssetId>,pub status:String}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Property {
    pub offset:u32,pub hash:u32,pub name:Option<String>,pub array_index:u8,pub operation:u8,
    pub bytes:Vec<u8>,pub text:Option<String>,pub references:Vec<Reference>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct SourceObject {
    pub id:AssetId,pub encoding:u32,pub class_hash:u32,pub class_name:Option<String>,pub instance_hash:u32,pub name:Option<String>,pub properties:Vec<Property>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct SourceDocument {pub version:u32,pub source_sha256:String,pub objects:Vec<SourceObject>,pub unsupported:Vec<serde_json::Value>}
type Symbols=BTreeMap<u32,Vec<AssetId>>;

fn reference(data:&BlockData,id:&AssetId,field:u32,value:u32,symbols:&Symbols)->Result<Option<Reference>> {
    if let Some(&symbol)=data.external.get(&field) {
        let candidates=symbols.get(&symbol).cloned().unwrap_or_default();
        let status=match candidates.len() {0=>"missing",1=>"unique",_=>"ambiguous-scope"}.into();
        Ok(Some(Reference{field_offset:field,symbol:Some(symbol),candidates,status}))
    } else if data.offsets.contains(&field) {
        data.at(value).with_context(||format!("dangling local reference at {field:08x}"))?;
        Ok(Some(Reference{field_offset:field,symbol:None,candidates:vec![AssetId{offset:value,..id.clone()}],status:"local".into()}))
    } else {Ok(None)}
}

fn properties(data:&BlockData,id:&AssetId,pointer:u32,names:&BTreeMap<u32,String>,symbols:&Symbols)->Result<Vec<Property>> {
    let bytes=data.at(pointer)?;let mut offset=0usize;let mut fields=vec![];
    loop {
        let hash=u32_at(bytes,offset)?;if hash==0 {break;}
        ensure!(fields.len()<65536,"too many object properties");
        let size=usize::from(u16_at(bytes,offset+4)?);
        let array_index=*bytes.get(offset+6).context("short property")?;let operation=*bytes.get(offset+7).context("short property")?;
        let payload=bytes.get(offset+8..offset+8+size).context("truncated property payload")?;
        let address=pointer.checked_add(offset as u32).context("property offset overflow")?;
        let mut references=vec![];
        for word in (0..size.saturating_sub(3)).step_by(4) {
            if let Some(r)=reference(data,id,address+8+word as u32,u32_at(payload,word)?,symbols)? {references.push(r);}
        }
        let name=names.get(&hash).cloned();
        let text=if name.as_deref()==Some("pText") && references.first().is_some_and(|r|r.status=="local") {
            let raw=data.at(u32_at(payload,0)?)?;
            let end=raw.iter().take(65536).position(|b|*b==0).context("unterminated menu text")?;
            Some(String::from_utf8_lossy(&raw[..end]).into_owned())
        } else {None};
        fields.push(Property{offset:address,hash,name,array_index,operation,bytes:payload.to_vec(),text,references});
        offset+=8+size;
    }
    Ok(fields)
}

pub fn export(catalog:&Catalog,cache:&Path,container_name:&str)->Result<SourceDocument> {
    let container=catalog.containers.iter().find(|c|c.source_path==container_name).context("document container missing")?;
    let mut symbols:Symbols=BTreeMap::new();
    // Preserve every candidate. Runtime scope/overrides are not silently guessed.
    for c in &catalog.containers {if c.source_path==container_name || matches!(c.logical_name.as_deref(),Some("MASTER.TBD"|"Master.tbd"|"CURSORS.TBD"|"Cursors.tbd")) || ["tbd/370BC447.tbx","tbd/17F20E3C.tbx"].contains(&c.source_path.as_str()) {
        for b in &c.blocks {let data=BlockData::load(cache,b)?;for (hash,offset) in data.public {symbols.entry(hash).or_default().push(AssetId{container:c.source_path.clone(),block:b.index,offset});}}
    }}
    let mut doc=SourceDocument{version:1,source_sha256:container.source_sha256.clone(),objects:vec![],unsupported:vec![]};
    for block in &container.blocks {
        let data=BlockData::load(cache,block)?;
        let mut names=catalog.names.clone();names.extend(data.labels.iter().map(|(h,n)|(*h,n.clone())));
        for entry in &block.types {if FILL_TYPES.contains(&entry.type_hash) {
            let id=AssetId{container:container_name.into(),block:block.index,offset:entry.encoded_offset};
            let result=(||->Result<SourceObject>{
                let o=data.at(entry.encoded_offset)?;let class_hash=u32_at(o,4)?;let instance_hash=u32_at(o,8)?;
                Ok(SourceObject{id:id.clone(),encoding:entry.type_hash,class_hash,class_name:names.get(&class_hash).cloned(),instance_hash,name:names.get(&instance_hash).cloned(),properties:properties(&data,&id,u32_at(o,12)?,&names,&symbols)?})
            })();
            match result {Ok(o)=>doc.objects.push(o),Err(e)=>doc.unsupported.push(serde_json::json!({"id":id,"error":format!("{e:#}")}))}
        }}
    }
    catalog::write_json(&cache.join("ui-document.json"),&doc)?;
    let screens=doc.objects.iter().filter(|o|o.class_name.as_ref().is_some_and(|n|n.contains("Menu") && !n.contains("Button"))).map(|o|serde_json::json!({"id":o.id,"class":o.class_name,"name":o.name,"instance_hash":o.instance_hash,"status":"source properties decoded; behavior unimplemented","references":o.properties.iter().filter(|p|p.name.as_ref().is_some_and(|n|n.contains("Menu"))).collect::<Vec<_>>()})).collect::<Vec<_>>();
    catalog::write_json(&cache.join("menu-inventory.json"),&serde_json::json!({"screens":screens,"runtime_transitions":"pending executable analysis","unsupported":doc.unsupported}))?;
    Ok(doc)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn dangling_references_and_short_fields_fail() {
        let id=AssetId{container:"test".into(),block:0,offset:0};
        let mut data=BlockData::default();data.regions.insert(0,vec![1,0,0,0,4,0,0,0,255,0,0,0,0,0,0,0]);data.offsets.insert(8);
        assert!(properties(&data,&id,0,&BTreeMap::new(),&Symbols::new()).is_err());
        data.offsets.clear();data.regions.get_mut(&0).unwrap().truncate(10);assert!(properties(&data,&id,0,&BTreeMap::new(),&Symbols::new()).is_err());
    }
    #[test] fn missing_external_reference_retains_identity() {
        let mut data=BlockData::default();data.external.insert(8,0x1234);
        let id=AssetId{container:"test".into(),block:0,offset:0};
        let r=reference(&data,&id,8,0,&Symbols::new()).unwrap().unwrap();assert_eq!(r.status,"missing");assert_eq!(r.symbol,Some(0x1234));
    }
}
