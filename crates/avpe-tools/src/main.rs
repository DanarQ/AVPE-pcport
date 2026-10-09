use anyhow::Result;
use avpe_assets::{catalog, xbe::XbeInfo};
use clap::{Parser,Subcommand};
use std::{fs,path::PathBuf};

#[derive(Parser)]
#[command(about="Xbox AVP: Extinction asset research and validation")]
struct Args {
    #[arg(long,global=true,default_value="Aliens Versus Predator Extinction")] source:PathBuf,
    #[arg(long,global=true,default_value="local/cache")] cache:PathBuf,
    #[command(subcommand)] command:Command,
}
#[derive(Subcommand)]
enum Command { Inventory, InspectXbe, Import, Verify, ExportVisuals, VerifyScenes, InspectUi, VerifyMeshes }

fn main()->Result<()> {
    let a=Args::parse();
    match a.command {
        Command::VerifyMeshes=> {
            let catalog=catalog::load(&a.cache)?;let entries=avpe_assets::mesh::inventory(&catalog,&a.cache)?;
            catalog::write_json(&a.cache.join("meshes.json"),&entries)?;
            let mut checked=0;let mut unsupported=vec![];
            for c in &catalog.containers {for b in &c.blocks {
                if !b.types.iter().any(|t|t.type_hash==avpe_assets::objects::MESH) {continue;}
                let data=avpe_assets::objects::BlockData::load(&a.cache,b)?;
                for ty in &b.types {if ty.type_hash==avpe_assets::objects::MESH {
                    let lod_count=avpe_assets::u16_at(data.at(ty.encoded_offset)?,0x48)?;
                    for lod in 0..usize::from(lod_count) {match avpe_assets::mesh::decode(&data,ty.encoded_offset,lod,false) {
                        Ok(_)=>checked+=1,Err(e)=>unsupported.push(serde_json::json!({"container":c.source_path,"block":b.index,"offset":ty.encoded_offset,"lod":lod,"error":format!("{e:#}")})),
                    }}
                }}
            }}
            catalog::write_json(&a.cache.join("mesh-report.json"),&serde_json::json!({"objects":entries.len(),"checked_lods":checked,"unsupported":unsupported,"status":"static geometry decoded; skeleton, animation, material overrides and scene placement pending"}))?;
            println!("{} mesh objects; {} LODs decoded; {} unsupported",entries.len(),checked,unsupported.len());
        }
        Command::InspectUi=> {
            let catalog=catalog::load(&a.cache)?;let doc=avpe_assets::document::export(&catalog,&a.cache,"tbd/B073ED74.tbx")?;
            println!("{} source objects; {} unsupported; see ui-document.json and menu-inventory.json",doc.objects.len(),doc.unsupported.len());
        }
        Command::VerifyScenes=>{
            let catalog=catalog::load(&a.cache)?;
            let mut scenes=vec![];let mut failures=0;
            for scene in &catalog.scenes {
                let record=match avpe_assets::terrain::load(&catalog,&a.cache,&scene.id) {
                    Ok(terrains)=>serde_json::json!({"id":scene.id,"terrains":terrains.iter().map(|t|serde_json::json!({"width":t.width,"height":t.height,"vertices":t.cells.len(),"triangles":t.indices().len()/3,"container":t.container,"block":t.block,"object":t.object})).collect::<Vec<_>>(),"status":"geometry decoded; materials and instances pending"}),
                    Err(e)=>{failures+=1;serde_json::json!({"id":scene.id,"error":format!("{e:#}")})}
                };scenes.push(record);
            }
            catalog::write_json(&a.cache.join("scene-report.json"),&scenes)?;
            println!("{} scenes checked; {} failures",scenes.len(),failures);
            anyhow::ensure!(failures==0,"terrain verification failed; see scene-report.json");
        }
        Command::ExportVisuals=>{
            let c=catalog::load(&a.cache)?;
            let r=avpe_assets::visuals::export(&c,&a.cache)?;
            println!("{} visual assets exported; {} unsupported",r.assets.len(),r.unsupported.len());
        }
        Command::Inventory=> {
            let m=catalog::inventory(&a.source)?;
            catalog::write_json(&a.cache.join("source-manifest.json"),&m)?;
            println!("{} files, {} bytes",m.files.len(),m.files.iter().map(|f|f.bytes).sum::<u64>());
        }
        Command::InspectXbe=> {
            let x=XbeInfo::parse(&fs::read(a.source.join("default.xbe"))?)?;
            catalog::write_json(&a.cache.join("xbe.json"),&x)?;
            println!("{} | title ID {:08x} | entry {:08x} | {} sections | {} strings",x.title,x.title_id,x.entry_point,x.sections.len(),x.strings.len());
        }
        Command::Import|Command::Verify=> {
            let c=if matches!(a.command,Command::Import) {catalog::import(&a.source,&a.cache)?} else {catalog::verify(&a.source,&a.cache)?};
            println!("{} containers, {} blocks, {} scenes; semantic rendering not yet verified",c.containers.len(),c.containers.iter().map(|a|a.blocks.len()).sum::<usize>(),c.scenes.len());
        }
    }
    Ok(())
}
