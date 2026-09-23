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
indices into `c_per_object`. Two examples for the UI shader
(`content/ui/materials/base/ui_default_base`) are included:

- `gui_tint.hlsl` — pixel shader: samples the material texture through the
  bindless arrays and tints it.
- `gui_wave.hlsl` — vertex shader: same interface as the shipped vertex stage
  with a small wave in clip space.

To modify an *existing* shader instead of writing one, its compiled program can
be translated back to editable HLSL; see
[Shader Decompilation](../docs/Shader%20Decompilation.md).

The pixel shader example:

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

## Applying a material from Lua

Pointing a widget at a custom material is done at runtime:

```lua
local LoadingView = require("scripts/ui/views/loading_view/loading_view")

local MATERIAL = "materials/mods/example/loading_screen_background"

local function set_texture_material(view, widget_name, material)
    local widget = view._widgets_by_name and view._widgets_by_name[widget_name]
    if not widget then
        return
    end
    for _, pass in ipairs(widget.passes or {}) do
        if pass.pass_type == "texture" then
            widget.content[pass.value_id] = material
        end
    end
end

local original_on_enter = LoadingView.on_enter
LoadingView.on_enter = function (self)
    original_on_enter(self)
    set_texture_material(self, "background", MATERIAL)
end

-- Also patch `update`: DTMT can inject the mod's Lua *after* a view has already
-- been created and entered (for example the title view during the
-- splash -> title transition). `on_enter` alone would never run for that
-- instance, while a patched `update` reaches it on the next frame.
local original_update = LoadingView.update
LoadingView.update = function (self, ...)
    original_update(self, ...)
    set_texture_material(self, "background", MATERIAL)
end
```

Only assign when the value actually changes if you apply it from `update`, so
the widget is not marked dirty every frame.

## Driving shader parameters from Lua

Shipped shaders expose a fixed set of material variables: the UI shader has the
float4 `dev_wireframe_color`, HUD shaders have `ui_scale` and `distortion`, and
so on. A custom shader can read one of those as its own parameter, and Lua can
write it through the UI pass' `material_values`.

The engine resolves the variable by name against the shader's own variable
table, which lives in the shader's group data: that table maps the name hash to
the cbuffer offset the value is written to. The material's `variables` block
only carries the value and its offset inside the material's own `variable_data`.

1. Read the variable in the shader, at the offset the shipped variable table
   uses (inspect the shipped container to find it):

   ```hlsl
   struct PerObject {
       ...
       float4 dev_wireframe_color;   // the UI shader exposes this at offset 224
   };

   ...
   c.rgb *= c_per_object.dev_wireframe_color.rgb;
   ```

2. Declare it on the material so it has storage and a default:

   ```sjson
   variables = {
     dev_wireframe_color = {
       type = "vector4"
       value = [1, 1, 1, 1]
       offset = 0
     }
   }
   ```

3. Drive it from Lua. `UIPasses.texture.draw` applies `ui_style.material_values`
   with `Material.set_scalar`/`set_vector2`/`set_vector3`/`set_vector4`, so the
   table has to be the one the pass draws with — the pass style itself, or the
   whole widget style when the pass has no `style_id`:

   ```lua
   local function set_material_value(widget, name, value)
       local style = widget.style
       if not style then
           return false
       end
       for _, pass in ipairs(widget.passes or {}) do
           if pass.pass_type == "texture" then
               local pass_style = (pass.style_id and style[pass.style_id]) or style
               pass_style.material_values = pass_style.material_values or {}
               pass_style.material_values[name] = value
               return true
           end
       end
       return false
   end
   ```

   The shipped HUD passes set `style_id = "texture"` in their definitions, which
   is why the game's examples use `widget.style.texture.material_values`. A pass
   without a `style_id` gets `style_id_<pass index>` instead, so resolving the
   style through `widget.passes` is the robust way.

Only variables that the shader's variable table lists can be driven this way.
The engine resolves a variable by looking its name hash up in the shader's
group data tables, which map the name to a cbuffer offset. Adding a *new* name
means appending a record to those tables, and reusing a shipped name means
aliasing its slot. Both were prototyped and validated in game (an appended
`mod_tint` record renders and Lua-drives correctly), but aliasing a shipped
variable is the prudent choice, so nothing shipped in DTMT edits the tables:
`dev_wireframe_color` (a debug variable at offset 224 that the UI shader never
reads) is the general purpose slot, and exactly what shipping mod parameters
should fall back to until the wrapper is decoded for real.

For the record, the experiment showed every copy of a table has to change (the
UI shader's group data serializes its per-object table 36 times for 12 groups),
and that growing the per-object cbuffer does not work as a new-slot source: the
recompiled shader can declare 256 bytes, every encoded size (the tail entries,
the `{240, 64, count}` group data headers and the containers' statistics) can be
patched into agreement, and the engine still reads zeros in the new slot. The
buffer allocation therefore has another input somewhere in the format.

## Status

Working:

- HLSL next to a material is compiled with `dxc` and spliced into the base
  material's shader section; frames are re-compressed with Oodle and the section
  header, sizes and frame keys are updated.
- Replacements are checked against the shipped interface (stage and both
  signature layouts); unreplaced programs are preserved byte-for-byte.
- Both stages can be replaced, so no shipped program code has to remain.
- Existing shaders can be translated back to editable HLSL with `dxil-spirv`
  and `SPIRV-Cross` (see `docs/Shader Decompilation.md`); the generated code
  needs its entry point and semantics restored from the shipped signatures.

Not implemented yet (see `docs/File Type - Material.-.md` for the format
unknowns behind these):

- generating the per-program metadata (reflection) for a shader with a
  different interface
- shader libraries with several of our own programs and permutation selection
- binding layouts other than the cloned one (the engine supplies the root
  signature)
- adding *new* shader parameters: Lua can drive the variables a shipped shader
  already exposes, but new variable entries would have to be added to the group
  data
