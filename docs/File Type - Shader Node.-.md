# File Type: Shader Node

A `.shader_node` is the Stingray source-side **shader declaration**: the text
description of the passes, channels, permutation sets and material variables a
shader has. Darktide does not store it; `dtmt build` reads it (together with a
`.shader_source` library and, when needed, an `engine_data` file) and generates
the compiled shader section that is embedded in a base
[material](File%20Type%20-%20Material.-.md).

There is therefore **no binary layout** for this file type. The binary it
produces is the `shader_data` blob described in
[File Type - Material.-](File%20Type%20-%20Material.-.md) and the shader section
notes.

Status tags:

| Tag | Meaning |
|-----|---------|
| **Verified** | Confirmed against the reader and the declarations in the test mod. |
| **Verified in code** | Confirmed against the reader/tests only; no shipped data exercises it. |
| **Observed** | Seen in a file, but a wider sample could add cases. |
| **Guess** | Our inference; not verified. |
| **Unknown** | No reliable information. |

Only five top-level keys are read: `inputs`, `channels`, `permutation_sets`,
`shader_contexts` and `code_blocks`. Everything else (`render_state`,
`sampler_state`, `options`, …) is ignored, so a declaration ahead of the reader
still parses. Status: **Verified in code**.

## Schema

### `inputs` — material variables

A map of uuid → entry. The uuid is a stable identifier; the variable is
addressed by `name`.

| Key | Type | Default | Meaning | Status |
|-----|------|---------|---------|--------|
| `name` | string | — | the variable name a material sets and the group data hashes | **Verified** |
| `is_required` | bool | `false` | whether every permutation must supply it (alias `required`) | **Verified** |
| `type` | string or table | — | the type, and optionally the permutation flags that enable it | **Verified** |
| `domain` | string | `pixel` | `vertex` or `pixel` | **Verified** |

`type` is either a bare name (`type = "vector3"`) or a one-entry table tying
the type to flags (`type = { vector3: ["HAS_BASE_COLOR"] }`). Accepted type
names, in either the HLSL or the material spelling:

| Spelling | Type |
|----------|------|
| `float`, `scalar` | scalar (4 bytes) |
| `float2`, `vector2` | vector2 (8 bytes) |
| `float3`, `vector3` | vector3 (12 bytes) |
| `float4`, `vector4` | vector4 (16 bytes) |
| `float4x4`, `matrix` | 4×4 matrix (64 bytes) |
| `float3x3` | 4×4 matrix (64 bytes) — **Guess**: the group data has no 3×3 size, so the reader sizes it as 4×4; a real 3×3 (36/48 bytes) is not representable |
| `texture2d`, `texture` | 2D texture |

An input with no name, no type, or an unknown type is rejected. When the key is
a raw `#<hex>` hash the name may be left out and the key is the name. Status:
**Verified** (except `float3x3`, above).

The map is keyed by the uuid, but the reader indexes the normalized variables by
`name`, so two entries with the same `name` collapse to one (a reconstructor
that writes three `texture_map` records must key them separately, e.g.
`#<hash>`). Status: **Verified in code**.

### `channels` — values exchanged between stages

A nested map. Each leaf has a `type`; a leaf with a `type` is a channel, a map
without one is a condition gating more channels, so a channel can sit under a
path of conditions.

| Key | Type | Default | Meaning | Status |
|-----|------|---------|---------|--------|
| `type` | string | — | the channel's type; also what marks a leaf | **Verified** |
| `semantic` | string | none | the DXBC semantic to bind to | **Verified** |
| `domain` | string | `pixel` | the stage that produces it | **Verified** |
| `domains` | string[] | `[]` | the stages it belongs to, when listed | **Verified** |

A channel of the same name as an optional input follows that input's flag. A
channel under conditions is not part of any interface until the conditions are
evaluated against a permutation. Status: **Verified**.

### `permutation_sets` — the compile-time choices

A map of set name → list of choices. A choice has:

| Key | Type | Meaning | Status |
|-----|------|---------|--------|
| `if` | string | the expression that selects the choice (alias `condition`) | **Verified** |
| `define` | list, string or table | the macros the choice defines (alias `defines`) | **Verified** |
| `permute_with` | name, list or nested table | other sets this choice delegates to | **Verified** |
| `default` | bool | the fallback choice; a choice without `if` is the default | **Verified** |

A `define` table is `{ macros = [...], stages = [...] }`; `stages` limits which
stages the macros apply to (empty means all). A choice may `permute_with`
another set, which is expanded recursively, so a set can be composed of others.
Status: **Verified**.

### `shader_contexts` — named sets of passes

| Key | Type | Meaning | Status |
|-----|------|---------|--------|
| `passes_sort_mode` | string | `immediate` or `deferred` | **Observed** |
| `compile_with` | table[] | when the context compiles, and over which sets (alias `permute`) | **Verified** |
| `passes` | list | the passes, in declaration order | **Verified** |

A `compile_with` entry is `{ if = <condition>, permute_with = <set(s)> }`; an
empty `permute_with` means every set.

A `passes` entry is either a **pass** or a **branch** (a `then` list, which is
what tells them apart):

| Pass key | Type | Meaning | Status |
|----------|------|---------|--------|
| `layer` | string | the layer it is drawn on | **Verified** |
| `code_block` | string | the code block it compiles | **Verified** |
| `defines` / `define` | list, string or table | macros for this pass | **Verified** |
| `render_state` | string | the render state it draws with | **Verified** |
| `conditions` | string[] | a raw condition set, replacing the derived layer/render-state set | **Verified in code** |
| `group` | int | the group the pass selects | **Verified in code** |
| `branch_key` | string | the key variants are sorted by | **Observed** |

A branch is `{ if = <condition>, then = [...], else = [...] }` (aliases
`condition` and `otherwise`).

### `code_blocks` — the code a pass compiles

| Key | Type | Meaning | Status |
|-----|------|---------|--------|
| `include` | string[] | chunks to include, as `path#chunk` or the bare name of another block | **Verified** |
| `code` | string or table | the body: a bare string, or `{ shared, hlsl, glsl }` | **Verified** |
| `samplers` | table | the texture slots the block reads | **Verified** |

A `samplers` entry nests by condition like `channels`; a slot has `source`
(`material` when a material binds it, `resource_set` for engine resources),
`slot_name` (the name a material's `textures` uses), `type` and `sampler_state`
(`path#name`). Status: **Verified in code**.

## Example

```sjson
inputs = {
	"9c0d9f11-6a52-4a6d-8f6b-000000000001" = {
		name = "base_color"
		display_name = "Base Color"
		type = { vector4: ["HAS_BASE_COLOR"] }
		domain = "pixel"
	}
}
channels = {
	texcoord = { type = "float2" semantic = "TEXCOORD" domain = "vertex" }
}
code_blocks = { ui_default_base = { } }
permutation_sets = {
	channels = [
		{ define: "CHANNEL_0" }
		{ define: "CHANNEL_1" }
	]
}
shader_contexts = {
	default = {
		compile_with = [ { permute_with: "channels" } ]
		passes = [
			{ layer="gui" code_block="ui_default_base" render_state="transparent_mask" defines=["TAIL_0"] }
			{ code_block="ui_default_base" defines=["TAIL_1"] }
		]
	}
}
```

## Mapping

The declaration is the front end of the shader-section generator. It produces:

* the **compile jobs** — one per (context, permutation, pass), each naming the
  code block and the macros to compile under;
* the **interface** — the channels and variables that belong to a permutation,
  selected by the permutation's defines;
* the **groups** and their condition sets — one per context permutation, keyed
  by the pass's `layer`/`render_state` (or its raw `conditions`);
* the **permutations** — the recursive product of the sets a context's
  `compile_with` names.

These feed the emitters that write the section's context, condition, group,
device and default-data blocks. The binary details of those blocks are covered
in [File Type - Material.-](File%20Type%20-%20Material.-.md) and the shader
notes. Status: **Verified in code**; the in-game results are documented in
[Shader Progress](Shader%20Progress.md) and
[Shader Section Generation Notes](Shader%20Section%20Generation%20Notes.md).

## Open questions

* Which stages a pass compiles for is fixed to `vertex` + `pixel` (`STAGES`);
  there is no reader for a stage list. Status: **Verified in code**.
* A context that names no `permute_with` is treated as permuting over every set,
  but the real toolchain also drops unused sets; the count is an upper bound.
  Status: **Guess**.
* `passes_sort_mode`, `branch_key` and the `samplers` `sampler_state` references
  are carried but their effect on the generated section is not decoded.
  Status: **Unknown**.
* The top-level keys that are ignored (`render_state`, `sampler_state`,
  `options`, instance data, …) are not documented. Status: **Unknown**.
