mod graphics;
mod terrain_view;

use anyhow::{Context, Result, ensure};
use avpe_assets::{catalog::{self, Catalog}, visuals::{self, VisualReport}, hash_file, safe_relative, SOURCE_XBE_SHA256};
use clap::Parser;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use winit::{application::ApplicationHandler, event::{ElementState, WindowEvent}, event_loop::{ActiveEventLoop, EventLoop}, keyboard::{Key, NamedKey}, window::{Window, WindowId}};

#[derive(Parser)]
#[command(about="Native AVPE asset inspection runtime — reconstruction in progress")]
struct Args {
    #[arg(long, default_value="Aliens Versus Predator Extinction")] source: PathBuf,
    #[arg(long, default_value="local/cache")] cache: PathBuf,
    #[arg(long)] inspect: bool,
    #[arg(long, default_value="M01")] scene: String,
    #[arg(long)] capture: Option<PathBuf>,
    #[arg(long)] smoke_frames: Option<u64>,
    #[arg(long, default_value_t=1280)] width: u32,
    #[arg(long, default_value_t=900)] height: u32,
    #[arg(long)] terrain: bool,
    #[arg(long)] mesh: Option<String>,
}

struct App {
    args: Args, catalog: Catalog, visuals: VisualReport, graphics: Option<graphics::Graphics>,
    selected: usize, filter: String, textures: HashMap<String,egui::TextureHandle>,
    inspect: bool, frames: u64, failure: Option<String>, error: Option<String>,
    terrains:Vec<avpe_assets::terrain::Terrain>, terrain_index:usize, terrain_key:Option<(String,usize)>,
    meshes:Vec<avpe_assets::mesh::MeshEntry>,mesh_index:usize,mesh_key:Option<usize>,view_mode:u8,
}

impl App {
    fn new(args: Args) -> Result<Self> {
        let catalog = catalog::load(&args.cache).context("Import source first with avpe-tools import")?;
        let visuals = visuals::load(&args.cache).context("Run avpe-tools export-visuals first")?;
        ensure!(!visuals.assets.is_empty(), "visual catalog is empty");
        ensure!(hash_file(&args.source.join("default.xbe"))? == SOURCE_XBE_SHA256, "source XBE does not match the supported build");
        let scene = catalog.scenes.iter().find(|s|s.id.eq_ignore_ascii_case(&args.scene)).context("unknown scene")?;
        let selected = visuals.assets.iter().position(|a|a.kind=="logo" && scene.loading.as_ref()==Some(&a.container)).unwrap_or(0);
        let inspect = args.inspect;
        let mesh_path=args.cache.join("meshes.json");
        let meshes:Vec<avpe_assets::mesh::MeshEntry>=if mesh_path.exists() {serde_json::from_slice(&std::fs::read(mesh_path)?)?} else {vec![]};
        let mesh_index=if let Some(name)=&args.mesh {meshes.iter().position(|m|m.name.as_ref().is_some_and(|n|n.eq_ignore_ascii_case(name))).context("mesh name not found; run verify-meshes to refresh the index")?} else {0};
        let view_mode=if args.mesh.is_some() {2} else if args.terrain {1} else {0};
        Ok(Self { args,catalog,visuals,graphics:None,selected,filter:String::new(),textures:HashMap::new(),inspect,frames:0,failure:None,error:None,terrains:vec![],terrain_index:0,terrain_key:None,meshes,mesh_index,mesh_key:None,view_mode })
    }
    fn ui(&mut self, root:&mut egui::Ui,graphics:&mut graphics::Graphics) {
        let ctx=root.ctx().clone();
        let previous_mode=self.view_mode;
        egui::Panel::top("status").show_inside(root,|ui| {
            ui.horizontal(|ui| {
                ui.strong("AVPE · Xbox asset inspection");
                ui.separator();
                ui.label("Original frontend and gameplay are not implemented");
                ui.checkbox(&mut self.inspect,"Inspector (F1)");
                ui.selectable_value(&mut self.view_mode,0,"Images");
                ui.selectable_value(&mut self.view_mode,1,"Terrain");
                ui.add_enabled_ui(!self.meshes.is_empty(),|ui|{ui.selectable_value(&mut self.view_mode,2,"Meshes");});
            });
        });
        if self.view_mode!=previous_mode {self.terrain_key=None;self.mesh_key=None;}
        if self.inspect {
            egui::Panel::left("assets").default_size(300.0).show_inside(root,|ui| {
                ui.heading("Original assets");
                egui::ComboBox::from_label("Loading image").selected_text(&self.args.scene).show_ui(ui,|ui| {
                    for scene in &self.catalog.scenes {
                        if ui.selectable_value(&mut self.args.scene,scene.id.clone(),&scene.id).changed()
                            && let Some(index)=self.visuals.assets.iter().position(|a|a.kind=="logo" && scene.loading.as_ref()==Some(&a.container)) { self.selected=index; }
                    }
                });
                if self.view_mode==1 {
                    ui.label("Geometry inspection: diagnostic colors and free camera. Source materials, objects and camera are pending.");
                    egui::ComboBox::from_label("Terrain object").selected_text(format!("{} / {}",self.terrain_index+1,self.terrains.len())).show_ui(ui,|ui| {
                        for (index,terrain) in self.terrains.iter().enumerate() {ui.selectable_value(&mut self.terrain_index,index,format!("{} × {} · block {}",terrain.width,terrain.height,terrain.block));}
                    });
                    ui.label("Left drag: orbit\nRight drag: pan\nWheel: zoom");
                }
                if self.view_mode==2 {ui.label("Static mesh inspection. Skeleton assembly, animations and original lighting are pending.");}
                ui.text_edit_singleline(&mut self.filter);
                ui.label(format!("{} assets · {} unsupported",self.visuals.assets.len(),self.visuals.unsupported.len()));
                let filter=self.filter.to_ascii_lowercase();
                egui::ScrollArea::vertical().show(ui,|ui| {
                    if self.view_mode==2 {
                        for (i,asset) in self.meshes.iter().enumerate() {
                            let label=format!("{} · {} / {}:{:x}",asset.name.as_deref().unwrap_or("unnamed mesh"),asset.id.container,asset.id.block,asset.id.offset);
                            if filter.is_empty() || label.to_ascii_lowercase().contains(&filter) {ui.selectable_value(&mut self.mesh_index,i,label);}
                        }
                        return;
                    }
                    for (i,asset) in self.visuals.assets.iter().enumerate() {
                        let label=format!("{} · {} / {}:{:x}",asset.name.as_deref().unwrap_or(&asset.kind),asset.container,asset.block,asset.object);
                        if filter.is_empty() || label.to_ascii_lowercase().contains(&filter) {ui.selectable_value(&mut self.selected,i,label);}
                    }
                });
            });
        }
        egui::Panel::bottom("provenance").show_inside(root,|ui| {
            if self.view_mode==2 && let Some(m)=self.meshes.get(self.mesh_index) {
                ui.label(format!("{} · block {} · object 0x{:08x} · {} LODs",m.id.container,m.id.block,m.id.offset,m.lod_count));
            } else if self.view_mode==1 && let Some(t)=self.terrains.get(self.terrain_index) {
                ui.label(format!("{} · block {} · object 0x{:08x} · {} × {} vertices",t.container,t.block,t.object,t.width,t.height));
            } else {let asset=&self.visuals.assets[self.selected];ui.label(format!("{} · block {} · object 0x{:08x} · {} × {}",asset.container,asset.block,asset.object,asset.width,asset.height));}
            ui.weak("Decoded from local Xbox data. Visual comparison with the original game is pending.");
            if let Some(error)=&self.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(egui::Color32::BLACK)).show_inside(root,|ui| {
            let result=match self.view_mode {1=>self.show_terrain(ui,graphics),2=>self.show_mesh(ui,graphics),_=>self.show_image(&ctx,ui)};
            if let Err(e)=result {self.error=Some(format!("{e:#}"));}
        });
    }
    fn show_mesh(&mut self,ui:&mut egui::Ui,graphics:&mut graphics::Graphics)->Result<()> {
        if self.mesh_key!=Some(self.mesh_index) {
            let mesh=avpe_assets::mesh::load(&self.catalog,&self.args.cache,&self.meshes[self.mesh_index])?;
            graphics.load_mesh(&mesh);self.mesh_key=Some(self.mesh_index);
        }
        if let Some(mesh)=&mut graphics.terrain {
            let rect=fit_rect(ui.max_rect(),egui::vec2(640.0,480.0));
            ui.painter().image(mesh.texture,rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),egui::Color32::WHITE);mesh.interact(ui,rect);
        }
        Ok(())
    }
    fn show_terrain(&mut self,ui:&mut egui::Ui,graphics:&mut graphics::Graphics)->Result<()> {
        let scene_changed=self.terrain_key.as_ref().is_none_or(|(id,_)|id!=&self.args.scene);
        if scene_changed {self.terrains=avpe_assets::terrain::load(&self.catalog,&self.args.cache,&self.args.scene)?;self.terrain_index=0;}
        let key=(self.args.scene.clone(),self.terrain_index);
        if self.terrain_key.as_ref()!=Some(&key) {graphics.load_terrain(&self.terrains[self.terrain_index]);self.terrain_key=Some(key);}
        if let Some(terrain)=&mut graphics.terrain {
            let rect=fit_rect(ui.max_rect(),egui::vec2(640.0,480.0));
            ui.painter().image(terrain.texture,rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),egui::Color32::WHITE);
            terrain.interact(ui,rect);
        }
        Ok(())
    }
    fn show_image(&mut self,ctx:&egui::Context,ui:&mut egui::Ui)->Result<()> {
        let asset=&self.visuals.assets[self.selected];
        if !self.textures.contains_key(&asset.image) {
            let image=image::open(self.args.cache.join(safe_relative(&asset.image)?))?.into_rgba8();
            let color=egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw());
            let texture=ctx.load_texture(&asset.image,color,egui::TextureOptions::LINEAR);
            // Keep GPU memory bounded while inspecting thousands of images.
            if self.textures.len()>=16 {self.textures.clear();}
            self.textures.insert(asset.image.clone(),texture);
        }
        let texture=&self.textures[&asset.image];
        let rect=fit_rect(ui.max_rect(),egui::vec2(asset.width as f32,asset.height as f32));
        ui.painter().image(texture.id(),rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),egui::Color32::WHITE);
        Ok(())
    }
    fn fail(&mut self,event_loop:&ActiveEventLoop,error:impl std::fmt::Display) {self.failure=Some(error.to_string());event_loop.exit();}
}

pub fn fit_rect(bounds:egui::Rect,size:egui::Vec2)->egui::Rect {
    let scale=(bounds.width()/size.x).min(bounds.height()/size.y);
    egui::Rect::from_center_size(bounds.center(),size*scale)
}

impl ApplicationHandler for App {
    fn resumed(&mut self,event_loop:&ActiveEventLoop) {
        if self.graphics.is_some() {return;}
        let window=match event_loop.create_window(Window::default_attributes().with_title("AVPE native Rust · asset inspection").with_inner_size(winit::dpi::PhysicalSize::new(self.args.width,self.args.height))) {
            Ok(window)=>Arc::new(window), Err(e)=>{self.fail(event_loop,e);return;}
        };
        match pollster::block_on(graphics::Graphics::new(window)) {
            Ok(graphics)=>self.graphics=Some(graphics),Err(e)=>self.fail(event_loop,format!("GPU initialization failed: {e:#}")),
        }
    }
    fn window_event(&mut self,event_loop:&ActiveEventLoop,_id:WindowId,event:WindowEvent) {
        let Some(mut graphics)=self.graphics.take() else {return;};
        let _=graphics.state.on_window_event(&graphics.window,&event);
        match event {
            WindowEvent::CloseRequested=>event_loop.exit(),
            WindowEvent::Resized(size)=>graphics.resize(size.width,size.height),
            WindowEvent::KeyboardInput{event,..} if event.state==ElementState::Pressed && event.logical_key==Key::Named(NamedKey::F1)=>self.inspect=!self.inspect,
            WindowEvent::RedrawRequested=> {
                let input=graphics.state.take_egui_input(&graphics.window);
                let ctx=graphics.ctx.clone();
                let output=ctx.run_ui(input,|ui|self.ui(ui,&mut graphics));
                let capture=if self.frames==4 {self.args.capture.as_deref()} else {None};
                match graphics.render(output,capture) {
                    Ok(rendered)=>if rendered {self.frames+=1;},
                    Err(e)=>self.fail(event_loop,format!("Rendering failed: {e:#}")),
                }
                if self.args.smoke_frames.is_some_and(|limit|self.frames>=limit) {event_loop.exit();}
            }
            _=>{}
        }
        self.graphics=Some(graphics);
    }
    fn about_to_wait(&mut self,_event_loop:&ActiveEventLoop) {if let Some(g)=&self.graphics {g.window.request_redraw();}}
}

fn main()->Result<()> {
    let args=Args::parse();
    ensure!(args.width>0 && args.height>0,"window size must be positive");
    ensure!(args.capture.is_none() || args.smoke_frames.is_none_or(|v|v>=5),"capture needs at least five frames");
    let mut app=App::new(args)?;
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(failure)=app.failure {anyhow::bail!(failure);}
    if let Some(error)=app.error {anyhow::bail!(error);}
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn letterbox_preserves_original_proportions() {
        let bounds=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1920.0,1080.0));
        let image=fit_rect(bounds,egui::vec2(640.0,480.0));
        assert_eq!(image.size(),egui::vec2(1440.0,1080.0));assert_eq!(image.left(),240.0);
    }
}
