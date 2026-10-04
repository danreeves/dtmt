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
- the **hash convention** (§ below) - a name is hashed to its id the same way
  everywhere, and that is the only truly fixed part of the vocabulary.

**Shader-specific (author-declared):**

- the **shader name** (the declaration's own name / material path);
- the **contexts** the shader declares;
- the **defines** and the **`#if` conditions** its code tests - these are the
  shader's own permutations;
- the pass's **`layer`** and **`render_state`** values - see the correction
  below.

### Correction: `layer`/`render_state` are author-declared, not engine data

An earlier reading of this doc called the layer/render-state vocabulary an engine
constant to be carried as a toolchain table. That is wrong in the part that
matters, and the distinction changes the derivation:

- `renderer.render_config` is where the **renderer's valid layer names are
  defined** (`{ name="gui" render_targets=["back_buffer"] sort="BACK_FRONT" }`,
  `{ name="transparent_mask" render_targets=["overlay_mask"] … }`). It ships in
  the SDK only; **Darktide ships no render_config**, so we cannot validate a
  name against the engine's list, and must not treat the values as constants.
- A `.shader_source`'s `layer="gui"` is a **use** of a name, not a definition.
  The name is whatever the shader author wrote; the compiler hashes it into the
  pass and the tail.
- The pass line in our own declaration is therefore the source:
  `{ layer="default" code_block="ui_default_base" render_state="default"
  defines=["SINGLE"] }`. `layer`/`render_state`/`defines` are **ours to
  declare**, and their names are hashed by the engine's convention exactly like
  the VT2 names.

So a from-scratch declaration generates its key set - and its condition set -
from *its own* shader name, contexts, defines, `#if` conditions, `layer` and
`render_state`, plus the engine's platform/renderer tail. The only constant is
how a name is hashed.

### The hash convention

Every name in these structures - contexts, defines, layers, conditions - is a
32-bit id computed the same way: **MurmurHash32 over the lowercase name**, i.e.
`high32(murmur64A(name))` (`Murmur32::hash` in the SDK, `9FCFE126` = `gui`,
`F2760503` = `default`). The section's **queries** are different: they are
`high32(murmur64A(full query key))` (the whole
`<shader>:<context>:<defines sorted>:PLATFORM…:RENDERER…` string), which is why a
query id is never a plain name hash.

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
`#if` conditions + the passes' `layer`/`render_state`, plus the engine's
platform/renderer tail. The gap is that our `ui_default_base` declaration declares
one pass (`SINGLE`) while the section it builds is the inherited UI-base family's
36-query / 6-tail shape - so the plan derives from the declaration, but the
**tail pool** and the family's program table still come from the shipped family.
Widening the declaration to the family's passes and conditions (the enumerator,
fed by the token sources above) is what lets the tail pool and the program table
derive too, after which nothing but the declaration and the sources is needed.

### Deriving the group tail from the declaration

The group `tail` is a condition set over the same name hashes. A pass already
names its conditions in the declaration:

```
{ layer="default" code_block="ui_default_base" render_state="default" defines=["SINGLE"] }
```

The derivation is therefore:

1. **One condition per pass** - the pass's `layer` and `render_state` (and,
   when they are part of the permutation, its `defines`). Each name is hashed
   with `Murmur32` (`murmer32(lowercase(name))`), the same convention as every
   other name in the tail.
2. **The tail bytes** - `u32 n`, then `n` 17-byte entries
   (`{u32 hash} + 13 zero bytes`), then an 8-byte trailer (`01` when the set is
   nonempty, zeros when it is the empty set). This is exactly
   `GroupTailText::Conditions` in `shader_engine_data.rs`, so the file would
   carry a pass's `layer`/`render_state` names rather than opaque hashes, and the
   build hashes them.
3. **The family's set** - the UI base's six tails are the GUI shader's branches
   (`{gui, transparent_mask}`, `{gui_hdr, transparent_mask}`, `{transparent_mask}`,
   `{gui_hdr}`, `{gui}`, empty). To generate them, the declaration must carry the
   same branches: its passes need the `layer`/`render_state` values the shipped
   shader used, which is the enumerator's job (declare the family's passes and
   their conditions). With one `SINGLE` pass the derived pool is one tail, which
   does not match the family's six - the honest reason the pool is still carried.

This is the concrete path: extend `CompileJob`/`PassValue` to carry
`layer`/`render_state`, hash them into `GroupTailText`, and widen
`ui_default_base.shader_node` to declare the family's passes. Until then the
288-byte pool stays.

**Landed (2026-10-04).** The first half is in: `CompileJob` now carries the
pass's `layer` and `render_state`, and `CompileJob::condition_ids()` hashes
`layer` + `render_state` + macros with `Murmur32`. A group tail may be written
`{ context = "default" pass = 0 }`; `EngineData::resolve_pass_tails` (called by
`dtmt build` with the declaration's jobs) turns it into the condition set. Test:
`a_pass_tail_resolves_from_the_declaration` (asserts `gui`/`gui_hdr`/`SINGLE` →
`9FCFE126`/`BC4EE226`/`407AB909`). The second half - declaring the family's 48
passes in `ui_default_base.shader_node` - is what would let the six-set pool
derive; until then the shipped pool is still used.

**The UI base's six tails derive exactly (2026-10-04).** Read off the shipped
section (`program_order`, `conditions_dump`), the six group tails are the
condition sets `{gui, transparent_mask}`, `{gui_hdr, transparent_mask}`,
`{transparent_mask}`, `{gui_hdr}`, `{gui}` and empty - and each is one pass's
`(layer, render_state)` pair:

| tail | pass |
| --- | --- |
| `{gui, transparent_mask}` | `layer="gui" render_state="transparent_mask"` |
| `{gui_hdr, transparent_mask}` | `layer="gui_hdr" render_state="transparent_mask"` |
| `{transparent_mask}` | `layer="transparent_mask"` |
| `{gui_hdr}` | `layer="gui_hdr"` |
| `{gui}` | `layer="gui"` |
| empty | (no layer/state) |

So deriving the pool needs only these six passes in the declaration, and
`condition_ids()` reproduces the exact bytes (pinned by
`the_ui_base_familys_six_tails_derive_from_six_passes`). No engine data is
needed for the tail pool.

**What still blocks using it (2026-10-04).** Two things, both the enumerator:

1. `dtmt build`'s declaration flow refuses a declaration with more than one
   compile job (`compile_declaration`: "must have exactly one pass for now"),
   because the override flow names one container per stage. Six passes means six
   `(Vertex, Pixel)` program pairs, which the single-container seam cannot carry.
2. The 48 program slots' **channel masks** and the 48 passes' mapping to the 36
   groups are the family's own; the mask values (`01/02/04/08/0F`) cycle in the
   section but the program-to-group mapping is not decoded (the docs' open item).
   A declaration can name the passes, but pairing them with the section's 48
   program slots also needs that mapping.

So the honest state: the **tail pool is decoded and generatable**; the **program
table still needs the multi-job flow and the program-to-group mapping**. The
288-byte file carries the tail pool because both are one step behind.

**Attempted end to end (2026-10-04).** The mechanism was wired: `compile_
declaration` now accepts a multi-pass declaration (compiling the first pass's
container, keeping every job for the derived tables), and `EngineData::
derive_tails` builds the group list and the six tails from the declaration's
passes when the file carries none. With `ui_default_base.shader_node` declaring
the six passes and the engine data reduced to `group_template = { repeat = 6 }`,
`dtmt build` **generated a section with the exact six tails** (the group data's
condition sets match), but the query/group structure came out **2 queries, not
the family's 36**. The reason is the last unknown: the family's 36 queries are a
**channel permutation** (the `red`/`green`/`blue`/`alpha` / `write_channels`
axes the condition records test), which the declaration does not yet express as
an `editor_options`/`permutation_sets` construct. So the tails derive, the query
enumeration does not. The mod was restored to its shipped pool (still
byte-identical, `F3EE2651139FDEB0`); the wiring stays in the toolchain for when
the channel permutation is declared too.

**The fix that came out of it:** a pass's macros are **not** part of its group
condition set - they build the query key and the channel mask. `conditions()` is
now layer + render_state only, so pass 0 (`defines=["SINGLE"]`) derives
`{gui, transparent_mask}`, not `{gui, transparent_mask, SINGLE}`.

**The 36 queries are a permutation product, and it enumerates (2026-10-04).**
Reading the shipped section's contexts (`conditions_dump`), the 36 queries across
the two contexts decompose as a product of axes:

- **layer** - `{gui}`, `{gui_hdr}`, neither (3);
- **`gui_mask`** - `BDF72706` present or not (2);
- **channels** - `{red, green, blue}`, `{red}`, `{green}`, `{blue}`, `{alpha}` (5),
  with `write_channels` (`8FB860CF`) marking the branch;

so `default` = 3 x 2 x 5 = **30** and `gui_render_pass` = 2 x 3 = **6**. Declared
as `permutation_sets` (`layers`, `gui_mask`, `channels`,
`render_pass_channels`) with a `compile_with` per context, `compile_jobs`
enumerates to exactly **36** - the family's count - with no engine data
(`the_ui_base_family_enumerates_to_its_36_queries`). So both halves are now
expressible: the **36 queries** (permutation sets) and the **6 tails** (pass
layer/render_state).

**The one modelling gap left:** the group tail's layer must vary *per
permutation*, but a pass's `layer` is static and the permutation sets vary only
macros. In VT2 the layer selection is the pass tree's job (`{ if:
"defined(LAYER_GUI)" then: [{ layer="gui" ... }] }`), so 36 queries come from 36
passes selected by condition - not one pass over 36 permutations. Reproducing the
family exactly therefore means declaring the **pass tree** (a pass per layer/
channel branch), which is mechanical but larger: it is generating the 48-pass
declaration from the section, not hand-writing it. That is the last step before
`.engine_data` can go.

**The generator exists and the section parses, but does not render
(2026-10-04).** `examples/family_declaration.rs` reads a shipped section and
emits exactly this declaration: the distinct group condition sets become passes
(hashes named against the engine vocabulary), and a `channels` permutation
crossed with each pass's own define enumerates to the section's query count. On
the shipped UI base it produces **6 passes x 6 channels = 36 jobs and 36 distinct
query ids**, with the six condition sets. `EngineData::derive_tails` now writes
the group count into the template prefix, so `group_template = { repeat = 6 }`
alone generates a section with 36 queries and 36 groups - the family's shape,
with no engine data.

**In game it crashes.** The generated section (421,684 B) hits an access
violation at the title screen - with no `ShaderTemplate` or `dispatch_loadtime`
context, unlike the known failures - while the restored shipped-pool build is
`RENDER_OK` (`F3EE2651139FDEB0`). So the **tails and the query count derive**, but
the section is not yet valid: the generator synthesizes each group's **head and
descriptor list** as zeros, and those are the family's own compiled interface
(four descriptors, `X` offsets, `Y` usage counts). Deriving them is the next
piece; until then the head/descriptor part is what an engine data file still has
to carry even with the tails derived.

**The crash was the header's context count - fixed, and the file is deleted
(2026-10-04).** The section header's `context_count` is the number of context
records, not the number of queries: `generate` wrote the query total (36) where
the shipped section writes 2 (its two contexts). One line fixed it -
`plans.len()` instead of the query sum - and the generated 36-query section then
**rendered in game** (`RENDER_OK`). The group heads were fine after all;
`derived_head` already builds the family's four descriptors.

With that, the whole chain derives:

- **`group_template = { repeat = 6 }` (34 bytes) built the section, and an
  *empty* engine data file built the *same* section** (`CD9854D69AC9A5B7`);
- `derive_tails` now also builds the template when the file carries none, taking
  `repeat` from the **query count over the tail count** (36 / 6 = 6);
- `dtmt build` generates from a **declaration alone**: a material with a sibling
  `.shader_node` and no `shader_engine_data` runs the same derive + generate flow
  on an empty engine data.

**Result: `.engine_data` is deleted and the shader renders.** snoopy-mod's
shader is now `ui_default_base.material` + `.shader_node` + `.shader_source` -
the goal at the top of `Shader RE TODO.md`. Verified in game with the file
absent: `RENDER_OK`, section `CD9854D69AC9A5B7`, 421,684 B, no material-set
crash. The one thing that was never engine-derived, the truth of it: even this
path still relies on the engine's render-state vocabulary in the *names*
(`gui`, `gui_hdr`, `transparent_mask`) - but those are names the declaration
writes, so the mod carries no engine data.
### Deriving the program table too

The family's 48-pixel program table is the pass list crossed with the
permutations, each slot's `(head, seam, mask)`. Once the declaration enumerates
the passes (each with its `layer`, `render_state` and `defines`), the table is
that enumeration - the mask is the pass's channel set (its defines), the head and
seam the family's, which are recorded per family. So the same widening that
generates the tail pool generates the program table.

**The program table is not the query list, and its block header is load-bearing
(2026-10-04, in-game).** `derived_programs` was changed to emit one
`(Vertex, Pixel)` pair per query, the pixel block a minimal 32-byte header plus
the preamble body, so the list would come from the declaration alone. On the UI
base this changed the section from 96 to 72 programs (421684 -> 333252 B,
`50FF4BBFA3B56D66`) and the engine **crashed** with an access violation in
`dispatch_loadtime` at `StateTitle` (no material-set line; the restored
hardcoded-table build is `RENDER_OK`, `CD9854D69AC9A5B7`). A second build with
the declaration widened to 48 queries (48 pairs, derived write-channel masks)
also crashed at `dispatch_loadtime`. Isolating the header: keeping the shipped
48-program list and the shipped masks but forcing every block's header to the
minimal 32-byte `{02}` form (`BA082DE84BE11755`, 421556 B) **also crashes**. So:

- the program list is the engine's **compiled variant list**, independent of the
  query count (the working section has 36 queries but 48 program pairs);
- the 36-byte header's **seam word is read by the engine**: a uniform minimal
  header corrupts it, so a generated program cannot fabricate the header.

Deriving the program list therefore needs the per-program **block header
grammar**, not a count or a mask rule. The table the toolchain still walks
(`UI_BASE_PASS_HEADS/SEAMS/MASKS`) is the shipped UI-base family's: 48 passes,
`head` 36 for 35 of them, `seam` 2 for the first twelve 36-byte heads and 1 for
the rest (the 32-byte ones, and the last, carry 0). What the seam selects is
undecoded; `docs/Shader RE TODO.md`'s "block synthesis" item is the same
grammar.

