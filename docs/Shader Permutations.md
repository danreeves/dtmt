# Shader permutations: what a key may contain

Working reference for the tokens a `shader43` permutation key is built from, and
which of them are engine constants rather than shader- or material-specific. It
collects what the toolchain can currently generate and what is still carried,
and grounds each claim in the VT2 SDK's own source (`core/stingray_renderer/**`)
and the engine's dev binaries, since Darktide ships no declarations of its own.

See `Shader RE TODO.md` (the worklist) and `Shader Section Generation Notes.md`
(the target and what each section needs) for the surrounding decode.

## The two key spaces

There are **two** key spaces, and they differ in more than the tail:

- the **query key** (the id a section's query carries):
  `<shader>:<context>:<defines, sorted>:PLATFORM_<platform>:RENDERER_<renderer>`
  - the VT2 compiler logs these, e.g.
    `gui:default:DIFFUSE_MAP:ONE_BIT_ALPHA:PLATFORM_WIN32:RENDERER_D3D12`,
    and the id is the high 32 bits of MurmurHash64A over it;
- the **library name** (what `.editor` / `debug_file_index.sjson` list, and what
  a compiled `<name>.shader_library` is called): `<shader>:<tokens in
  declaration order>`, lowercased, **no context**.

Both are modelled in `lib/sdk/src/filetype/permutation.rs` (`key`, `id`,
`library_key`, `library_file`), with tests pinned to the SDK compiler's own
output. The VT2 compiler's `debug_file_index.sjson` was reproduced 220/220 by
`examples/vt2_editor.rs`, and `vt2_key_check.rs` reproduces the key rule against
the SDK's `.shader_source` files.

## What feeds the token set

Three declaration mechanisms put tokens into a key (all three are already read by
`filetype::shader_node` and fed to `compile_jobs`):

1. **`static_compile` entries** - `{ shader = "copy" defines = "CUBE_CAPTURE" }`;
   the shader name and the sorted `defines` are the whole key.
2. **passes** in a `shader_contexts` entry - `{ layer=... code_block=...
   render_state=... defines=[...] }`, selected through a `defined=`/`fail=`
   decision tree. The pass's `defines`, its `render_state` and its `layer`
   become the identifiers.
3. **`#if defined(X)` branches inside the code block** - the compiler permutes
   the code block over the conditions the source tests, so each branch becomes a
   token even when no declaration line names it. This is why `copy` compiles 10
   keys from one pass (`copy:clear`, `copy:flip_y:…`).

A pass's `render_state` is what makes names like `clear`, `flatten`,
`shadow_mapping` and `write_mask` appear as key tokens beside define-like ones
(`diffuse_map`, `one_bit_alpha`). The two are not distinguished in the key.

## The token vocabulary

A token is **any name the shader tests or nests under**, plus the identifiers the
declaration names. Concretely, from the SDK:

- **layers / render states** - the `layer=` and `render_state=` values across
  115 distinct names in the SDK's output nodes and libraries (`gui`, `gui_hdr`,
  `transparent_mask`, `gbuffer_material`, `shadow_caster`, `opaque_forward`,
  `wireframe`, `emissive`, `hdr_transparent`, `decal_*`, `filter_*`, …). These
  are the **renderer's own layer list**: `renderer.render_config` declares them
  by name (`{ name="gui_hdr" render_targets=["hdr0"] sort="BACK_FRONT" }`,
  `{ name="transparent_mask" render_targets=["overlay_mask"] … }`).
- **defines** - the `<name>` of an `editor_options` entry
  (`{ name="One Bit Alpha" define="ONE_BIT_ALPHA" }`), a pass's `defines`, a
  `static_compile`'s `defines`, or an `#if defined(...)` the code tests.
- **context names** - `default`, `shadow_caster`, `gui_render_pass`, … The
  context occupies the position right after `<shader>` in the query key; the
  library name drops it.

## Engine constants vs. shader-specific tokens

This is the part worth keeping straight when generating a key set.

**Engine-constant (same across every shader):**

- the **platform** and **renderer** tail - on the target these are literally
  `PLATFORM_WIN32:RENDERER_D3D12` (`permutations_contexts` passes `"WIN32"` and
  `"D3D12"`; the engine binary carries `PLATFORM_WIN32/PS4/XB1/XB12/LINUX` next
  to the render-profile enum);
- the **key format** itself (separators, sorting, uppercase, the murmur64A
  seed);
- the **renderer's layer and render-state vocabulary** - because it is the
  engine's list, the same names recur across shaders and versions. It is the one
  part of the vocabulary that can be carried as a toolchain table rather than
  read from each shader.

**Shader-specific:**

- the **shader name** (the declaration's own name / material path);
- the **contexts** the shader declares;
- the **defines** and the **`#if` conditions** its code tests - these are the
  shader's own permutations;
- which **layers** it emits (a GUI shader emits `gui`/`gui_hdr`/
  `transparent_mask`; a standard shader emits `gbuffer`/`shadow_caster`/…).

So a from-scratch declaration generates its key set from *its own* shader name,
contexts, defines and `#if` conditions, plus the engine's platform/renderer tail
and (for the layer tokens) the engine's render-state vocabulary.

## The Darktide group-tail hashes decode as these tokens

The group `tail` pool a section carries is the same vocabulary: a group's tail is
its **condition set**, and its hashes are layer/render-state names. Verified by
hashing the SDK's layer vocabulary with the engine's Murmur32 (`condition_reverse`
against the 115-name list built from `core/stingray_renderer/**`):

| hash | name | source |
| --- | --- | --- |
| `9FCFE126` | `gui` | `layer="gui"` (gui/unlit output nodes) |
| `BC4EE226` | `gui_hdr` | `{ name="gui_hdr" }` in `renderer.render_config` |
| `625D415E` | `transparent_mask` | `{ name="transparent_mask" }` in `renderer.render_config` |
| `BDF72706` | `gui_mask` | `LAYER_GUI_MASK` → the `transparent_mask` pass |
| `F2760503` | `default` | the context name |
| `E2C8865F` | `gui_render_pass` | the context name (Darktide-only) |
| `3100C3D2` | `shadow_caster` | a context / render state |
| `7F9E89FD` | `num_skin_weights` | a `permutation_sets` predicate |

The UI base's six tails are therefore the condition sets
`{gui, transparent_mask}`, `{gui_hdr, transparent_mask}`, `{transparent_mask}`,
`{gui_hdr}`, `{gui}` and the empty set - the GUI shader's own branches, not
opaque engine data.

Four hashes in the current pool are **still unnamed**: `26E1CF9F`, `26E24EBC`,
`5E415D62` and `B5F45768`. They are not plain members of the SDK's 115-name
layer vocabulary, nor of the ~882k-name dictionary, so they are either a
Darktide-only layer (a newer engine has more), or a composed/instanced name (the
way `texture_map_<hash>` and `material_variable_<hash>` are). That is the
remaining naming bounty for this pool.

## The section structures these tokens key

- **query ids** - `murmur64A(key) >> 32`; the section's contexts carry one per
  query, and the group data has one group per query. For a from-scratch
  declaration the engine demands a specific id (`6FA3FCCF` for
  `materials/mods/snoopymod/ui_default_base:SINGLE:PLATFORM_WIN32:RENDERER_D3D12`
  - verified in game).
- **group tail** - the group's condition set: `u32 n`, then `n` 17-byte entries
  (`condition hash` + 13 bytes) and an 8-byte trailer. Now written as the hash
  list `{ conditions = [ "9FCFE126" "625D415E" ] }`. It is **load-bearing even
  with an empty conditions tree** (zeroing it renders `RENDER_BLACK`), which is
  why it is the one pool still carried: deriving it means deriving the shader's
  own condition graph, not an engine constant.
- **the conditions tree** - the bytecode over the same hashes, mapping a
  material's channel/flag subset to a group. Only needed for permuting shaders
  (the UI base's material ships an empty tree).

## What this means for generation

A key set is fully generated when the declaration is: name + contexts + defines +
`#if` conditions, with the platform/renderer tail and the layer vocabulary as
toolchain constants. The gap is that our `ui_default_base` declaration declares
one pass (`SINGLE`) while the section it builds is the inherited UI-base family's
36-query / 6-tail shape - so the plan derives from the declaration, but the
**tail pool** and the family's program table still come from the shipped family.
Widening the declaration to the family's passes and conditions (the enumerator,
fed by the token sources above) is what lets the tail pool and the program table
derive too, after which nothing but the declaration and the sources is needed.
