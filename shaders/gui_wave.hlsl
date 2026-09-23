// Example Darktide vertex shader.
//
// This replaces the `gui` shader's vertex stage used by
// `content/ui/materials/base/ui_default_base`. It keeps the shipped shader's
// interface exactly:
//
//   inputs   POSITION, COLOR, TEXCOORD0
//   output   SV_Position, CUSTOM0 (color), CUSTOM1 (uv), CUSTOM2
//   b0       c_per_object (per-object constants; the vertex stage uses b0)
//
// and adds a gentle static wave in clip space.

struct PerObject {
    uint bindless_sampler_texture_map;
    uint2 bindless_tex2d_texture_map;
    uint2 bindless_minlod_texture_map;
    float4x4 view_proj;
    float4x4 world_view_proj;
    float4x4 world;
    float4 dev_wireframe_color;
};
ConstantBuffer<PerObject> c_per_object : register(b0);

struct VS_INPUT {
    float4 position : POSITION;
    float4 color : COLOR;
    float2 uv : TEXCOORD0;
};

struct PS_INPUT {
    float4 position : SV_Position;
    float4 color : CUSTOM0;
    float2 uv : CUSTOM1;
    float3 extra : CUSTOM2;
};

PS_INPUT vs_main(VS_INPUT input) {
    float4 clip = mul(input.position, c_per_object.world_view_proj);

    clip.x += sin(clip.y * 12.0) * 0.03;
    clip.y += sin(clip.x * 12.0) * 0.03;

    PS_INPUT output;
    output.position = clip;
    output.color = input.color;
    output.uv = input.uv;
    output.extra = 0;
    return output;
}
