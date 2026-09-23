# Custom shaders

A base material can ship custom shader programs. Put the HLSL next to the
material's SJSON and `dtmt build` does the rest: it compiles the shader with
`dxc`, replaces the matching programs in the material's shader section,
re-compresses every frame with Oodle, and updates all sizes, offsets and frame
keys.

## Layout

For a material at `materials/mods/example/ui_base.material`, any of these work:

| File | Meaning |
| --- | --- |
| `ui_base.hlsl` | Single file; `vs_main` and/or `ps_main` are compiled if present. |
| `ui_base.vs.hlsl` | Vertex shader, entry point `vs_main`, target `vs_6_0`. |
| `ui_base.ps.hlsl` | Pixel shader, entry point `ps_main`, target `ps_6_0`. |

The compiled container replaces **every program of that stage** in the
material's shader section. Programs of other stages are preserved byte for byte.

## Requirements

- `dxc.exe` must be available. `dtmt build` looks for it in this order:
  1. `dxc = "..."` in `dtmt.cfg`
  2. the `DTMT_DXC` environment variable
  3. the newest `C:\Program Files (x86)\Windows Kits\10\bin\*\x64\dxc.exe`
- The replacement must keep the shipped shader's interface. `dtmt build` checks
  the shader stage and both signature layouts (semantic names, indices,
  registers and masks) and refuses to build if they differ, because the other
  stages and the engine's input layouts are unchanged.

## Interface

Shipped Darktide shaders use bindless resources; the material writes descriptor
indices into `c_per_object`. `gui_tint.hlsl` is a minimal example for the UI
shader (`content/ui/materials/base/ui_default_base`):

```hlsl
Texture2D<float4> global_texture2D[] : register(t0, space2);
SamplerState global_samplers[] : register(s0, space2);

ConstantBuffer<GlobalViewport> global_viewport : register(b0);
ConstantBuffer<PerObject> c_per_object : register(b1);

struct PS_INPUT {
    float4 position : SV_Position;
    float4 color : CUSTOM0;
    float2 uv : CUSTOM1;
    float3 extra : CUSTOM2;
};

float4 ps_main(PS_INPUT input) : SV_Target0 {
    uint sampler_index = c_per_object.bindless_sampler_texture_map;
    uint texture_index = c_per_object.bindless_tex2d_texture_map.x;
    ...
}
```

Inspect a shipped shader's exact interface with:

```
cargo run -p sdk --example shader43 -- --dump out/containers <material data file>
dxc -dumpbin out/containers/<name>_p01.dxbc
```

## Under the hood

The material's shader section (`shader43`) contains framed DXBC programs:

```text
u32 envelope (1)
u32 frame_length
u8  frame[frame_length]        // an Oodle stream holding a DXBC container
u32 metadata_kind (5)
u32 decoded_dxbc_length
u64 frame_key                  // MurmurHash64A(frame, seed 0)
counted metadata tables and opaque state
```

Rebuilding decodes each frame with Oodle, swaps the container, re-compresses it
with Oodle's Kraken compressor, and writes the new frame length, decoded length
and frame key. The device data size and the default data offset in the shader
header are updated too.

## Manual experiments

`compile.ps1` compiles a shader on its own, and the `shader43` example can
replace programs in a material data file directly:

```
.\shaders\compile.ps1 -Input .\shaders\gui_tint.hlsl -Entry ps_main -Target ps_6_0 -Output .\out\gui_tint.dxbc
cargo run -p sdk --example shader43 -- --rebuild out\ --replace-ps out\gui_tint.dxbc <material data file>
```

The rebuilt data file still has to be deployed as the material's stream (for
example by updating `shader_data` in the material's SJSON and running
`dtmt build`).
