use crate::{FORMAT_VERSION, SOURCE_XBE_SHA256, archive, hash_file, name_hash, string_hash, safe_relative, sha256, xbe::XbeInfo};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::{Path,PathBuf}};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceFile { pub path:String, pub bytes:u64, pub sha256:String }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceManifest { pub version:u32, pub files:Vec<SourceFile> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedChunk { pub tag:String, pub file_offset:usize, pub original_bytes:usize, pub decoded_bytes:usize, pub sha256:String, pub cache_path:String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetBlock { pub index:usize, pub chunks:Vec<DecodedChunk>, pub types:Vec<archive::TypeEntry> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetContainer { pub source_path:String, pub source_sha256:String, pub logical_name:Option<String>, pub blocks:Vec<AssetBlock> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAsset { pub id:String, pub background:String, pub loading:Option<String>, pub semantic_status:String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiDocument { pub source_path:String, pub discovered_type_names:Vec<String>, pub semantic_status:String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog { pub version:u32, pub source:SourceManifest, pub containers:Vec<AssetContainer>, pub scenes:Vec<SceneAsset>, pub ui:UiDocument, pub names:BTreeMap<u32,String> }

fn files_in(root:&Path, dir:&Path, out:&mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let e=entry?;
        let ty=e.file_type()?;
        ensure!(!ty.is_symlink(), "source symlink not supported: {}", e.path().display());
        if ty.is_dir() { files_in(root,&e.path(),out)?; }
        else if ty.is_file() { out.push(e.path().strip_prefix(root)?.to_path_buf()); }
    }
    Ok(())
}

pub fn inventory(root:&Path) -> Result<SourceManifest> {
    let mut paths=vec![]; files_in(root,root,&mut paths)?; paths.sort();
    let files=paths.into_iter().map(|p| {
        let absolute=root.join(&p);
        Ok(SourceFile{path:p.to_string_lossy().replace('\\',"/"),bytes:fs::metadata(&absolute)?.len(),sha256:hash_file(&absolute)?})
    }).collect::<Result<Vec<_>>>()?;
    Ok(SourceManifest{version:FORMAT_VERSION,files})
}

pub fn write_json(path:&Path, value:&impl Serialize) -> Result<()> {
    if let Some(parent)=path.parent() { fs::create_dir_all(parent)?; }
    fs::write(path,serde_json::to_vec_pretty(value)?)?; Ok(())
}

pub fn import(source:&Path, cache:&Path) -> Result<Catalog> {
    let source=source.canonicalize()?;
    fs::create_dir_all(cache)?;
    let cache=cache.canonicalize()?;
    ensure!(!cache.starts_with(&source) && !source.starts_with(&cache), "cache and source directories must be separate");
    let manifest=inventory(&source)?;
    let xbe=XbeInfo::parse(&fs::read(source.join("default.xbe"))?)?;
    ensure!(xbe.sha256==SOURCE_XBE_SHA256, "unsupported XBE build; expected {SOURCE_XBE_SHA256}, found {}",xbe.sha256);
    write_json(&cache.join("source-manifest.json"),&manifest)?;
    write_json(&cache.join("xbe.json"),&xbe)?;
    let mut names=BTreeMap::new();
    for s in &xbe.strings {
        // Type registration names and logical source names retain their provenance in xbe.json.
        names.entry(string_hash(&s.text)).or_insert_with(|| s.text.clone());
        if s.text.to_ascii_lowercase().ends_with(".tbd") {
            let basename=s.text.rsplit(['/', '\\']).next().unwrap_or(&s.text);
            names.insert(name_hash(basename),basename.to_string());
        }
    }
    let mut containers=vec![];
    for f in &manifest.files {
        if !f.path.to_ascii_lowercase().ends_with(".tbx") { continue; }
        let bytes=fs::read(source.join(safe_relative(&f.path)?))?;
        let mut blocks=vec![];
        for block in archive::parse(&bytes).with_context(|| f.path.clone())? {
            let mut chunks=vec![]; let mut types=vec![];
            for chunk in block.chunks {
                let decoded=chunk.decoded(&bytes).with_context(|| format!("{} block {} {} @ {:x}",f.path,block.index,chunk.tag,chunk.offset))?;
                if chunk.tag=="TYPE" { types=archive::type_entries(&decoded)?; }
                let hash=sha256(&decoded);
                let path=format!("blobs/{hash}.bin");
                let dest=cache.join(&path);
                fs::create_dir_all(cache.join("blobs"))?;
                if !dest.exists() || hash_file(&dest)?!=hash { fs::write(dest,&decoded)?; }
                chunks.push(DecodedChunk{tag:chunk.tag,file_offset:chunk.offset,original_bytes:chunk.size,decoded_bytes:decoded.len(),sha256:hash,cache_path:path});
            }
            blocks.push(AssetBlock{index:block.index,chunks,types});
        }
        let stem=Path::new(&f.path).file_stem().context("missing stem")?.to_string_lossy();
        let logical_name=u32::from_str_radix(&stem,16).ok().and_then(|h| names.get(&h).cloned());
        containers.push(AssetContainer{source_path:f.path.clone(),source_sha256:f.sha256.clone(),logical_name,blocks});
    }
    let mut scenes=vec![];
    for c in &containers {
        if c.source_path.ends_with("/FD090663.tbx") {
            let parent=Path::new(&c.source_path).parent().context("scene parent missing")?;
            let id=parent.file_name().context("scene ID missing")?.to_string_lossy().into_owned();
            let loading=format!("{}/06F4F187.tbx",parent.to_string_lossy().replace('\\',"/"));
            let loading=containers.iter().any(|c| c.source_path==loading).then_some(loading);
            scenes.push(SceneAsset{id,background:c.source_path.clone(),loading,semantic_status:"container-decoded; scene semantics pending".into()});
        }
    }
    let ui=UiDocument{source_path:"tbd/B073ED74.tbx".into(),discovered_type_names:xbe.strings.iter().filter(|s| s.text.starts_with('G') && (s.text.contains("Menu")||s.text.contains("Loading")) && !s.text.contains(' ')).map(|s|s.text.clone()).collect(),semantic_status:"type names discovered; layout and transitions pending".into()};
    let catalog=Catalog{version:FORMAT_VERSION,source:manifest,containers,scenes,ui,names};
    write_json(&cache.join("catalog.json"),&catalog)?;
    Ok(catalog)
}

pub fn load(cache:&Path) -> Result<Catalog> {
    let catalog:Catalog=serde_json::from_slice(&fs::read(cache.join("catalog.json"))?)?;
    ensure!(catalog.version==FORMAT_VERSION, "stale cache version; reimport source");
    Ok(catalog)
}

pub fn verify(source:&Path, cache:&Path) -> Result<Catalog> {
    let catalog=load(cache)?;
    ensure!(inventory(source)?==catalog.source, "source changed; cache is stale");
    for c in &catalog.containers {
        for b in &c.blocks { for chunk in &b.chunks {
            let path=cache.join(safe_relative(&chunk.cache_path)?);
            ensure!(fs::metadata(&path)?.len()==chunk.decoded_bytes as u64 && hash_file(&path)?==chunk.sha256, "cache blob missing or corrupt: {}",chunk.cache_path);
        }}
    }
    Ok(catalog)
}
