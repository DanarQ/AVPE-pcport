use avpe_assets::terrain::Terrain;
use bytemuck::{Pod,Zeroable};
use glam::{Mat4,Vec3};
use wgpu::util::DeviceExt;

#[repr(C)] #[derive(Clone,Copy,Pod,Zeroable)]
struct Vertex {position:[f32;3],normal:[f32;3],color:[f32;3],uv:[f32;2]}

pub struct TerrainView {
    pipeline:wgpu::RenderPipeline,vertices:wgpu::Buffer,indices:wgpu::Buffer,count:u32,
    uniform:wgpu::Buffer,bind:wgpu::BindGroup,
    color:wgpu::TextureView,depth:wgpu::TextureView,pub texture:egui::TextureId,
    pub target:Vec3,pub distance:f32,pub yaw:f32,pub pitch:f32,pub extent:f32,
}

impl TerrainView {
    pub fn new(device:&wgpu::Device,queue:&wgpu::Queue,renderer:&mut egui_wgpu::Renderer,terrain:&Terrain)->Self {
        let min=terrain.cells.iter().map(|c|c.height).fold(f32::INFINITY,f32::min);
        let max=terrain.cells.iter().map(|c|c.height).fold(f32::NEG_INFINITY,f32::max);
        let mut vertices=vec![];
        for y in 0..terrain.height {for x in 0..terrain.width {
            let left=Vec3::from(terrain.position(x.saturating_sub(1),y));let right=Vec3::from(terrain.position((x+1).min(terrain.width-1),y));
            let front=Vec3::from(terrain.position(x,y.saturating_sub(1)));let back=Vec3::from(terrain.position(x,(y+1).min(terrain.height-1)));
            let normal=(right-left).cross(back-front).normalize_or_zero();
            let position=terrain.position(x,y);let h=(position[2]-min)/(max-min).max(0.1);
            vertices.push(Vertex{position,normal:normal.to_array(),color:[0.15+h*0.40,0.25+h*0.40,0.30+h*0.35],uv:[0.0;2]});
        }}
        let extent=terrain.width.max(terrain.height) as f32*terrain.spacing;
        let target=Vec3::new(terrain.origin[0]+(terrain.width-1) as f32*terrain.spacing*0.5,terrain.origin[1]+(terrain.height-1) as f32*terrain.spacing*0.5,(min+max)*0.5);
        Self::build(device,queue,renderer,&vertices,&terrain.indices(),None,target,extent)
    }
    pub fn from_mesh(device:&wgpu::Device,queue:&wgpu::Queue,renderer:&mut egui_wgpu::Renderer,mesh:&avpe_assets::mesh::Mesh)->Self {
        let mut min=Vec3::splat(f32::INFINITY);let mut max=Vec3::splat(f32::NEG_INFINITY);
        let vertices=mesh.vertices.iter().map(|v| {
            min=min.min(Vec3::from(v.position));max=max.max(Vec3::from(v.position));
            Vertex{position:v.position,normal:v.normal,color:mesh.color,uv:v.uv}
        }).collect::<Vec<_>>();
        Self::build(device,queue,renderer,&vertices,&mesh.indices,mesh.texture.as_ref(),(min+max)*0.5,(max-min).max_element().max(0.01))
    }
    #[allow(clippy::too_many_arguments)]
    fn build(device:&wgpu::Device,queue:&wgpu::Queue,renderer:&mut egui_wgpu::Renderer,vertices:&[Vertex],indices:&[u32],texture:Option<&image::RgbaImage>,target:Vec3,extent:f32)->Self {
        let count=indices.len() as u32;
        let vertices=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("source terrain vertices"),contents:bytemuck::cast_slice(&vertices),usage:wgpu::BufferUsages::VERTEX});
        let indices=device.create_buffer_init(&wgpu::util::BufferInitDescriptor{label:Some("source terrain triangles"),contents:bytemuck::cast_slice(&indices),usage:wgpu::BufferUsages::INDEX});
        let uniform=device.create_buffer(&wgpu::BufferDescriptor{label:Some("inspection camera"),size:64,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let layout=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:None,entries:&[
            wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None},
            wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
            wgpu::BindGroupLayoutEntry{binding:2,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None},
        ]});
        let fallback=image::RgbaImage::from_pixel(1,1,image::Rgba([255;4]));let texture=texture.unwrap_or(&fallback);
        let material=device.create_texture(&wgpu::TextureDescriptor{label:Some("source material"),size:wgpu::Extent3d{width:texture.width(),height:texture.height(),depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8UnormSrgb,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
        queue.write_texture(material.as_image_copy(),texture.as_raw(),wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(texture.width()*4),rows_per_image:None},material.size());
        let material_view=material.create_view(&Default::default());
        let sampler=device.create_sampler(&wgpu::SamplerDescriptor{address_mode_u:wgpu::AddressMode::Repeat,address_mode_v:wgpu::AddressMode::Repeat,mag_filter:wgpu::FilterMode::Linear,min_filter:wgpu::FilterMode::Linear,..Default::default()});
        let bind=device.create_bind_group(&wgpu::BindGroupDescriptor{label:None,layout:&layout,entries:&[wgpu::BindGroupEntry{binding:0,resource:uniform.as_entire_binding()},wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::TextureView(&material_view)},wgpu::BindGroupEntry{binding:2,resource:wgpu::BindingResource::Sampler(&sampler)}]});
        let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:None,bind_group_layouts:&[Some(&layout)],immediate_size:0});
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("diagnostic terrain shader"),source:wgpu::ShaderSource::Wgsl(include_str!("terrain.wgsl").into())});
        let pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
            label:Some("terrain inspection"),layout:Some(&pipeline_layout),
            vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs_main"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:44,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x2]}]},
            fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format:wgpu::TextureFormat::Rgba8UnormSrgb,blend:None,write_mask:wgpu::ColorWrites::ALL})]}),
            primitive:wgpu::PrimitiveState{cull_mode:None,..Default::default()},
            depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:Some(true),depth_compare:Some(wgpu::CompareFunction::Less),stencil:Default::default(),bias:Default::default()}),
            multisample:Default::default(),multiview_mask:None,cache:None,
        });
        let color=device.create_texture(&wgpu::TextureDescriptor{label:Some("4:3 terrain viewport"),size:wgpu::Extent3d{width:960,height:720,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8UnormSrgb,usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[]}).create_view(&Default::default());
        let depth=device.create_texture(&wgpu::TextureDescriptor{label:Some("terrain depth"),size:wgpu::Extent3d{width:960,height:720,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[]}).create_view(&Default::default());
        let texture=renderer.register_native_texture(device,&color,wgpu::FilterMode::Linear);
        Self{pipeline,vertices,indices,count,uniform,bind,color,depth,texture,target,distance:extent*1.7,yaw:-1.0,pitch:0.8,extent}
    }
    pub fn render(&self,queue:&wgpu::Queue,encoder:&mut wgpu::CommandEncoder) {
        let direction=Vec3::new(self.yaw.cos()*self.pitch.cos(),self.yaw.sin()*self.pitch.cos(),self.pitch.sin());
        let view=Mat4::look_at_rh(self.target+direction*self.distance,self.target,Vec3::Z);
        let projection=Mat4::perspective_rh(50.0f32.to_radians(),4.0/3.0,self.extent*0.001,self.extent*20.0);
        queue.write_buffer(&self.uniform,0,bytemuck::cast_slice(&(projection*view).to_cols_array()));
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("terrain"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:&self.color,resolve_target:None,depth_slice:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color{r:0.015,g:0.02,b:0.03,a:1.0}),store:wgpu::StoreOp::Store}})],depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment{view:&self.depth,depth_ops:Some(wgpu::Operations{load:wgpu::LoadOp::Clear(1.0),store:wgpu::StoreOp::Discard}),stencil_ops:None}),..Default::default()});
        pass.set_pipeline(&self.pipeline);pass.set_bind_group(0,&self.bind,&[]);pass.set_vertex_buffer(0,self.vertices.slice(..));pass.set_index_buffer(self.indices.slice(..),wgpu::IndexFormat::Uint32);pass.draw_indexed(0..self.count,0,0..1);
    }
    pub fn interact(&mut self,ui:&mut egui::Ui,rect:egui::Rect) {
        let response=ui.interact(rect,ui.id().with("terrain-camera"),egui::Sense::drag());
        if response.hovered() {let scroll=ui.input(|i|i.smooth_scroll_delta.y);self.distance=(self.distance*(-scroll*0.002).exp()).clamp(self.extent*0.025,self.extent*5.0);}
        let delta=ui.input(|i|i.pointer.delta());
        if response.dragged_by(egui::PointerButton::Primary) {self.yaw-=delta.x*0.006;self.pitch=(self.pitch+delta.y*0.006).clamp(0.05,1.54);}
        if response.dragged_by(egui::PointerButton::Secondary) {
            let right=Vec3::new(-self.yaw.sin(),self.yaw.cos(),0.0);let forward=Vec3::new(self.yaw.cos(),self.yaw.sin(),0.0);
            self.target+=(right*delta.x-forward*delta.y)*self.distance/rect.height().max(1.0);
        }
    }
}
