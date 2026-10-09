@group(0) @binding(0) var<uniform> camera: mat4x4<f32>;
@group(0) @binding(1) var material:texture_2d<f32>;
@group(0) @binding(2) var material_sampler:sampler;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
}
@vertex fn vs_main(@location(0) position:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) color:vec3<f32>, @location(3) uv:vec2<f32>) -> VertexOut {
    var output:VertexOut;
    output.position=camera*vec4<f32>(position,1.0);
    output.normal=normal;
    output.color=color;
    output.uv=uv;
    return output;
}
@fragment fn fs_main(input:VertexOut) -> @location(0) vec4<f32> {
    let light=0.25+0.75*max(dot(normalize(input.normal),normalize(vec3<f32>(-0.4,-0.6,0.9))),0.0);
    let texel=textureSample(material,material_sampler,input.uv);
    if(texel.a<0.05) {discard;}
    return vec4<f32>(texel.rgb*input.color*light,1.0);
}
