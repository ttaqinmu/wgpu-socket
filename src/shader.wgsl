struct Uniforms {
    rotation: vec4<f32>,
    params: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) brightness: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let rx = uniforms.rotation.x;
    let ry = uniforms.rotation.y;
    let rz = uniforms.rotation.z;
    let mx = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, cos(rx), sin(rx)), vec3<f32>(0.0, -sin(rx), cos(rx)));
    let my = mat3x3<f32>(vec3<f32>(cos(ry), 0.0, -sin(ry)), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(sin(ry), 0.0, cos(ry)));
    let mz = mat3x3<f32>(vec3<f32>(cos(rz), sin(rz), 0.0), vec3<f32>(-sin(rz), cos(rz), 0.0), vec3<f32>(0.0, 0.0, 1.0));
    let rotation = mz * my * mx;
    let position = rotation * (input.position * uniforms.params.x);
    let z = 4.0 - position.z;
    let f = 1.7320508;
    output.position = vec4<f32>(position.x * f / uniforms.params.y, position.y * f, z * 1.010101 - 0.1010101, z);
    let normal = normalize(rotation * input.normal);
    output.brightness = 0.32 + 0.68 * max(dot(normal, normalize(vec3<f32>(0.5, 0.8, 1.0))), 0.0);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(uniforms.color.rgb * input.brightness, 1.0);
}
