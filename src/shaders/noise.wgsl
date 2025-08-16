struct Params {
  size:  vec2<f32>, // 8B
  frame: u32,       // +4B
  _pad:  u32,       // +4B → 合計16B（std140でもOK）
}

@group(0) @binding(0) var<uniform> params: Params;

struct VSOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, };

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VSOut {
  var p = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -3.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 3.0,  1.0)
  );
  var o: VSOut;
  o.pos = vec4<f32>(p[vid], 0.0, 1.0);
  o.uv = (p[vid] * 0.5 + vec2<f32>(0.5, 0.5));
  return o;
}

// 適当ハッシュ（そのままでOK）
fn hash2(p: vec2<f32>, seed: f32) -> f32 {
  let q = vec2<f32>(
    dot(p, vec2<f32>(127.1, 311.7)),
    dot(p, vec2<f32>(269.5, 183.3))
  );
  let s = sin(q + seed) * 43758.5453;
  return fract(sin(dot(s, vec2<f32>(1.0, 7.0))) * 0.5 + 0.5);
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
  let coord = in.uv * params.size;
  let n = hash2(coord, f32(params.frame));
  return vec4<f32>(vec3<f32>(n), 1.0);
}