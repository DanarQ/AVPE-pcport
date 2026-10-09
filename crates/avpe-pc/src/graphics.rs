use anyhow::{Context,Result,ensure};
use std::{path::Path,sync::Arc,time::Duration};
use winit::window::Window;

pub struct Graphics {
    pub window:Arc<Window>, pub ctx:egui::Context, pub state:egui_winit::State,
    instance:wgpu::Instance, surface:wgpu::Surface<'static>, device:wgpu::Device, queue:wgpu::Queue,
    config:wgpu::SurfaceConfiguration, renderer:egui_wgpu::Renderer,
    pub terrain:Option<crate::terrain_view::TerrainView>,
}

impl Graphics {
    pub async fn new(window:Arc<Window>)->Result<Self> {
        let instance=wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface=instance.create_surface(window.clone())?;
        let adapter=instance.request_adapter(&wgpu::RequestAdapterOptions{compatible_surface:Some(&surface),..Default::default()}).await?;
        let (device,queue)=adapter.request_device(&wgpu::DeviceDescriptor::default()).await?;
        let size=window.inner_size();
        let caps=surface.get_capabilities(&adapter);
        let mut config=surface.get_default_config(&adapter,size.width.max(1),size.height.max(1)).context("surface configuration unavailable")?;
        config.format=caps.formats.iter().copied().find(|f|f.is_srgb()).unwrap_or(config.format);
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {config.usage|=wgpu::TextureUsages::COPY_SRC;}
        surface.configure(&device,&config);
        eprintln!("GPU: {} ({:?}); surface {:?}",adapter.get_info().name,adapter.get_info().backend,config.format);
        let ctx=egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        let state=egui_winit::State::new(ctx.clone(),egui::ViewportId::ROOT,window.as_ref(),Some(window.scale_factor() as f32),window.theme(),Some(device.limits().max_texture_dimension_2d as usize));
        let renderer=egui_wgpu::Renderer::new(&device,config.format,egui_wgpu::RendererOptions::default());
        Ok(Self{window,ctx,state,instance,surface,device,queue,config,renderer,terrain:None})
    }
    pub fn load_terrain(&mut self,terrain:&avpe_assets::terrain::Terrain) {
        if let Some(previous)=self.terrain.take() {self.renderer.free_texture(&previous.texture);}
        self.terrain=Some(crate::terrain_view::TerrainView::new(&self.device,&self.queue,&mut self.renderer,terrain));
    }
    pub fn load_mesh(&mut self,mesh:&avpe_assets::mesh::Mesh) {
        if let Some(previous)=self.terrain.take() {self.renderer.free_texture(&previous.texture);}
        self.terrain=Some(crate::terrain_view::TerrainView::from_mesh(&self.device,&self.queue,&mut self.renderer,mesh));
    }
    pub fn resize(&mut self,width:u32,height:u32) {
        if width>0 && height>0 {self.config.width=width;self.config.height=height;self.surface.configure(&self.device,&self.config);}
    }
    pub fn render(&mut self,output:egui::FullOutput,capture:Option<&Path>)->Result<bool> {
        self.state.handle_platform_output(&self.window,output.platform_output);
        let jobs=self.ctx.tessellate(output.shapes,output.pixels_per_point);
        for (id,delta) in &output.textures_delta.set {self.renderer.update_texture(&self.device,&self.queue,*id,delta);}
        let frame=match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)|wgpu::CurrentSurfaceTexture::Suboptimal(frame)=>frame,
            wgpu::CurrentSurfaceTexture::Outdated=>{self.surface.configure(&self.device,&self.config);return Ok(false);}
            wgpu::CurrentSurfaceTexture::Lost=>{self.surface=self.instance.create_surface(self.window.clone())?;self.surface.configure(&self.device,&self.config);return Ok(false);}
            wgpu::CurrentSurfaceTexture::Timeout|wgpu::CurrentSurfaceTexture::Occluded=>return Ok(false),
            wgpu::CurrentSurfaceTexture::Validation=>anyhow::bail!("surface validation failed"),
        };
        let screen=egui_wgpu::ScreenDescriptor{size_in_pixels:[self.config.width,self.config.height],pixels_per_point:output.pixels_per_point};
        let view=frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder=self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        if let Some(terrain)=&self.terrain {terrain.render(&self.queue,&mut encoder);}
        let extra=self.renderer.update_buffers(&self.device,&self.queue,&mut encoder,&jobs,&screen);
        {
            let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label:Some("AVPE native presentation"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                    view:&view,resolve_target:None,depth_slice:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color::BLACK),store:wgpu::StoreOp::Store},
                })],..Default::default()
            }).forget_lifetime();
            self.renderer.render(&mut pass,&jobs,&screen);
        }
        let row_bytes=(self.config.width*4).div_ceil(256)*256;
        let readback=if capture.is_some() {
            ensure!(self.config.usage.contains(wgpu::TextureUsages::COPY_SRC),"GPU surface cannot be captured");
            let buffer=self.device.create_buffer(&wgpu::BufferDescriptor{label:Some("capture"),size:u64::from(row_bytes)*u64::from(self.config.height),usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
            encoder.copy_texture_to_buffer(frame.texture.as_image_copy(),wgpu::TexelCopyBufferInfo{buffer:&buffer,layout:wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(row_bytes),rows_per_image:None}},wgpu::Extent3d{width:self.config.width,height:self.config.height,depth_or_array_layers:1});
            Some(buffer)
        } else {None};
        self.queue.submit(extra.into_iter().chain([encoder.finish()]));
        if let (Some(path),Some(buffer))=(capture,readback) {
            let (sender,receiver)=std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read,move |result|{let _=sender.send(result);});
            self.device.poll(wgpu::PollType::Wait{submission_index:None,timeout:Some(Duration::from_secs(20))})?;
            receiver.recv_timeout(Duration::from_secs(20))??;
            let bytes=buffer.slice(..).get_mapped_range();
            let mut rgba=Vec::with_capacity((self.config.width*self.config.height*4) as usize);
            for row in bytes.chunks_exact(row_bytes as usize) {rgba.extend_from_slice(&row[..self.config.width as usize*4]);}
            if matches!(self.config.format,wgpu::TextureFormat::Bgra8UnormSrgb|wgpu::TextureFormat::Bgra8Unorm) {for pixel in rgba.chunks_exact_mut(4) {pixel.swap(0,2);}}
            if let Some(parent)=path.parent() {std::fs::create_dir_all(parent)?;}
            image::save_buffer(path,&rgba,self.config.width,self.config.height,image::ColorType::Rgba8)?;
            drop(bytes);buffer.unmap();eprintln!("Captured {}",path.display());
        }
        frame.present();
        for id in &output.textures_delta.free {self.renderer.free_texture(id);}
        Ok(true)
    }
}
