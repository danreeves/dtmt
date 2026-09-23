Contains the layout of `stingray::MaterialResource` as used by Darktide, as
well as the SJSON format DTMT compiles from and decompiles to.

## Binary format

A material's bundle entry is a small resource header whose payload lives in an
external `data/<xx>/<hash>` file (property `DATA`), exactly like streamed
textures. The payload is the material stream:

```
u32 version                 // 60 or 61
u32 material_offset         // always 28
u32 material_size
u32 shader_offset           // u32::MAX if the material has no embedded shader
u32 shader_size
u32 unk2_offset             // u32::MAX if absent
u32 unk2_size
// at material_offset:
u32 name                    // IdString32, usually 0
MaterialTemplate...
// at shader_offset (if present): raw compiled shader blob
// at unk2_offset (if present): raw blob
```

`MaterialTemplate` uses the regular Stingray (little-endian) serialization:

| Type | Field | Meaning |
|------|-------|---------|
| `u64` | `material1` | primary parent material |
| `u64` | `material2` | secondary parent material |
| `IdString32[]` | `unk1` | shader texture channels (base materials) |
| `(IdString32, u64)[]` | `textures` | channel → texture resource |
| `(IdString32, IdString32)[]` | `material_contexts` | context → context material |
| `ShaderVariableReflection[]` | variables | `(u32 class, u32 elements, IdString32 name, u32 offset, u32 stride)` |
| `u8[]` | `variable_data` | packed variable values, indexed by `offset` |
| `(IdString32, bool)[]` | `unk2` | |
| `(u32, u32)[]` | `unk3` | |

`ShaderVariableReflection::class` is `0` scalar, `1` vector2, `2` vector3,
`3` vector4 and `12` for arbitrary data (where `elements * stride` gives the
byte size).

A material whose `shader_size > 0` ("base material") carries its own compiled
shader. Every other material is an *instance* that inherits the shader through
`material1`/`material2`. DTMT can compile instance materials only; shaders are
compiled DXBC blobs. Decompiling a base material emits its `shader_size` but
compiling it again is rejected.

`variable_data` may contain floats that are not described by any reflection
entry. They are preserved through the `extra_data` field so that
decompile → compile round trips stay byte-exact.

### Material contexts

`material_contexts` is *not* about resource paths. It maps a context name to a
context material name, both of which are 32-bit names. Darktide's
`surface_material` context is the surface type of the material, e.g. `cloth`,
`dirt` or `bone`, which is what decals, hit effects and footsteps use. The
Vermintide 2 source dump shows the same field with values such as `metal`,
`snow` or `flesh`.

## SJSON format

The SJSON format follows the Stingray source format (the same one used by
Vermintide 2's `.material` files). Values are resource paths; DTMT hashes them
the same way the game does. Decompilation writes the known name of a hash, or
an explicit `#`-prefixed hex hash for values that could not be named.

```sjson
parent_material = "content/ui/materials/base/ui_default_base"
material_contexts = {
  surface_material = "bone"
}
textures = {
  texture_map = "content/ui/textures/loading/loading_screen_background"
}
variables = {
  dirt = {
    type = "scalar"
    value = [0.5]
    offset = 0
  }
}
```

Fields:

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `name` | string | no | material name (`IdString32`), usually omitted |
| `parent_material` | string | no | `material1` |
| `parent_material_2` | string | no | `material2` |
| `material_contexts` | map | no | context name → context material name |
| `textures` | map | no | channel name → texture resource path |
| `variables` | map | no | variable name → `{ type, value, offset?, elements?, stride? }` |
| `channels` | string[] | no | `unk1`, only on base materials |
| `shader_size` | integer | no | set when decompiling a base material |
| `extra_data` | string | no | hex of `variable_data` bytes not covered by variables |
| `unk2` | map | no | unnamed `(name, bool)` pairs |
| `unk3` | array | no | `(a, b)` pairs |

Variable values are always arrays, even for `type = "scalar"`. The SJSON number
grammar parses a bare float like `0.5` as the integer `0` followed by `.5`, so
scalars cannot be represented as bare numbers without corrupting the file.

`offset` is optional. When omitted, the value is packed after the previous
variable. Decompiled materials always include it, because the game's instance
materials distribute their variables at offsets that are not reproducible from
the variable list alone.

### Base materials and new textures

To use an entirely new texture, create an instance material that references an
existing base material and point one of its texture channels at the new texture
resource:

```sjson
parent_material = "content/ui/materials/base/ui_default_base"
textures = {
  texture_map = "textures/mods/example/my_image"
}
```

Whether the reference resolves depends on the texture resource being present in
the bundle database, which DTMM takes care of when deploying the mod.

### Referencing a new material from Lua

A new material is a new resource, so nothing loads it until something asks for
it by name. The most reliable way to do that in a mod is to point a UI widget at
it at runtime. For example, to replace the loading screen background:

```lua
local LoadingView = require("scripts/ui/views/loading_view/loading_view")

local LOADING_MATERIAL = "materials/mods/example/loading_screen_background"

local original_on_enter = LoadingView.on_enter

LoadingView.on_enter = function (self)
    original_on_enter(self)

    local widget = self._widgets_by_name and self._widgets_by_name.background
    for _, pass in ipairs(widget.passes or {}) do
        if pass.pass_type == "texture" then
            widget.content[pass.value_id] = LOADING_MATERIAL
        end
    end
end
```

The widget is only built once the view's package has loaded, which is why this
runs in `on_enter` rather than `init`. `LoadingView.init` rebuilds the
background widget from `Views.loading_view.backgrounds`, so patching the view's
definitions file is not enough.

### Self-contained materials

Instance materials inherit their shader from a parent material. Referencing a
game material directly can stop the engine from unloading the mod package
cleanly, because the base material is shared with the game. A mod can instead
ship its own copy of a base material: decompiling one emits its compiled shader
as `shader_data`, and compiling that again reproduces the original blob
byte-for-byte. Pointing the new instance material at the mod's own base material
keeps the whole material graph owned by the mod.

## Status

- Binary format: **Partial**. The header, material template and external data
  file layout are known, and instance materials round trip byte-for-byte.
  `unk1`/`unk2`/`unk3` are preserved but not understood.
- Compilation: **Partial**. Instance materials only; embedded shaders are not
  supported.
- Decompilation: **Partial**. Base materials are decompiled for inspection but
  cannot be re-compiled.
