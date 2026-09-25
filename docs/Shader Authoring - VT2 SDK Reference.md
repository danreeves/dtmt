# Shader authoring reference (VT2 SDK)

The Vermintide 2 SDK ships the same engine's authoring files, so they document
what the Darktide sections were compiled *from*: `.shader_node` files (node
definitions with their inputs, outputs and inline shader source),
`shader_libraries/*.shader_source` (library sources) and
`import_config/texture_format_spec.config` (texture formats).

## Cbuffers are declared in HLSL inside the output nodes

`core/stingray_renderer/output_nodes/standard_base.shader_node` contains the
shader source itself, including the per-object constant buffer:

```hlsl
CBUFFER_START(c_per_object)
    float4x4 view_proj;
    float4x4 world_view_proj;
    #if defined(NEEDS_INV_WORLD)
        float4x4 inv_world;
    #endif
    float4x4 world;
    float4x4 last_world;
    #if defined(JITTER_TRANSPARENCY)
        float inv_jitter_alpha;
    #endif
    ...
CBUFFER_END
```

So the cbuffer names, member order and sizes we see in a material's group data
tables and program tails are written by hand in the shader source, and the
`#if defined(...)` guards are the same permutations that turn up as the
condition tree in `shader43`. Darktide's equivalent node sources are not
shipped, but their compiled result is what we decoded: `c_per_object` on the UI
base holds `texture_map` (a channel), `view_proj`, `world_view_proj`, `world`
and `dev_wireframe_color`, in that order.

`core/stingray_renderer/shader_libraries/common.shader_source` declares
`global_viewport` and the engine globals, `placeholders.shader_source` mentions
`c_per_object` too.

## Texture channels are node inputs, and formats live in a config

`shader_nodes/*.shader_node` declare texture inputs (for example
`sample_texture_or_pass_through.shader_node`, `flipbook_sampler.shader_node`),
and `core/import_config/texture_format_spec.config` lists the formats
(`R8G8B8A8`, `R16G16B16A16F`, `BC4`, `BC5`, ...) with their alpha/min-size
rules. The material side then names those channels: the Darktide materials we
decoded declare `channels = [ "#3A04AF1C" ]` and
`textures = { "#3A04AF1C" = "#63B5BD3FD54C00C2" }`, and the material's shared
block carries the same name as its first record, `{name_hash, 3, 1}` - `a` being
3 or 4, i.e. plausibly the texture's component count (RGB vs RGBA) rather than
anything engine-internal. Editor-generated names like `curve_texture_43db3c3b`
(which the dictionary resolves) come from setting a curve texture up in the
editor and are stored in the material / shader node, not derived at runtime.

## Permutations are named defines

Node inputs carry `type = { vector3: ["HAS_VERTEX_OFFSET"] }`-style permutation
defines, and the output node's `channels`/`graph` blocks are keyed by conditions
such as `"(defined(HAS_NORMAL) && !defined(WORLD_SPACE_NORMAL))"`. That is the
authoring form of `shader43`'s contexts/conditions tree: the engine compiles the
defines into the group variants and the condition tree selects them.

## What this means for generating our own sections

- The variable layout we must generate for a custom base material has a readable
  source: our own HLSL's cbuffer declarations (names, members, sizes) plus the
  material's channel/texture list.
- The shared block's records line up one-for-one with the material's channel
  entries (`{name_hash, 3|4, 1}`), so fitting its grammar should be a matter of
  walking those entries rather than reverse engineering an opaque blob.
- The texture formats come from `texture_format_spec.config`-style data, so link
  the "3 or 4" field to the channel's format when fitting.
