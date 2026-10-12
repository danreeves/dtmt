# Shader RE TODO

Working notes and next steps for the `shader43` reverse engineering. See
`File Type - Material.-.md` (field-level findings),
`File Type - Shader Node.-.md` and `File Type - Shader Source.-.md` (the two
authoring source formats),
`Shader Section Generation Notes.md` (the target and what each section needs) and
`Shader Permutations.md` (what a permutation key may contain, and which tokens
are engine constants). `TODO.md` is the project-wide umbrella.

**Community reference.** `C:\dev\Darktide-Asset-Compiler`
(<https://github.com/fviuff/Darktide-Asset-Compiler>) writes the same formats
and is updated often. Its `material/shader_section.*` decodes/encodes a shipped
shader section exactly (version `0x2b`) and `material/shader_compiler.*`
documents the group **root table** - the order a group's distinct
cbuffer/resource names are first bound across its passes and stages - which is
the numbering a tail's resource records index. Re-check it as we take each list.

## Priority order

**Current goal: delete `.engine_data` entirely** - a `.material` + `.shader_node`
+ `.shader_source` + `.texture` that builds a section from scratch, with no
`shader_engine_data = "..."` side file. **DONE (2026-10-04):** snoopy-mod's
shader is now exactly that tree - `ui_default_base.material` + `.shader_node` +
`.shader_source`, no engine data - and it **`RENDER_OK`** in game (section
`CD9854D69AC9A5B7`, 421,684 B, generated from the declaration alone). The
section's whole shape derives from the declaration: the permutation plan and
`context_count`, contexts, material table, group head, `between`/`mid`, preamble,
programs, tails, block heads, the group list, and the group tail pool (six passes'
`layer`/`render_state`). See "The group tail derives" and "The permutation plan
derives" below, and `Shader Permutations.md` for what a key may contain.

The road, in order: 649 -> 429 (tails as condition sets) -> 406 (derived
`prefix`) -> 387 (derived `context_count`) -> 288 (plan from the declaration) ->
**0 bytes** (tails and template from the declaration; the file deleted).

**The standard family's tails derive too (2026-10-12).** Its engine data is the
last one that carried resource lists; the split derivation now reproduces them
(see "The tail's rest is nine counted lists"), so `examples/split_tails.rs`
dropped every program's lists and the file went **46,075 -> 42,091 bytes** with
the built section **byte-identical** (`578c983197da6b91`). The file now carries
only the per-program device blocks. The UI-base family already shipped with no
engine data.

**The blocks now share their body and tail (2026-10-12).** Every pixel block is
`head + the preamble body + trailing`. The middle - the body's records - is
verbatim and in order in every pixel block, so `block_text` now stores the
**longest run of the preamble body a block carries** as a *reference*
(`body_start`/`body_len`) plus the bytes that differ as patches, and the bytes
after the run as a pooled `trailing` index (a permutation's vertex and pixel
blocks repeat a handful of trailings). Together that took the file from
**42,091 -> 16,921 bytes** (a 60% cut from the capture) with the built section
byte-identical. The body needs no new pool: the preamble already carries it.

**The engine table is carried as a diff (2026-10-12).** The group template's
engine table is the toolchain's captured table (`data/global_viewport.hex`) with
its **last four records dropped** - every common record matches the toolchain's in
kind, offset and size, and the four are the toolchain table's last four. So
`GroupTemplateText` writes `engine_drop = 4` (the count of trailing records the
file drops) instead of the 2.7 KB table, taking the file from **16,921 ->
14,165 bytes**, byte-identical. It is a diff, not a derivation: the engine table
is engine-side data whose records come from the game build.

**What the block's trailing is (2026-10-12).** `examples/trailing_probe.rs` dumps
a pixel block's head, its run of the body's records and the trailing after them as
`u32` words. What stands out:

- the trailing opens with the **same six words in every pixel program**
  (`0000000E 00000400 00000000 00000D00 00010000 00000000`), so it is not purely
  per-shader;
- at the byte level it opens with a run of **12-byte records** whose second byte
  counts up (`5E 5F 60 61`), then switches to a longer record type - so the framing
  is irregular (the trailing lengths are odd: 379/353/249/828/155), not a plain
  `u32 count` + fixed records;
- it closes with runs of a repeated ~15-word record that carries a **type word and
  a hash** - and the hashes are the group's **channel** hashes (`8CCCAD2F`,
  `9C075586`, `F3955EB4`), the same ones the `mid` derives from. So the trailing
  looks like the per-program **resource-binding shadow** and is a candidate to
  derive from the channel list plus the head, but the record grammar is not yet
  decoded.

The block's **head** is a distinct structure, not the group head: program 1's (104
B) is `{2, 0, 4, 2, 0x100, 0, 0x40, 3, FFFFFFFF, 0x40, 0, 4, FFFFFFFF, 0x40, 0, 2,
0xC0, 0x40, 0x40, 0x80, 0x58, 0, 0, 0, 42, 30}` and program 3's (84 B) ends `{...,
0x50, 0x58, 0, 0, 0, 43, 30}`. The **last word is always 30** - the body's record
count - and the word before it varies per program (42, 43, 35, 38, 48, ...), so the
head trails `{n, record_count}` much like the group head. The leading words hold
per-program resource records (`0x40` and `0xFFFFFFFF` recur); decoding them is the
remaining work for deriving the blocks.

A block is **not** a rearrangement of the preamble: the longest run it shares with
the preamble is the 393-byte record run (the body), and the whole block is not a
substring of it. So the head and the trailing are per-program data, not slices of
the engine block.

**A program's group derives (2026-10-12).** `derive_program_groups` already fills
the program-to-group mapping from the group tails' condition counts; `to_text` now
omits each program's `group` tag when it equals what that rule produces (verified
on the standard: group `i` owns `conditions(i) × 2` consecutive programs). Took the
file from **14,165 -> 13,801 bytes**, byte-identical.

**The trailings pool as shared-suffix chains (2026-10-12).** The seven distinct
trailings share long **suffixes** (up to 196 bytes - the per-program resource
runs), so the pool now stores each as its unique prefix plus a reference to an
earlier entry's suffix (`tail = { node = 1 start = 162 }`). The text can no longer
express this as a plain hex list, so an older file re-migrates through
`to_text` (the intermediate `Vec<Hex>` form is not read back). Took the file from
**13,801 -> 12,911 bytes**, byte-identical.

**The blocks' head and trailing pools share fronts and backs (2026-10-12).** The
same pool (`BytesPool` / `ChunkText`) now serves the block **heads** too, which
share long **fronts** (up to 60 bytes) - a value is written as its own middle bytes
with an optional `head = { node, len }` front and `tail = { node, start }` back. A
shared run is only referenced when it is at least 24 bytes (below that the fields
cost more than the bytes they save). A plain blob still parses (`ChunkText` is an
untagged `Plain(Hex) | Parts`), so an older pool migrates through `to_text`. Took
the file from **12,911 -> 12,546 bytes**, byte-identical.

**The block body slices pool too (2026-10-12).** Twelve of the standard's fourteen
tails carry the same `(start = 1, len = 393)` slice of the preamble body, so the
slice is a small `block_bodies` pool (`BodyText { start, len }`) the blocks index;
the whole body is still the default (no index). Took the file from
**12,546 -> 12,266 bytes**, byte-identical.

**Zero-valued indices are omitted (2026-10-12).** The group template's groups and
the program list wrote every index even when it was the default `0` (`head = 0`,
`tail = 0`, ...); they now skip it. Took the file from **12,266 -> 11,870 bytes**,
byte-identical.

**Pool references are compact, and two results (2026-10-12).** A shared front/back
reference is written `head = "2:60"` (`node:len` / `node:start`) rather than a
four-line table - **11,870 -> 11,552 bytes**, byte-identical. Two things that look
derivable:

- **the group heads** (the bulk of the group template) - they do derive, after a
  real fix: `head_descriptors` hashed the raw dxc binding name, but the resource
  *lists* hash the engine's spelling (dxc's combined-sampler `__tex_` / `__samp_`
  prefixes stripped, [`engine_name`]). With the two made consistent, clearing the
  heads rebuilds the section **byte-identically**, so `to_text` now omits a head
  pool whose entries are all query-only and `mid_migrate` truncates each group's
  head to its query. Took the file from **11,552 -> 9,905 bytes**, byte-identical;
- **a program's `job`**: the build path does not read `program_jobs`, but removing
  the tags changes the built section's descriptor lists, so it is needed.

The shared-front/back pool is near-optimal for the trailings: a pairwise
longest-common-substring check (max 196 B, and every match a prefix or suffix)
shows no long **middle** run left unshared, so the pool needs no third reference.

**The head's descriptor flags are the engine's, not dxc's kinds (2026-10-12).**
Dropping the carried heads and comparing the built material file against the
shipped one (`ground-truth-material`) showed the derived heads differed in 7 bytes
- each a descriptor's flags, where a texture was written as its dxc kind `2`
instead of the engine's `3`. `engine_kind_flags` maps the kinds (texture `2 -> 3`,
the structured SRVs `1/5/7 -> 4`, the UAVs to `5`/`7`). With that, the built file
matches the shipped one except **two pre-existing bytes** (at group-data offsets
10732 and 10924: a material record's `80`/`96` where we write `72`/`88`) - those
were there before the heads derived and are a separate, still-open fidelity gap.

**The `between` derives from the engine table (2026-10-12).** The docs called
`between` "two constants then the count word"; the first word is actually the
engine table's **byte size** (`table_size`, e.g. `0x6F0` = 1776 for the
standard's 69 records) and the second is zero - the toolchain's `derived_between`
(`{0xF0, 0x40, 0}`) was wrong, so the standard carried its betweens. Deriving
`{table_size(engine), 0, 0}` and letting `GroupData::build` rewrite the count
reproduces the shipped bytes exactly, so `to_text` omits an all-empty `between`
pool and `mid_migrate` clears each group's between. Took the file from
**9,905 -> 9,763 bytes**. (`GroupData::build` rewriting the count still needs
checking against a family whose count differs. The UI base - a declaration-only
family with no engine data - also derives its betweens, and so now carries the
engine-table size too instead of the old hardcoded `{0xF0, 0x40, 0}`; the pumpkin
test still renders.)

**A repeating group list is a pool plus a pattern (2026-10-12).** The standard's
five groups hold only three distinct specs, so `to_text` writes the distinct specs
and a `pattern` of indices when that is smaller (`group_pattern = [0 0 1 2 2]`),
and the reader expands through it. Took the file from **9,763 -> 9,651 bytes**.

**Two carried pieces that do not derive (2026-10-12).** Exercise both by hand:

- the **preamble** (985 B, 10% of the file): deleting it and rebuilding changes the
  section by 128 KB - the shipped preamble is a *different* engine block from the
  toolchain's `shader_engine_block` template, so it is carried, not regenerated;
- a program's **`job`**: deleting the tags rebuilds a section **127 KB smaller**
  (the build compiles the declaration's jobs and frames each program with its
  container), so it is load-bearing despite the build path not reading
  `program_jobs` directly;
- the **material tables** (554 B): deleting the `materials` pool and rebuilding
  gives a section **112 bytes larger** whose body differs throughout (not a plain
  insertion), so the derived table is not the shipped one and the pool is carried.

**Stage names are unquoted, and shorter shares pay (2026-10-12).** `serde_sjson`
writes a `String` raw, so the `serialize_quoted` wrapper on a program's stage was
what forced `"Vertex"`; dropping it writes the SJSON-idiomatic `Vertex` (and the
old helper is gone). The shared-front/back threshold (`MIN_SHARED`) was over
conservative at 24 - a compact `head = "2:60"` reference costs about a line, so a
12-byte share already pays. Took the file from **9,651 -> 9,578 bytes**.

**The group head's layout is pinned (2026-10-12).** Reading a shipped head as
`u32` words shows the exact shape: `{header, descriptor_count, {name, flags, x, y} x
count, {cbuffer_count, engine_record_count}}`. Group 0's 208 bytes are
`{0x390, 12, ...}` with twelve descriptors (`global_viewport` `0x516D5CCD` flags
`0x0101`, `c_per_object` `0xB5639618` 0, `bones` `0x18DEAD01` `0x00400004` with
`y = 0x155`, ...), then `{5, 69}` - five cbuffer descriptors and the engine table's
69 records. That is what `derived_head_for_group` rebuilds. The **block** head is a
*different* table (program 1's 104 bytes start `{2, 0, 4, 2, 0x100, ...}` and trail
`{42, 30}`), so the group derivation does not cover it - see above.

**Next step: the enumerator for *any* family.** Today the six passes are declared
by hand for the UI-base family; `examples/family_declaration.rs` generates such a
declaration from a shipped section, and the same enumeration would build other
families' declarations. The tooling is `lib/sdk/examples/vt2_editor.rs` and
`vt2_enumerate.rs`.

**The material table derives from the declaration's inputs, and the group→tail
assignment is carried (2026-10-07).** The last two carried pieces of a
declaration-only build have a text path:

- **`#hash` inputs.** A declaration's `inputs` may be keyed by a raw `#<hex>`
  hash (`"#78E25D0F" = { type = "vector4" }`), the spelling a pass's raw
  `conditions` already uses, so a `c_per_object` record the dictionary cannot
  name still round-trips. `shader_node::name_id` hashes a plain name with
  `Murmur32` and reads a `#<hex>` name as the id itself; `--enumerate` and
  `--reconstruct` now write every object-table record (named where the
  dictionary can, `#<hex>` otherwise) instead of dropping the unnamed ones.
- **The material table from the inputs.** `material_records` no longer needs a
  graph: for a plain declaration it lays out the engine's three matrices, the
  code block's texture slots (the `samplers` triple) and the declaration's
  `inputs` in a running constant-buffer layout (`pack`: an element takes the
  next offset that does not straddle a 16-byte row, HLSL's own `c_per_object`
  packing). The compiled `c_per_object` cbuffer's size is the check - a derived
  table wider than it warns. The graph path is unchanged (the UI base's table
  is still the shipped 7 rows).
- **The group→tail assignment.** A pass may carry `group = <n>`, written by
  `--enumerate` from the program→group mapping. `EngineData::derive_tails` then
  builds **one group per distinct index**, in first-appearance order, with that
  group's condition set as its tail, instead of a uniform `repeat` - which is
  what a family whose passes outnumber its groups needs (the standard family's
  11 passes over 5 groups). A declaration that names no group keeps the
  repeating model, so the UI base's shape is unchanged.

**Caveat, not yet in game.** The offsets are *our* packing, not a read of the
compiled `c_per_object` (a reconstructed source's cbuffer is a flat array, so
its reflection gives only the total). It matches the shipped UI base's 7 rows,
but a family's own table can be wider than the derived one - the standard
declaration derives **192 bytes** against its compiled **416** - so the values
may land at the wrong slots until the layout is checked in game. The goal is a
build derived from the text sources, not a byte-exact one.

**In game (2026-10-07).** The title harness reports `RENDER_OK` on the unchanged
UI-base section (`CD9854D69AC9A5B7`), and `docs/scripts/pumpkin-render-test.ps1`
reports `PUMPKIN_OK` for the standard material: snoopy-mod's `init.lua` reaches
the hub, spawns the pumpkin and quits with `confirmed=true`; the only `<<Crash>>`
is the benign `refcount == 0` unload. Two things were fixed after the first run:
the pumpkin's off-screen placement was `init.lua`'s spawn offset (the
`Vector3(0,0,2.5)` moved it along the wrong axis; it now spawns at
`position + Quaternion.up(rotation) * 1.0`), not the shader - and the recompiled
standard programs' resources match the true shipped `dump-cmp/shipped` containers
exactly. A clean pre-change vs post-change `dtmt build` differs only in
`files.sjson` (the reordering listing).

**The standard family's `containers` are dropped (2026-10-07).** A program may
name the declaration's compile job (`job = <n>`) instead of a carried container
(`container = <n>`); `build.rs` compiles the job and frames the program with the
job's `(vertex, pixel)` container (the `ShaderOverrides::job_containers` list).
`lib/sdk/examples/container_jobs.rs` migrates a captured file: it compiles the
declaration's jobs against the `.shader_source` libraries, matches each program's
carried container by interface (26/26), and writes the `job` form with no
`containers` hex. snoopy-mod's `standard_base.engine_data` went **375,410 ->
51,936 bytes** (the containers were 86% of the file), the built section is
**byte-identical** (`578c983197da6b91`, `4cc21b79457af061`), and the pumpkin
renders in game (`PUMPKIN_OK`).

**The `tails` now derive too (2026-10-12).** The carried tails used to index
their group's descriptor list, which the standard's groups disagree on, and the
reflection-derived lists used the raw HLSL names and the wrong record layout, so
a split section crashed. Both are fixed: `set_derived_lists` follows the
engine's name rule (`__tex_`/`__samp_` stripped) and record shapes,
`derived_cbuffers` takes `c_per_object`'s size from the compiled container (the
group's material table is narrower), and `derive_program_groups` fills the
program-to-group mapping from the engine's `2 x n` rule when the file carries no
`group` tag. The env-gated test `the_derived_lists_match_a_built_section`
reports **0 mismatches** for all 26 programs' constant-buffer prefixes and
resource lists. `examples/split_tails.rs` rewrites the file (`from_text` ->
`to_text`), dropping each derivable tail to its block; snoopy-mod's
`standard_base.engine_data` went **46,075 -> 42,091 bytes** and the built section
is **byte-identical** (`578c983197da6b91`, 145,604 B). So the file now carries
only the per-program device blocks, no resource lists at all.

**But the enumerator alone is not enough: the program table's block header is
load-bearing (2026-10-04, in-game).** Making `derived_programs` emit one program
pair per query (72 programs) crashes at `dispatch_loadtime`; keeping the shipped
48-program list but forcing a uniform minimal 32-byte block header **also**
crashes. So the list is the engine's compiled variant list and the 36-byte
header's seam word is read by the engine - the per-program block-header grammar
must be decoded before a section's programs can be generated. See
`Shader Permutations.md`, "Deriving the program table too".

**The VT2 SDK is the writer's ground truth (2026-10-04).** The official SDK at
`C:\Program Files (x86)\Steam\steamapps\common\Vermintide 2 SDK` ships, under
`TEMP/streamable_resources_compile/`, the output of a full compile the toolchain
itself performed: `data/` holds **250 compiled `.shader_library` files** (the
VT2 device data, not the `shader43` section - VT2's format is a plain
`DXBC` + tail stream), the `debug_file_index.sjson` names every one
(`<shader>:<tokens>.shader_library`), and `shader_cache.db` is the compiler's
manifest. So this is the writer's own artifact set to diff against.

The dev binaries name the types and the writer. `strings` on
`sdk/engine/win64/dev/stingray_win64_dev_x64_compiler_1.exe` and
`bin/stingray_win64_dev_x64.exe` shows (RTTI/assert strings, no PDB is shipped):

- **`stingray::shader_compiler::make_device_data`** - the function that writes
  the device data (`platform_interface.cpp`), i.e. the byte stream after each
  program's compiled container;
- the device-data type is **`stingray::D3DShaderPassData`** and its parts are
  named exactly: `D3DShaderPassData::Shader::{ConstantBufferBindInfo,
  ResourceBindInfo, SamplerBindInfo}`, `D3DShaderPassData::StreamOutDeclaration::Channel`,
  `D3DShaderPassData::InstanceData::Variable`, `D3DShaderPassData::Shader`, and
  `Vector<D3DShaderPassData>` - the same split as this project's group head
  (cbuffer/resource descriptors), the tail lists (samplers, stream-out) and the
  block (the per-program device records). `D3DShaderPass::init` /
  `initialize_shader_program` (`d3d_shader.cpp`) is the D3D11-side reader;
- `shader_compiler::ShaderLibrary::Shader::Pass`, `StateBlock` and
  `StateVariable` (the pass and render-state tables), `reflect_cbuffer`,
  `validate_cbuffer_reflection`, `PlatformD3D12::compile/convert_shader/convert_pass`;
- the source paths name the modules:
  `runtime/application/rendering/shader_compiler/{platform_interface,platform_d3d12,platform_d3d11,library_compiler,preprocessor}.cpp`.

The names give the structure; the field offsets are not in any shipped symbol
table (the `.pdata` function table is present, but the functions are otherwise
anonymous). Two ways to turn them into offsets: run
`compile_streamable_resources.bat` on an SDK example mod and diff the emitted
`.shader_library` against its source (the writer's response to a known input), or
follow `make_device_data`'s call graph in a disassembler. The compiler's own
`--output-shader-pdb`/`--shader-pdb-path` flags mean a PDB can be produced for a
compile if the PDB-writing dev toolchain is available.

**The example-mod compile runs (2026-10-05), and the PDB flag is a dead end.**
`stingray_win64_dev_x64.exe --compile-for win32 --source-dir <example_mods/
streamable_resources> --data-dir ... --bundle-dir ... --map-source-dir core <sdk>`
compiles the mod in ~50 s (126 items, 41 `shader_source` -> 250+ libraries) and
writes them to `.build/data/data/<xx>/<hash>`. Adding `--output-shader-pdb
--shader-pdb-path <path>` returns success but produces **no `.pdb`** anywhere: the
installed toolchain does not carry the PDB-writing dev path, so the writer's
offset info cannot be recovered this way.

**The `.shader_library` header, decoded (2026-10-05, `annotate_lib`).** The
smallest library, `gui:.shader_library` (`97072897edac0bef`, 4490 B), is one
context, one pass, no defines - so every field can be read against a single
`gui_gradient` pass (`hlsl_shader="gui_gradient" render_states="gui_gradient"
layer="gui"`, from `gui.shader_source`). Its header:

```
+0x00  76          config_count (matches the UI base's 37..43-family field)
+0x04  2
+0x08  4B32DF61    a per-shader hash (before the "groups")
+0x30  A17AE7B4    world_view_proj  (cbuffer)
+0x38  0x40        its size 64 B
+0x44  E67E11BC    threshold_fade   (cbuffer)
+0x48  0x40        size 64 B
+0x4C  4
+0x50  0x50
+0x54  0x20
+0x58  1
+0x5C  9FCFE126    gui  (the pass's layer)
```

So the header is `config_count`, a per-shader hash, then a **64-byte record per
cbuffer** (`{name, 0, 0, size, ...}` - `world_view_proj` 64, `threshold_fade`
64, the same two the shader's `RDEF` emits), then the pass's state fields with
the **layer hash `9FCFE126` (`gui`)**. The config records from ~0x110 are
`(index, value)` pairs over the engine's `global_viewport` index space - the same
index space this project already decodes for `shader43` (`0x5E..0x65` the eight
channel slots, `0x0C..0x1A` the engine variable block). `4B32DF61` is not a plain
name hash of any token, so it is the per-library/query identity, like the
`shader43` query id.

**A second library confirms the header *is* the writer's device data
(2026-10-12).** `linearize_depth.shader_library`
(`data/02/0205d79d3be91999`, 5776 B - the `linearize_depth` filter in
`post_processing/utilities.shader_source`) is a **945-byte header + its DXBC**
(`DXBC` at `+0x3B1`, chunks `RDEF` `ISGN` `OSGN` `SHEX` `STAT`). The header is the
VT2 writer's per-pass data - the analog of the `shader43` **shader_data** and
**block** together - and it is readable: `+0x00` the config count
(`0x4C` = 76), `+0x04` `2`, `+0x08` the per-library hash, then **variable-length
cbuffer records** shaped `{variable_count, 0, name, 0, size, ...}` - `world_view_proj`
(`A17AE7B4`) at `+0x48` with its size `0x40` at `+0x50`, the second cbuffer
(`CDF7C2BF`) at `+0x68` with size `0x0C` at `+0x70`, matching the filter's own
`CBUFFER_START(c0)`. The RTTI names give the shape the header holds:
`D3DShaderPassData::Shader::{ConstantBufferBindInfo, ResourceBindInfo,
SamplerBindInfo}`, `StreamOutDeclaration::Channel`, `InstanceData::Variable`. So the
`shader43` block looks like a `D3DShaderPassData` - its **head** the shader bind
infos, its **body** the engine's `InstanceData` variables (the preamble's 30
records), its **trailing** the stream-out/state records. Next: decode the header's
variable records and `make_device_data`'s field offsets (`objdump` ships with
w64devkit; `make_device_data` is the disassembly target).

**The block's type is confirmed from the compiler's own RTTI (2026-10-12).** The
`vftable`/TypeRTTI string pool in
`sdk/engine/win64/dev/stingray_win64_dev_x64_compiler_1.exe` (`.rdata`) spells the
D3D device data out exactly:

```text
stingray::Vector<stingray::D3DShaderPassData::Shader>::set_capacity
stingray::Vector<stingray::D3DShaderPassData::Shader::ConstantBufferBindInfo>::...
stingray::Vector<stingray::D3DShaderPassData::Shader::ResourceBindInfo>::...
stingray::Vector<stingray::D3DShaderPassData::Shader::SamplerBindInfo>::...
stingray::Vector<stingray::D3DShaderPassData::InstanceData::Variable>::...
stingray::Vector<stingray::D3DShaderPassData::StreamOutDeclaration::Channel>::...
```

So a `D3DShaderPassData` is an array of `Shader`s (each with a
**ConstantBufferBindInfo** / **ResourceBindInfo** / **SamplerBindInfo** array) plus
an **InstanceData** variable array and a **StreamOutDeclaration** channel array -
i.e. the `shader43` block is `head` = the `Shader` bind infos, `body` = the
`InstanceData` variables (the preamble's own), `trailing` = the stream-out /
state records. The writer is `stingray::shader_compiler::make_device_data`
(`platform_interface.cpp`; its name string sits at `.rdata` `0x140ECD4C8`). What is
still missing is the **field order inside each record** - a controlled compile (add
one cbuffer / resource / sampler and diff the library header) or a
`make_device_data` disassembly gives it. (Early guesses from the *descriptor* counts
failed - `104` vs `84` for groups of `12` vs `10` descriptors - because the head
counts the *cbuffer* descriptors, not all of them; see the record sizes below.)

**The record sizes, from the compiler's own `set_capacity` (2026-10-12).**
Disassembling each `Vector<T>::set_capacity` instantiation (found through its
assert string in `.rdata`) gives the writer's *serialized* element sizes:

| type | bytes |
| --- | ---: |
| `D3DShaderPassData::Shader::ConstantBufferBindInfo` | **20** |
| `D3DShaderPassData::Shader::ResourceBindInfo` | **20** |
| `D3DShaderPassData::Shader::SamplerBindInfo` | **12** |
| `D3DShaderPassData::InstanceData::Variable` | **16** |
| `D3DShaderPassData::StreamOutDeclaration::Channel` | **20** |
| `D3DShaderPassData::Shader` (in memory) | 128 |

(The size is the `lea (%rX,%rX,4)/shl $2` = x20 before the allocation; e.g. the
`ConstantBufferBindInfo` function at `0x1402785DA`.) A block is therefore the
serialized `D3DShaderPassData` - **head** = the shader's bind-infos, **body** = the
engine's config records (the same 30 `{index, value, 0}` the preamble's
`config_base` holds), **trailing** = the instance variables (16 B each), the
stream-out channels and the rest. The head sizes are consistent with a sum of these
arrays plus a small count word - `104 = 5 x 20 + 4` (group 0) and `84 = 4 x 20 + 4`
(group 1) - but not with a cbuffers-only count (`68` for group 2 is `64 + 4` and
`64` is not a multiple of `20`), so the head mixes the cbuffer, resource and sampler
arrays; the field order inside each record is the remaining unknown.

This confirms the writer's shape directly from the source: the library's
**state block carries per-record fields the pass declares** (`write_mask0`,
blend, z state), which is exactly where the `shader43` block's seam/mask words
come from - the VT2 library stores the same pass state in a readable form the
game then packs into `shader43`.

**The channel mask maps exactly to the VT2 render state's `write_mask0`
(2026-10-05, verified 48/48).** The writer's `render_states` (in
`core/stingray_renderer/shader_libraries/common/common.shader_source` and
`gui.shader_source`) declare `write_mask0` per pass, as a `|`-joined channel
list. Packing it into one bit per channel gives exactly the `shader43` block's
body[477] byte:

| `write_mask0` | mask byte |
| --- | --- |
| `0x0` | `00` |
| `red` | `01` |
| `green` | `02` |
| `blue` | `04` |
| `alpha` | `08` |
| `red\|green` | `03` |
| `red\|green\|blue` | `07` |
| `red\|green\|blue\|alpha` | `0F` |

The UI base's 48 shipped pixel masks (`07`, then `01 01 02 02 04 04 08 08 0F 0F
07` twice, then `01 02 04 08 0F 07` four times) are reproduced **48/48** from the
passes' channel tokens with this rule - `07` is `red|green|blue`, the state's
default (the preamble body's own value), and `0F` is the `gui_gradient`
`defined_WRITE_MASK`/`ADDITIVE_BLEND` full-alpha case. So the mask is **derived,
not a recorded table**: it is the pass's write-channel set.

The `gui_gradient` state block is the source of the UI base's channels: it
branches `defined_WRITE_MASK` -> `WRITE_MASK1` -> `write_mask0 = "green"`,
`WRITE_MASK2` -> `"blue"`, else `"red"`, with `ADDITIVE_BLEND` controlling the
blend. `gui.shader_source`'s `editor_options` name the tokens
(`WRITE_MASK`, `WRITE_MASK1`, `WRITE_MASK2`, `ADDITIVE_BLEND`, `MASKED`, ...) -
the same tokens the shader43 declaration's channel permutation uses.

The **seam word** (the 36-byte head's `{seam,0,0,0}`) is the remaining field: it
is not the mask (the mask is body[477]) and does not correlate with it. Measured
run: `2` on passes 1..21, `1` on 23..46, absent on the 32-byte heads (0,22-even,47).
It is the per-pass **blend/z state** the same `render_states` declare
(`blend_enable`, `z_enable`, `z_write_enable`, `ADDITIVE_BLEND`), packed - the
`gui_gradient` block's `ndefined_ADDITIVE_BLEND -> blend_enable=false` vs
`defined_MASKED -> blend_enable=true/alpha blend` is the field's likely source.
Decoding its exact packing is the last step for a fully generated program table.

**The state fields, measured by a controlled compile (2026-10-05).** Probe
shaders were built - a copy of `gui_gradient` whose single pass points at a state
that differs in exactly one field - compiled with the SDK toolchain, and their
`.shader_library` headers diffed. The state fields land at fixed header offsets:

| field | header offset | values |
| --- | --- | --- |
| `write_mask0` | `+0x241` | `0F` rgba, `07` red\|green\|blue, `01` red, `02` green, `04` blue, `08` alpha |
| `blend_enable` | `+0x130` | `1` on, `0` off |
| `z_enable` | `+0x2a9` | `1` on, `0` off |
| `blend_op`/`dest`/`src` | `+0x2dd`, `+0x2ea` | small enums (add/alpha/mul) |
| (any state override) | `+0x13d` | `01` base, `03` overridden - a version/counter |

So the VT2 `.shader_library` header's `+0x241` **is the same packed channel mask
as `shader43`'s block `body[477]`** (`red=1 green=2 blue=4 alpha=8`, default
`07`), and the state fields are `blend_enable`, `z_enable`, the blend ops and a
state-version word. The `shader43` mask is therefore derived from the pass's
`write_mask0`, and `write_mask`/`write_mask_tokens` in `shader_engine_data.rs`
pack it (test `the_ui_bases_masks_are_the_passes_write_masks`, 48/48).

**The seam is not in the D3D11 header.** Probing every `gui_gradient` state
branch (blend on/off/add/mul, z off, write-mask red/rgb) changed only the fields
above - none is `1`/`2`-valued, and the `z` field is `1`/`0`. The SDK compiler's
platforms are `D3D11 GL XB1 GNM` (no D3D12), so the `shader43` seam (which
is `1`/`2`/absent) is a **D3D12-specific** per-pass field the D3D11 compile does
not emit. It is the last unmapped field; the mask half is fully derived and
tested (`write_mask`/`write_mask_tokens`; `derived_programs` now computes each
program's mask from its pass's channel set, and the UI base build is unchanged,
`CD9854D69AC9A5B7`).

**The program-to-group mapping is decoded: `2 × n` programs per group, in group
order (2026-10-05, 5/6 families).** The program list is grouped by group, each
group owning **`2 × n`** programs where `n` is its condition-header entry count
(the group tail's condition-set size), and the groups run in order. Measured with
`examples/program_groups.rs`:

| family | groups (n) | programs | 2 x sum(n) |
| --- | --- | --- | --- |
| `004F18EA` | 3,3,1 | 14 | 14 |
| `17A3DC01` | 5,1,1 | 14 | 14 |
| `38ECBAD1` | 2 | 4 | 4 |
| `3F08AC44` | 5,5,1,1,1 | 26 | 26 |
| `427B5E6E` | 5,1,1 | 14 | 14 |
| UI base (`007bf44b`) | 2,2,1,1,1,1 x6 | 96 | 96 |

The resource-record matching confirms it: the standard family's programs land on
groups **g0 0-9, g1 10-19, g2 20-21, g3 22-23, g4 24-25** (7/7/1/2/2 of them
carry 7-word records). `2A04418E` is the exception (its condition headers do not
read), as `docs` already notes. So a program's group is
`first g with 2*sum(n_0..n_g) > index`, and the group's material table is the one
its split tails should use.

**The standard family now generates (2026-10-05).** Three fixes made the
reconstruction build end to end:

1. `EngineData::fill_material_tables` reads the material tables from a carried
   `group_data` blob (`GroupData::object_tables`), so a family whose groups do not
   template still has them.
2. `EngineData::program_split` records, per program, whether its tail is a **split**
   tail (empty cbuffers *and* empty lists) from the text form; `build_device` only
   derives such a tail. A carried tail with no constant buffers - a vertex tail, or
   a family's phase-prefixed tail - is no longer mistaken for split.
3. `derive_tails` no longer builds a one-group template for a file that carries its
   own `group_data` blob.

With those, the scratch mod's `3F08AC44` reconstruction **generates a 274,800-byte
section**, and the UI base build is unchanged (`CD9854D69AC9A5B7`). The next step
for the pumpkin is to make this a base material in snoopy-mod and reparent the
instance, then test in game.

**Every distinct program decompiles (2026-10-05).** `shader43 --reconstruct-all
<dir> [--variables <dict>] [--dxil-spirv <exe>] [--spirv-cross <exe>] <section>`
decompiles **every distinct container** of a section (not just the first of each
stage), writes one `hlsl_shaders` chunk per container under its stage guard, and
compiles each chunk back to check its interface against the original. On
`3F08AC44` - 26 programs, **17 distinct containers**, 11 distinct `(vs, ps)`
pairs - the result is **17 matched, 0 mismatched, 0 failed**. (The earlier
`--reconstruct` wrote only 2 chunks, one per stage; that gap is why the
source-built standard section crashed with `shader '#ID[38a9b1db]'`
`E_INVALIDARG` - the build compiled one container per stage and paired it with
the family's 17 distinct shipped tails.)

The fix that made 6 of the 17 match: `rebuild_struct` **sorted** a signature's
elements by `(register, index)` before writing the struct. Two elements can share
a register with different masks (`TEXCOORD15` float3 and `CUSTOM1` float at
register 1), and sorting reordered them, swapping their component masks. The
elements are already in signature (register/component) order - the order DXC
packs them - so the sort is gone; a width check pairs a field with the element
whose mask it matches when a register is shared. The UI base reconstruction is
unaffected.

**The enumerator emits the family's passes (2026-10-05).** `shader43 --enumerate
<dir> <section>` writes a `.shader_node` with **one code block per distinct
`(vertex, pixel)` program pair**, each including that pair's two
`--reconstruct-all` chunks, and **one pass per block**. On `3F08AC44`: 26
programs, 17 distinct containers, 13 pairs, **11 distinct `(vs, ps)` pairs** - so
the declaration has 11 passes (the pairs share containers: `pair_0` and `pair_4`
both use `c1` as their pixel chunk, etc.). `shader43 --compile` on that
declaration compiles **all 22 containers** (11 vs + 11 ps) with the expected
sizes. The chunk include resolves by name (`find_chunk`), and each chunk's
`#if defined(STAGE_VERTEX)`/`STAGE_PIXEL` guard makes `job_source` take the right
one per stage.

That is the enumerator: the family's declaration now enumerates its interfaces
from source. What is left for a from-source standard section is the build's
**multi-job mapping** - compiling each pass's container and pairing it with the
section's programs (the carried `program_containers` gives the pairing) - which
is the next step.

**The multi-job mapping is in (2026-10-05).** `compile_declaration` now compiles
**every** job per stage (`ShaderOverrides::job_containers`, one container per job
and stage in job order) as well as the first job's `vertex`/`pixel`. `dtmt build`
detects a multi-job declaration (`job_containers.len() > 2` **and** a carried
container list) and replaces each of the engine data's carried containers with
the compiled container whose **interface** matches it
(`shader::interface_mismatch`), so every program gets the container its own pass
compiled while `program_containers` still indexes the list. The standard base -
the enumerator's 11-pass declaration, the 17-container source, and the carried
engine data - now **generates a 151,856-byte section from source**, and it
**round-trips identically**; its programs carry the compiled containers (program
23 decodes to 4704 B, the compiled pixel size; program 24 to 7426 B, the compiled
vertex size).

**The decompiled source did not reproduce the resources, and now does
(2026-10-05).** In game the source-built standard **crashed** at pipeline
creation: `HRESULT 0x80070057 = E_INVALIDARG, assert: shader '#ID[38a9b1db]'`.
`reflect_dump` showed why: the interface check compared only ISG1/OSG1, and the
resource bindings differed. The shipped vertex container binds
`global_viewport`/`c_instance_offset`/`c_per_object` as cbuffers b0/b1/b2,
`bones`/`last_bones`/`idata` as **structured buffers** (kind 5) and
`global_uav_buffers` as a `uav-rwbyteaddress` (kind 8); `spirv-cross` recompiled
a synthetic `SPIRV_Cross_VertexInfo` at b3, the structured buffers as typed
`Buffer<uint4>` (kind 2) and the UAV as `RWBuffer<uint>` (kind 4).

`--reconstruct-all` now rebuilds the declarations from each container's **own
reflection** (`dxc::reflect` - the ground truth for names, kinds and registers),
replacing the dictionary-driven `name_declarations`. For each container it drops
the synthetic cbuffer and substitutes its members with zero (the engine draws
with a base of zero), renames every cbuffer and resource, and rewrites the
kinds: `StructuredBuffer` for a structured buffer, `ByteAddressBuffer`/
`RWByteAddressBuffer` for a byte-address buffer - the interlocked intrinsics
become the buffer's methods with a byte offset - and the `spvTextureSize`
helper's `GetDimensions` gains the stride a structured buffer needs. The
verification now runs `shader::resource_mismatch` beside `interface_mismatch`,
and **all 17 containers match on both** (before: 17 interface-only).

**The source-built standard renders (2026-10-05).** In game the source-built
pumpkin **spawns and renders** - the `E_INVALIDARG` at pipeline creation is gone
and the pumpkin is visible above the player (the screencap crops can miss it
because it sits a little higher than the player). So the standard base is now
built end to end from our own source and draws. One difference remains: a shader
assert `ASSERT_INSTANCE_BUFFER_OFFSET, value was: 4` fires on the spawn, where
the carried shipped programs assert nothing. The shader writes its assert code to
`global_uav_buffers[10][2]` and the engine reads it back, so the decompiled
program's logic still differs from the original at the check the assert guards -
the `spvTextureSize(idata[...]) / 4 > 4` size check, whose `GetDimensions` the
structured-buffer retype changed. It does not stop the draw, but it is the
remaining fidelity gap.

The in-game flow itself works: the mod's Lua drives title -> hub with no input
(`TitleView.on_enter` -> `_continue`, `StateMainMenu._set_selected_profile` ->
`event_state_main_menu_continue`), and a timer after `StateGameplay.on_enter`
loads the package and spawns the pumpkin (`Application.quit()` after a delay).

The compiler flattens every `.shader_source` declaration into a `.editor`
registry (one entry per compiled `.shader_library`):

```
"#ID[hash]" = { defines = "" | "A B" | [ "A" "B" ]; shader = "name"; ... }
```

and the library name is `<shader>:<tokens in declaration order>`, where
`defines = ""` gives no token (`apply_fog`), `defines = [ "" ]` gives one empty
token (`gui:`), and a list keeps its order. **`vt2_editor` reproduces the
compiler's `debug_file_index.sjson` exactly: 220/220.** (The old "sorted"
reading was wrong - the compiled order is the declaration's order.)

There are **two key spaces**, and the notes must not conflate them:

- **library name** (`debug_file_index.sjson`, `.editor`): `<shader>:<tokens in
  order>`, lowercased, **no context**;
- **query key** (a section's query id): `<shader>:<context>:<defines sorted>
  :PLATFORM_<p>:RENDERER_<r>` - this is what `permutation::key`/`id` model.

The declaration mechanisms that feed the token set:

- `static_compile` entries - `{ shader = "copy" defines = "CUBE_CAPTURE" }`;
- `passes` in a `shaders` entry - `{ hlsl_shader="copy" defines=[...] }`,
  selected through a `defined=`/`fail=` decision tree;
- `editor_options` - each option is `{ name=... define="DIFFUSE_MAP"
  condition="!MASKED" }`, the material-visible define with its condition;
- the block's condition tables (`defined_X` / `ndefined_X`) and code tests
  (`#if defined(X)`, `#ifdef X`, bare `#elif X`).

`vt2_editor --sources <core>` measures the coverage: the `.shader_source` files
alone declare **197 of the 220**. The 23 that remain are:

- **9 engine built-ins** (`apply_hdr_transparent`, `missing_shader`,
  `depth_filter`, `error_debug`, `fill_far_plane`, `fixed_function_blend` +
  `:point_sampler[:skin]`/`:premultiplied`, `copy_filter:cubic_filter*`) - names
  the engine registers itself, present in no source (some only in render
  configs);
- **~14 combinations / compiler-internal names** - `copy:*` (`clear`,
  `encode_rgbm`, `flip_y`, `red_to_rgb[:gamma]`, `project_to_far_plane`,
  `cube_capture:encode_rgbm`), `apply_fog` dropping `DEVELOPMENT`,
  `gui:yuv_video`, `temporal_aa:simple:linear_sampling`,
  `ssr_ray_march_pass:compute`, `bright_pass:eye_adaptation`. Their tokens are
  mostly in the global condition vocabulary (the `#if`/`ndefined_` names); a few
  (`encode_rgbm`, `gamma`, `cubic_filter`) are named only in the build output.

**On the Darktide side:** `vt2_enumerate` runs the same walk through the SDK's
own `ShaderNode::compile_jobs`. Our `ui_default_base` declaration (one context,
one pass, `defines=["SINGLE"]`) yields **one** library key
(`ui_default_base:single`) - which is exactly why the section's family table has
to come from the **inherited UI-base family**: a from-scratch declaration only
generates as many programs as it declares passes x permutation combinations.
Generating the whole 96 outside the family therefore means **declaring the
family's passes and conditions** in our `.shader_node` - the enumerator is the
tool that turns such a declaration into the key set.

**The shipped UI-base family's declaration is now read out of its own carried
data (2026-10-03).** The mined section `e3370cb2107d8aca` carries, in its
contexts and conditions, exactly the material the declaration would have to
produce:

- **contexts:** `default` (murmur32 `F2760503`, 30 queries) and
  `gui_render_pass` (murmur32 `E2C8865F`, 6 queries) - 36 queries, the same 36
  the section ships;
- **conditions** (the 10 hashes its tree tests, `examples/condition_reverse.rs`
  hashing candidates with the engine's Murmur32): `gui` (`9FCFE126`),
  `gui_render_pass` (`E2C8865F`), `gui_hdr` (`BC4EE226`), `gui_mask`
  (`BDF72706`), `transparent_mask` (`5E415D62`), `write_channels` (`8FB860CF`),
  `red` (`9B8DE7E4`), `green` (`4BA4BD58`), `blue` (`0977913D`), `alpha`
  (`3F697354`). Every one is a render layer or a channel-component test - the
  same shape as `gui_gradient`'s `editor_options`. Only `B5F45768` is unnamed.

So the family's "editor options" are layers (`gui`, `gui_hdr`, `gui_mask`,
`transparent_mask`, `gui_render_pass`) plus the channel tests (`write_channels`,
the four components). The 36 query ids are still not plain names or key-space
values (re-checked 2026-10-03 against the full 884k-name dictionary and the 36
ids: no hits), so what a query id hashes over is still open - but its
**condition vocabulary and contexts are now known**, which is what a declaration
needs.

**The group list and the group tails decode out of the family's own tree
(2026-10-03).** The carried `group_template` is not opaque: it is the family's
36 queries crossed with its 6 condition sets, and both halves are readable.

- A **group tail** is a condition set: `u32 n` + `n` 17-byte entries
  (`condition hash` + 13 zero bytes) + an 8-byte trailer (`01 00 00 00 00 00 00
  00`, or zeros for the empty set). The UI base's six, each covering **six**
  queries, are: tail 0 `{gui, transparent_mask}`, tail 1 `{gui_hdr,
  transparent_mask}`, tail 2 `{transparent_mask}`, tail 3 `{gui_hdr}`, tail 4
  `{gui}`, tail 5 `{00000000}` (empty) - all names now reversed (above). Their
  groups use head 0 for tails 0/1 and head 1 for tails 2..5.
- A **group** is `query_id` in query order plus `(head, between, mid, tail,
  material_first)`. The UI base's 36 groups are exactly its 36 queries in order
  (verified: every group hash equals its query id), six per tail, tails 0..5 in
  order. So the group list is
  `for (query, tail) in zip(queries, tails): group(query_id, head_of(tail), ...)` -
  derived from the contexts and the condition tree, not carried.

The `tail` field's bytes are the group's **condition header** (the
`GroupParts::tail` after the shared 28-byte packed record), which is where those
condition hashes live, so the tail is the tree's per-query branch. The one thing
still named by a shipping value is the **group node** (`28B0AB00`, the first
query id in the carried data): deriving it as the context query `6FA3FCCF`
crashes `dispatch_loadtime`, so it stays carried for now.

**The group list is now derived, verified in game (2026-10-04).** Because the UI
base's groups are a plain repetition - group `i` uses tail `i / 6`, every other
part at its zero index - the file writes the count instead of the list: the text
form's `group_template.repeat` field. The reader expands it to
`repeat x tails.len()` groups (the group count is the **family's**, from the
tails; a from-scratch file declares one query but ships the family's 36 groups),
each with a zero query placeholder that `generate` fills from the contexts it
derives. The writer collapses a plain repeating list back to `repeat`. So the
mod's `ui_default_base.engine_data` dropped from **4,990 to 649 bytes** with the
36-entry `groups` block replaced by `repeat = 6`, and the built section is
**byte-identical** (`F3EE2651139FDEB0`, 421,508 B) and **`RENDER_OK`** in the
harness. Test: `a_repeating_group_list_round_trips_as_a_repeat`.

**The group tails are written as condition sets (2026-10-04).** A tail is exactly
its condition header: `u32 n`, then `n` entries of a condition hash plus thirteen
zero bytes, then an eight-byte trailer (`01` when the set is nonempty, zeros when
it is empty). The text form now writes the hash list -
`{ conditions = [ "9FCFE126" "625D415E" ] }` - each hash a quoted word the way the
file writes every other name hash (a bare `0x…` token reads as neither an integer
nor an identifier, and a bare decimal loses the hex), and the reader rebuilds the
fixed bytes. The UI base's six tails are `{gui, transparent_mask}`,
`{gui_hdr, transparent_mask}`, `{transparent_mask}`, `{gui_hdr}`, `{gui}` and the
empty set, so the pool is names rather than bytes. That took the mod's file from
**649 to 429 bytes** (4,990 before the `repeat` step); the build is still
byte-identical (`F3EE2651139FDEB0`) and `RENDER_OK`. Test:
`a_condition_tail_round_trips_as_its_hash_list`.

**The group count is derived too (2026-10-04).** The bytes before the first
group are the group count alone - `GroupData::group_count` reads the first word,
and the first group's node is the header's second word - so the text omits
`prefix` whenever it equals the count its group list implies (`repeat x
tails.len()` groups), and the reader writes that count back. A section with a
longer prefix still carries it. The mod's file went **429 to 406 bytes**, and the
build is still byte-identical (`F3EE2651139FDEB0`).

**`context_count` derives from the permutation plan (2026-10-04).** The header's
context count is the number of contexts a file's `permutations` block (or its
carried `contexts` blob) names - the section's own header word counts the
context records, not the queries - so the text writes it only when it cannot be
inferred, and a plans-only file omits it.

**The permutation plan derives from the declaration (2026-10-04).** With the
count derived, dropping the `permutations` block makes `dtmt build` take the plan
from the declaration's compile jobs (`permutation_plans` in `build.rs`, which
already handles the case where the engine data carries none). The built section
is still byte-identical (`F3EE2651139FDEB0`) and **`RENDER_OK`** in game. The
mod's `ui_default_base.engine_data` is now **288 bytes** - the six group tails
and `repeat = 6`, nothing else. The declaration is one pass while the built
section is the UI-base family's 36-group shape, so deriving still works only
because `generate` pairs the family's tails with the group list; widening the
declaration (the enumerator) makes it clean. But the plan itself is no longer
carried.

The rest of this section is the older, still-open worklist (group data
constructor, conditions tree), kept for reference.

Items 1 and 2 are done and verified against the sections on disk; item 3 turned
out to be done already. What is left is the group data constructor, the
conditions tree, and the in-game test that no round trip can replace.

1. ~~**Group data channel records**~~ **done for the six small sections.** The
   canonical 20-byte records are read through the count word before every table,
   and the packed 28-byte copies are readable; the packed framing still misses
   the kind 5 binding on `427B5E6E` (6 of 7 copies) and the UI base's channel
   table is not reached by the stride. The channel table is the one table a
   from-scratch group data *writes*, and `rebuild_channels` keeps the engine's
   kind, flags and size for every slot.
2. ~~**New-shader path proper**~~ **done for the section codec.** Contexts,
   conditions (as bytes), dependencies, group data and programs are read and
   written, and **all seven sections measured - the six small ones and the real
   UI base - come back byte for byte**. The contexts are variable length and fill
   their region exactly; `conditions_offset` is `48 + the sum of the record
   lengths`, and there is no link table. The carried list is: the section's
   identity word (see below - it is murmur32 of the owning material's path, so
   it is *derived*, not carried), the default-data header word, each context's
   second word (0 on every section measured), the conditions blob, the slack
   between group data and programs, the programs, and the bytes after them. The
   substitution test covers the half a round trip cannot: a renamed context is 4
   bytes at +48, a renamed variable 4 bytes inside the group data, and a query
   added moves every later offset by the sum with the group data as written. See
   `Shader Section Generation Notes.md`.
3. ~~**Unit workstream**~~ **was already done.** `filetype::unit` has compiled
   *and* decompiled the version `0x73` payload for some time, with 16 passing
   tests including four round trips - mesh geometry, the scene graph, mesh objects
   and a full decompile. Verified rather than assumed; the roadmap was stale.
4. **Particles**: a new file type (compile/decompile) so particle-based mods
   like RainbowFlame can be replicated. `File-Type-Status.md` still has this as
   `None`, and it is the last format on the list.
5. **From-scratch end to end**, in game. The declaration front end reads the 15
   real `.shader_node` files - inputs, channels, permutation sets with choice
   recursion, contexts and passes - and a single-group dry run builds a section
   with no shipped blob. What is missing is the generated group data and the
   conditions tree, so a multi-group declaration against a carried template is
   refused by the query/group count check rather than written with placeholder
   ids. Context names pair by identity (`default`, `shadow_caster`); channel
   names do not, so the oracle left is whether the game loads it.


## Goal

Everything a shader needs is defined in the mod (`<material>.hlsl` sources plus a
shader declaration); `dtmt build` compiles the sources and **generates the whole
`shader43` section**. No shipped shader blob, and ideally no engine data either - the
only game-derived data allowed is genuinely engine-side constants, kept as small
as the decode allows.

## Status

**Custom shaders work as long as they are based on an existing shader
interface.** Verified in game, a mod can:

- ship its own programs (HLSL compiled with `dxc`, spliced into a generated
  section);
- bind its own textures through the library's channels;
- drive values for every variable the library knows, from the material SJSON or
  from Lua - variables bind **by name**, and a name the library does not know
  stays out of the layout (up to four floats per known vector4 slot).

What does not work yet:

- **adding** material variables and **adding or renaming channels**. Renaming a
  variable works (verified in game, see below); a cloned variable and a renamed
  channel are implemented and awaiting their in-game observations;
- a **custom interface** (our own cbuffers, resources and slots). The interface
  is carried by data we can only copy today: the device **block** (the preamble
  tail, repeated after each pixel program), the packed group-data copies, the
  group descriptors' `Y` field and the program tails' resource lists. They are
  understood well enough to patch consistently, not yet enough to write.

## Done

- `shader43` section codec: parse/rebuild, Oodle frames, stage from `PSV0`,
  interface check (`lib/sdk/src/filetype/shader.rs`).
- Material-side flows: sibling `.hlsl` compile + splice; `shader_engine_data`
  declaration in a material SJSON makes `dtmt build` generate the entire section
  from the engine data + compiled sources (`crates/dtmt/src/cmd/build.rs`,
  `lib/sdk/src/filetype/shader_engine_data.rs`, example
  `lib/sdk/examples/generate_shader.rs`). snoopymod runs this way; its base
  material is ~427 bytes and the engine data is the only game-derived file.
- Verified in game: a generated section renders (title screen tint driven by Lua
  material values); a generated section *without* the device preamble makes the
  engine run out of memory at the title.
- Verified in game: **renaming a library variable works**. `dev_wireframe_color`
  (b1 + 224) was renamed to `mod_tint` in the group data (216 occurrences), the
  preamble and all 36 tails; the material declares `mod_tint`, Lua drives it,
  and the title background cycles hue under Lua control (its UV ripple and
  brightness pulse are the demo shader's own effects). The engine resolves
  material variables by the library's compiled names, and the name set was
  patchable. The `variable`/`clone` rewrite lines were removed on 2026-09-27;
  the mod now drives the record under its own name.
- `mine_materials` example: dumps `materials/variables/groups/defaults/contexts/
  conditions/tails.csv` for the whole game (2037 shader materials) and writes
  `known.txt`/`unknown.txt` hash bounty lists.
- Shader census (from `variables.csv`): of 2036 mined materials and 3353
  distinct variable names, 129 names appear in at least half of the materials
  (the engine variables), and the material-specific remainder ranges from 0 to
  86 per material with a long tail (most materials have 4 to 40). The per-file
  ranking was joined with a dump of every bundle's entries (the entry's
  `dfn=...` field maps a data file to its resource name hash, which the
  dictionaries resolve). The richest shaders are FX materials:
  `content/fx/materials/abilities/cryptic_force_field_02` (86),
  `content/weapons/materials/weapon_power_sword/weapon_power_effect_cryptic`
  (62), `content/fx/materials/master/wind_render` (61). Many had no
  dictionary entry, so a mod that needs many existing parameter slots should
  prefer the named ones.
- Contexts: read and written as `{name, word2, count, count x {query_id,
  conditions_offset}}`; the queries are the groups (measured on seven
  sections). Conditions tree (records `{tag, b, c, count}` +
  hashes + u16 payload; names are channels: `gui`, `red`, `green`, `blue`,
  `alpha`, `fog_volume`, `linear_depth`, ...), group header descriptor shape
  `{name_hash, flags, X, Y}`, group variable tables, default data table,
  program tails (cbuffer entries + signature lists + resource records for
  textures/UAVs/samplers with space fields and a bindless sentinel).

## Next steps

**Next action (decides whether custom interfaces are a compiler problem or a
dead end): decode the block's record grammar.** The block is the one structure
whose authority is still unproven, and the name-to-slot map lives in it or
beside it. Method: dump the device preamble of shaders with known, different
variable sets (the miner's CSVs give the sets and hashes), diff them and locate
the record for a known variable (e.g. `dev_wireframe_color` = `795CF4A7` on the
UI base shader). Then clone that record with a new name hash and offset, patch
it consistently (block, canonical records, packed copies, descriptors, tails)
and test in game whether a material declaring the new name binds. If the block
can be grown, generation is a matter of modelling its record stream; if not, the
interface is engine-compiled and custom base materials stay bound to shipped
shaders. Supporting decodes, as the experiment needs them: descriptor `Y`
semantics, the packed copies' generation grammar, and the tail resource-list
kinds.

### The tail's rest is nine counted lists, then the device preamble

Measured across every program of six engine-data files (the four carried
families, the UI base and the built UI base material; `tail_dump <file> <stage>
0 check` prints the list counts per program - no failures): after the cbuffer
list a program tail carries **nine counted lists** - each a u32 count, then that
many fixed-size records - followed by the byte-packed device preamble (the same
bytes the engine-data text calls `device_preamble`; that is why only pixel tails
carry the real stream and vertex tails end in a small zero placeholder).

| list | record | content |
|---|---|---|
| 0 | - | the codec's unused second constant-buffer list (empty in every sample) |
| 1 | - | the codec's unused `resources[0]` (empty in every sample) |
| 2 | 7 words | SRVs: bounded textures (`linear_depth`, the material `_mi_*` textures) and bounded structured buffers |
| 3 | 7 words | bindless SRV arrays: the unbounded texture array (`global_texture2D`) and unbounded structured buffers (`bones`, `idata`) |
| 4 | 7 words | UAVs: bounded UAVs (no record in any sample) |
| 5 | 7 words | bindless UAV arrays: `global_feedback_buffers`, `global_uav_buffers` |
| 6 | 4 words | static samplers (`static_minlod_sampler`, space 31) |
| 7 | 3 words | the stage's inputs, system values included (`{murmur32(name), semantic index, register}`) |
| 8 | 3 words | the bindless sampler array (`global_samplers`; the codec reads a fourth `space` word, but writing it hangs the game - see below) |

The framing was confirmed against the Darktide-Asset-Compiler's shader-section
codec (`material/shader_section.{h,cpp}`), which reads the same bytes as
`constant_buffers[2]`, `resources[5]`, `static_samplers`, `inputs` and
`sampler_arrays`; the four resource lists here are its `resources[1..4]`, split
by (UAV?, unbounded?), and lists 0/1 are the two lists it never fills. Its
`material/shader_compiler.cpp` gives the record shapes and the name rule.

A resource record is `{name, index, register, count, space, stride, 0}`:
`index` is the resource's index in the group's descriptor list, `count` is
`0xFFFFFFFF` for an unbounded array and the bind count (normally 1) otherwise,
and `stride` is 0 for a raw (byte-address) buffer, the element stride for a
structured buffer (DXC reports it in the reflection's `NumSamples`), and
`0xFFFFFFFF` for a texture. A sampler is `{name, register, count, space}` and a
sampler array `{name, register, 0, space}`.

**The engine's own names.** A record's `name` is the engine's spelling, not the
HLSL one: DXC names a texture / sampler pair from a combined sampler object
`__tex_<name>` / `__samp_<name>`, and the engine strips that prefix
(`__tex__mi_texcoord0` -> `_mi_texcoord0`, `__samp__mi_coords` -> `_mi_coords`).
That is why the tail names did not match the raw reflection. The engine's own
resources (`static_minlod_sampler`, `global_feedback_buffers`,
`global_texture2D`, `global_samplers`) are in the container's reflection too, so
they are written once, from it, with no extra engine records.

`set_derived_lists` implements this (tests
`the_resource_lists_come_from_the_container_reflection` and, env-gated,
`the_derived_lists_match_a_built_section`). A bindless texture array is the
engine's `global_texture2D` whatever the declaration names it (a from-scratch
source calls it `g_material_textures`), and a bindless sampler array is
`global_samplers`.

`the_derived_lists_match_a_built_section` is the check that matters: pointed at
snoopy-mod's engine data and its built material
(`DTMT_ENGINE_DATA` / `DTMT_MATERIAL`), it derives every program's lists from its
own container's reflection and the group's descriptor list and diffs them
against the carried tail. With the rules below it reports **0 mismatches** for
the standard family (26 programs), so the split derivation reproduces the
shipped tails.

**The engine's sampler-side records are injected, not read.** A stage that binds
the bindless sampler array always carries `static_minlod_sampler` and
`global_feedback_buffers`, whether or not its declaration names them (the UI
base's from-scratch source does not). A **reconstructed** container's reflection
already has them, so the injection dedupes. The gate is the sampler array, not
"the stage samples a texture": `linear_depth`'s program (p9) reads a texture
with no sampler, and its shipped tail has neither record. The feedback record is
injected only when the group's descriptor list carries `global_feedback_buffers`
- a program whose group does not would otherwise fail to index it.

**The bindless sampler array is 3 words, not 4.** The Darktide-Asset-Compiler's
codec reads `{name, register, 0, space}`, but writing that fourth word breaks the
game: the UI base derived that way (421,876 B) hangs at D3D / window init - two
fresh launches produced `NO_LOAD` with no Lua boot at all. The engine's runtime
reads the sampler-array record as 3 words (`{name, register, 0}`), which is what
this reader does (`TailLists::SIZES[8] = 3`) and what the derived record now
writes, leaving whatever follows in the block. Reverting to the 3-word form
restores the known-good UI base (`4cc21b79457af061`, 421,684 B, file hash
`CD9854D69AC9A5B7`). So for this one record the derivation follows the engine,
not the codec. The standard family's tails are unaffected - they are **carried**
in the engine-data text (not split), so the derivation never runs for it.

**The split is enabled for the standard (2026-10-12).** Both pieces landed:
`examples/split_tails.rs` rewrites the file (dropping every derivable tail to
its block), and `derive_program_groups` fills the program-to-group mapping from
the engine's `2 x n` rule (a group owns `2 x n` consecutive programs, `n` its
tail's condition count, in group order) when the file carries no `group` tag, so
the split gate accepts it in place of "every group agrees". The migrated
`standard_base.engine_data` is **42,091 bytes** and the built section is
**byte-identical** (`578c983197da6b91`), **`PUMPKIN_OK`** in game. Run
`split_tails` to a fixed point (the first pass off a carried file writes a
slightly larger intermediate; a second pass stabilises at 42,091). The `2 x n`
measurement is above (`examples/program_groups.rs`).

**What the file still carries** (42,091 B): the per-program device **blocks**
(~23 KB, 55%) - the block grammar is the open item; the **group template**
(~15.6 KB, 37%) - the descriptors, material table, condition tails and group
list, derivable from the declaration like the UI base's; the preamble (~1 KB),
contexts + conditions (~0.3 KB) and the program list (~1.8 KB). So the two
milestones left are the per-program block grammar and deriving the standard
family's group data (the enumerator).

**The contexts and conditions do not derive from the declaration alone** (checked
2026-10-12): dropping the file's `contexts`/`conditions` so `build.rs`'s
`permutation_plans` derives them from the 11-pass declaration builds a section
**112 bytes smaller** (145,492 vs 145,604), so the derived condition tree is not
the family's. They stay carried until the family's conditions are declared - the
enumerator (`Shader Permutations.md`).

`group_probe` on the migrated file shows why the group template is not just the
`derived_head` path: the five groups' heads are **212/180/180/116/100 bytes**
and their `mid`s 2140/2140/240/288/288 - each group has its own descriptor list
and material table. Deriving it needs a **per-group** head (the union of that
group's programs' resources in root order, which `derive_program_groups` can now
select) rather than the single `derived_head(resources)`, plus the per-group
`between`/`mid`. `group_probe` now dumps each group's descriptor list
(`[{name, flags, x, y}]`): the `x` offsets follow the same 24-per-cbuffer /
8-per-other rule `derived_head` uses, but the **`y` usage field is
family-specific** - the standard's `bones`/`idata` are 341, `global_uav_buffers`
682, `linear_depth` 256, the textures 1 and `global_feedback_buffers` 2, against
the UI base's 0/5/10. So the group-template milestone needs the descriptor `Y`
mask, decoded below.

**`y` is decoded: a 2-bit-per-pass usage mask.** Correlating the whole game's
group descriptors (`C:\Users\Dan\dtmt-dump\groups.csv`, 80,465 rows, via the
scratch `decode-descriptors.ps1`) shows `y`'s values are exactly the fields `01`
or `10` repeated: `1, 5, 21, 85, 341, ...` (the `01` sequence, `x -> 4x + 1`) and
`2, 10, 42, 170, 682, ...` (the `10` sequence), plus gapped masks like `256`
(`0x100`, pass 4's `01` field alone). So `y` packs **two bits per pass** - bit
`2k` for the pass's first stage, bit `2k+1` for its second - and the field count
is the group's pass count. Worked against the shipped reflections: group 0's
`bones`/`idata` `0x155` = the first stage of all five passes (p0/p2/p4/p6/p8),
`last_bones` `0x1` = pass 0 alone, `global_uav_buffers` `0x2AA` = the second
stage of all five (p1/p3/p5/p7/p9); group 3's vertex-only `bones`/`idata` `1`,
its `global_uav_buffers` `2`. The bindless arrays are the exception - every group
has `global_texture2D` `1`/`5` and `global_feedback_buffers` `2`/`10` (the two
`01`/`10` sequences over the group's passes), so they are attributed to a fixed
stage, not to whichever program reads them. `linear_depth` `0x100` attributes it
to pass 4's first stage, which the recompiled reflections do not, so **the mask
is mined from the shipped group data, and deriving a head needs each pass's
stage→resource set** (the root-signature order the community compiler's
`shader_compiler` builds). Runs: `group_probe <engine_data>` (rebuilt group data
10,725 B), or `decode-descriptors.ps1` over the miner's `groups.csv`.

**The group head fully derives (verified).** The env-gated test
`the_derived_lists_match_a_built_section` builds each group's head from that
group's programs and compares every field to the shipped one: over the standard's
43 descriptors, **name order, `flags`, `x` and `y` all match exactly** (0
mismatches). The rules:

- **names / order**: the union of the group's programs' bound cbuffers / SRVs /
  UAVs (samplers excluded), in binding order;
- **`flags`**: the mined value for the engine's own names
  (`engine_descriptor_flags`) OR-ed with the reflection kind's low bits (texture
  `0x3`, structured SRV `0x4`, UAV `0x5`, raw UAV `0x7`); this needed one table
  entry, `global_environment_settings` (`0xB3A2EB88`) at `0x101`;
- **`x`**: the 24-per-cbuffer / 8-per-other running offset;
- **`y`**: **two bits per pass** - bit `2k` when the resource is bound as an
  **SRV** in pass `k` (the program pair `2k`/`2k+1` of the group), bit `2k+1`
  when as a **UAV**; cbuffers take no bit. So `bones`/`idata` (SRV, all five
  passes) are `0x155`, `global_uav_buffers` (UAV, all five) `0x2AA`,
  `linear_depth` (SRV, pass 4) `0x100`, `global_texture2D` `1`,
  `global_feedback_buffers` (UAV) `2`.

So each descriptor **record** (`name`, `flags`, `x`, `y`) of a per-group head is
fully derivable. **The `between` part derives too**: it is `{global_viewport
size, 0, material record count}` (the standard's are `1776, 0, 2` / `1776, 0, 2`
/ `1776, 0, 11`), where `derived_between` writes a hard-coded UI-base
`{240, 64, 0}`.

One field is still unexplained: the head's leading **header word**
(`parts.head[4..8]`) - the standard's groups carry `0x390`, `0x380`, `0x1A0`,
`0x170`, `0x170` where `derived_head` writes a hard-coded `0x130` (a UI base
value that rendered). It is not a function of the descriptor count (the two
`0x170` heads have 6 and 5 records, while the two 10-record heads are `0x380`
and `0x1A0`). It **tracks the descriptors' final `x`**: `header - x_end` is
`736, 736, 288, 296, 296` across the five groups, so it is `x_end` plus a
per-family/table constant (a size or offset).

**That constant is the sum of the sizes of the head's kind-0 (material)
cbuffers**, which `cbuffer_sizes` now gives, and `x_end` is the descriptors'
per-draw total **rounded up to 16**:

| group | per-draw total | rounded | header | sum of kind-0 cbuffers |
| --- | --- | --- | --- | --- |
| 0 | 168 + 8 = 176 | 176 | 912 | 16 + 416 + 304 = 736 |
| 1 | 152 + 8 = 160 | 160 | 896 | 16 + 416 + 304 = 736 |
| 2 | 120 + 8 = 128 | 128 | 416 | 272 + 16 = 288 |
| 3 | 72 + 8 = 80 | 80 | 368 | 16 + 272 = 288 |
| 4 | 64 + 8 = 72 | **80** | 368 | 16 + 272 = 288 |

So **`header = round16(per-draw total) + sum`**, and all five groups fit. The
rounding only bites group 4 (72 -> 80); the other four are already 16-aligned.
The per-draw `x` allocates 24 per cbuffer regardless of the real size, so the two
terms are independent - which is why the header word looked like "x_end plus a
mystery constant".

**Dropping the head content is how to exercise the derivation - and it fails
today (2026-10-12).** Blanking the standard's five head-pool entries (so each
group's head is just its 4-byte query - the only shape `derived_head` runs on)
stops the build with **`A306B24F is not in the group's descriptor list`**:
`derived_head` emits the engine's own descriptors plus the reflection's cbuffers
and structured buffers, but not the **other reflected resources** the standard's
heads carry (`A306B24F` flags `0x0107`, `B3A2EB88` `0x0101`, `9B8038E0`
`0x0003`), and its order differs. So before the carried heads can be dropped, the
group template's head needs the family's own **descriptor table** (names + flags
in the shipped order); the `mid` needs the same table for its records, and the
`between` for `global_viewport`. `derived_head` is currently dead code - neither
the standard (carries its heads) nor the UI base (its declaration template
carries them) reaches it - so this table is the concrete next step, not more
field-level decoding.

**The head's descriptor order is the first-binding order.** The community's
`shader_compiler` names this rule for the engine's root table - "the order a
group's distinct constant-buffer / resource names are first bound across its
passes and stages" - and the standard's heads fit it: `derived_head` pushes a
fixed order (`c_per_object`, `global_viewport`, then cbuffers, then structured
buffers) and only those two kinds, which is why it misses `A306B24F` (a UAV,
flags `0x0107`) and `B3A2EB88` (flags `0x0101`) and orders the rest wrongly. So
the head derivation is: walk the group's programs' reflected bindings in order,
take each distinct resource once, map its flags (the engine's mined from a
shipped head, a stage resource's by kind), then apply the `x` (24 per cbuffer, 8
per other) and `y` (two bits per pass) rules and the header word above.
`StageResources` sorts its resources by kind, so it has to keep the raw binding
order for this.

**Validated on the standard (2026-10-12).** Reflecting group 0's ten programs'
built containers (`snoopy-std-dump/*.dxbc`) and taking each distinct binding once
gives

```
global_viewport, c_instance_offset, c_per_object, bones, last_bones, idata,
global_uav_buffers, global_environment_settings, global_samplers,
static_minlod_sampler, global_texture2D, global_feedback_buffers, c_depth_only
```

and the shipped head is that list **minus the two samplers** (11 descriptors),
then one more - the material's `linear_depth` (`9B8038E0`, flags `0x0003`, which
is not a container binding of group 0). So the head is `(the group's programs'
bindings, samplers dropped, first-seen order) + the material's resources`, and
every engine name's flags are already in `engine_descriptor_flags` - `A306B24F`
is `global_uav_buffers` and `B3A2EB88` is `global_environment_settings`, both
confirmed against the constants. The 11 derived descriptors match the shipped
head position for position, and the 12th (`linear_depth`) is explained too: the
source declares `Texture2D<float4> linear_depth : register(t0, space0)`, but the
*rebuilt* container strips it (the reconstruction leaves it unused), so it is
absent from the rebuilt reflection while the shipped container binds it. The rule
is therefore exactly **the group's programs' bindings, samplers dropped** - no
separate material term. What is
left for the group
template is that header word, the per-group `mid`, and dropping the carried
heads. The `mid` is a different structure than the UI base's: `derived_mid`
produces **180 bytes** for every standard group against the shipped
**2140 / 2140 / 240 / 288 / 288**, with **0 leading bytes** agreeing, so the
standard's `mid` is a per-group packed region `derived_mid` does not model.

The `mid` pool is the **biggest single piece of the carried file** - 9,648
chars / 4,808 B, 23% (the program-block pool is the 23 KB of `tails`). It is
the same data `group_data.rs` parses for a shipped blob, in template form:
a run of sections, each `{ u32 W1, u32 W2, u32 count }` + `count` records.

- The records of every section but the last are the **20-byte canonical
  records** `group_data.rs` already models - `{kind, flags, name_hash,
  cbuffer_offset, size}` (kinds 0..4 size 4/8/12/16/64).
- The last section is the **28-byte packed copies** `{name, a, b, offset, kind,
  cbuffer, X}` - the shape `derived_mid` emits per texture slot
  (`{bca, 5, 0, 0, 5, c_per_object, 1}`), with a trailing `X` where `derived_mid`
  writes a constant.

**Every standard group walks exactly.** Walking all five shipped mids with
`{W1, W2, count}` + `count` records (20-byte canonical records, the last section
28-byte copies) lands on the word in every case:

```
mid 0 (535): {16,176,14} 5w | {416,192,58} 5w | {400,0,13} 5w | {304,608,14} 7w
mid 1 (535): {16,160,14} 5w | {416,176,58} 5w | {400,0,13} 5w | {304,592,14} 7w
mid 2 ( 60): {272,128,1} 5w  | {16,400,7} 7w
mid 3 ( 72): {16,80,9} 5w    | {272,96,3} 7w
```

Group 0's canonical records include `A17AE7B4` (the engine's `world_view_proj`,
offset 128 size 64); the copies section is the shape `derived_mid` already
emits per texture slot. `W2` of a later canonical section is
`W1(first) + x_end` (16 + 176 = 192, 272 + 128 = 400, 16 + 80 = 96), and the
copy section's `W2` is `W1(s2) + W2(s2)` (416 + 192 = 608, 416 + 176 = 592).
**`W1` is a cbuffer's size, from the compiled reflection** (solved
2026-10-12). The VT2 SDK's `.shader_library` files are **DXBC with an RDEF chunk
naming every cbuffer member**, which pointed at
`ID3D12ShaderReflectionConstantBuffer::GetVariableByIndex`; `lib/dxc` now
exposes each constant buffer's variables (`CbufferVariable`), and `reflect_dump`
prints them. The reflection - and the decompiled `.shader_source` - show every
cbuffer as **one opaque `float4 _NN_m0[N]` array**: the standard's
`global_viewport` **1776**, `c_instance_offset` **16**, `c_per_object` **416**,
`global_environment_settings` **400**, `c_depth_only` **304**. Those sizes are
the mid's `W1`s (16 / 416 / 400 / 304, plus 272 for a variant's `c_per_object`)
and `global_viewport`'s 1776 is the `between`'s word0. So `W1` is a **running
cbuffer offset** - each section is one cbuffer's field records and the header's
first word is the previous cbuffer's size - already in hand as
`StageResources.cbuffer_sizes`. The mid's records are that cbuffer's **fields**:
resolved through `dictionary.csv`, group 0's are `world_view_proj`, `view_proj`,
`world`, `last_world`, `bca`, `nm`, `orm`, `material_id`, `linear_depth`, etc. -
the declaration's own cbuffer fields, not the reflection's opaque array. The `mid` is **not a new grammar**: it is `group_data.rs`'s
canonical records and packed copies, so `derived_mid` should be the template's
existing `VariableTable` / `rebuild` path fed from the compiled program's
reflection, not a hand-built 180-byte constant.

**The `mid` is derivable without the original declaration (2026-10-12).** Group
0's first `mid` section resolves to `bca`, `nm`, `orm` - exactly
`standard_base.material`'s `channels = ["bca", "orm", "nm"]` - plus `view_proj`,
`world_view_proj`, `world`, `last_world`, `linear_depth` and `material_id`, which
are the engine's. So a `mid` section is **the material's channels plus the
engine's cbuffer fields**: the channels come from the `.material`, and the
engine's fields need only a recorded **engine interface table** (name, cbuffer,
offset, size, kind - the same kind of mined table as `engine_descriptor_flags`).
That is the route to dropping the `mid` pool (9.6 KB, 23% of the file) for the
standard, and it needs no reconstruction of the original HLSL.

**The sections map to the group's cbuffers, one header ahead (2026-10-12).**
Reading group 0's `mid` records (`group_probe`) makes the layout explicit.
Section 1 is `{W1=16, W2=176, n=14}` and its 14 records are the material's
channels (`bca` at 0, `orm` at 24, `nm` at 40 - each expanding into its
components) followed by the engine's view matrices (`view_proj` 64,
`world_view_proj` 128, `world` 192, `last_world` 256, `711b4adc` 320) and two
more (`ed9d8188` 384, `b9d685cb` 400) - i.e. exactly **`c_per_object`'s 416
bytes**. Its `W1` is 16, `c_instance_offset`'s size, so a header carries the
**previous** cbuffer's size and the **current** cbuffer's field count. Section 2
(`{416, 192, 58}`) is `global_environment_settings`'s 58 fields, section 3
(`{400, 0, 13}`) is `c_depth_only`'s, and section 4 (`{304, 608, 14}`) the packed
copies. So the `mid` is `for each cbuffer: {the previous cbuffer's size, running
offset, its field count} + its fields`, with the material's channels packed
first in `c_per_object` - derivable from the group's cbuffers (the reflection),
the material's channels (`.material`) and a recorded engine interface table.

**That engine interface table is a fixed engine thing (2026-10-12).** Resolving
group 0's `mid` sections 2 and 3 through `dictionary.csv` gives the engine's own
fields - `fog_color`, `fog0_settings`, `sun_direction`, `sun_shadow_map_bias`,
`skydome_flow_tiling`, `volumetric_phase`, `ssm_enabled`,
`global_roughness_multiplier`, `wind_amount`, `world_interaction_window_size`,
the view matrices, ... - with the material's channels (`bca`, `orm`, `nm`)
interspersed. So the table is the engine's cbuffer layouts (name, cbuffer,
offset, size, kind), fixed across materials, and recordable once in code the way
`engine_descriptor_flags` already records the descriptor flags. With it,
`derived_mid` has every input: the group's cbuffers in first-binding order
(minus `global_viewport`), each cbuffer's engine fields from the table, the
material's channels packed first in `c_per_object`, and the packed copies.

**Group 0's engine interface, read off (2026-10-12).** `group_probe` plus the
dictionary give the table's shape. `W1=16` heads **`c_per_object`** (416 B): the
material's channels at 0..56, then the engine's `view_proj` 64, `world_view_proj`
128, `world` 192, `last_world` 256, `711b4adc` 320 (64 bytes each), `ed9d8188`
384, `b9d685cb` 400. `W1=416` heads **`global_environment_settings`** (400 B):
`emissive_intensity` 12, `global_roughness_multiplier` 16, `sun_enabled` 96,
`sun_direction` 100, `sun_color` 112, `local_shadow_map_bias` 132,
`sun_shadow_map_bias` 144, `ssm_shadow_map_bias` 160, `sun_shadows_enabled` 172,
`ssm_enabled` 176, `skydome_u_offset` 180, `skydome_intensity` 184,
`skydome_tint_color` 192, `skydome_flow_direction` 208, `skydome_flow_speed` 220,
`skydome_flow_tiling` 224, ... `W1=400` heads **`c_depth_only`** (304 B). The
engine's offsets are fixed - the material's channels occupy a fixed prefix - so
the table is `(cbuffer, kind, hash, offset, size)` and records as one const.

**The second header word (`W2`) is a running sum (2026-10-12).** Group 0's
sections have `W2 = 176, 192, 0, 608`: the first is `x_end`; the second is
`W1(first) + W2(first)` (16 + 176); the third is `0` - its cbuffer,
`c_depth_only`, contributes nothing; and the packed-copy section is
`W1(second) + W2(second)` (416 + 192). Groups 1-4 fit the same pattern (160/176,
128/400, 80/96). So `W2` is the running end of a section's data in the packed
space, `0` for a section with none - the last small rule needed before
`derived_mid` can be written.

**The declaration's `inputs` drive the build (2026-10-12).** The standard's
`.shader_node` in the mod is the *enumerated* form, which carries no `inputs`; the
`--reconstruct` path emits them (`#78E25D0F`, `#9A531871` - the group's two
object records, both textures). Adding those two entries to the mod's node leaves
the built section **byte-identical** (`578c983197da6b91`, 145,604 B), so the
material's variables now reach the build from the declaration rather than the
engine data. Dropping the engine data's `materials` pool on top of that does
*not* reproduce the section (+112 B), so the material table is still built
differently - the next gap on the road to dropping the engine data.

**The material table is the object table, not the channels (2026-10-12).** The
engine data's `materials` pool holds `{5, 0, <texture>, offset, 4}` rows - group
0's table is exactly two, `78E25D0F` at 0 and `9A531871` at 4 - and those are the
same two hashes the `--reconstruct` path emits as its `inputs` (taken from the
section's object table). `build.rs`'s `material_records`, however, builds the
table the *other* way: per sampled channel (`bca`/`orm`/`nm`) it emits `{5, 0,
<channel>, offset, 4}`, `{1, 0, <channel>, offset + 4, 8}`, `{1, 0, <channel>,
offset + 16, 8}`, then the engine's `view_proj` / `world_view_proj` / `world` rows
and the declaration's variables. So the +112 B is that difference - the shipped
table is the object table (two slot rows), the graph-derived one the channels'
clones - and it is the concrete mismatch to close: `material_records` should
agree with the `inputs` the reconstruction already emits.

Those two hashes are **not** the material's channels (`bca` `2FADCC8C`, `nm`
`B45E95F3`, `orm` `8655079C`) nor its texture paths (`textures/mods/snoopymod/
pumpkin_color` `44363231`, `..._normal` `a244124a`), and the dictionary does not
name them at all. The material is `material_contexts = { surface_material =
"twitch_icon_bad_indigestion" }`, i.e. it **inherits** a template, so its object
table's two rows are that template's own engine-provided textures - which the
build's graph evaluation resolves, and which is why they have no dictionary name.
So `material_records` has to reproduce the *object* table's rows (the resolved
engine textures), not the declaration's channel clones. Dropping the pool and
rebuilding grows the standard's section by only **112 bytes** (and leaves the UI
base's section untouched), so the derived table is close to the object table - a
row or two per group - rather than a wholesale mismatch.

(A build's log reports the *section* size, which is 132 bytes below the data
file's because the file also carries the 28-byte header and the material
template; do not compare the two directly.)

**The engine interface is keyed by cbuffer name, and its offsets are relative
(2026-10-12).** Mining the engine fields out of the `mid`s (78 distinct ones)
shows the same field at two *different* absolute offsets in different groups -
`view_proj` at 32 and 64, `A17AE7B4` at 96 and 128 - because a cbuffer's size
includes the material's channels, so it varies with the material. The interface
therefore has to key on the **cbuffer name** (`c_per_object`,
`global_environment_settings`, `c_depth_only`) and store each field's offset
*relative to the end of the material's variables* (padded to 16), which is the
"offsets by HLSL packing" rule. The `mid`'s records then carry the absolute
offset (the channels' padded size + the relative one). The mined table is at
`%TEMP%/dtmt-mat/mid-interface.txt` as `(size, kind, hash, offset, size)`; it
needs re-keying by name and re-basing by the channel prefix before it can be a
const.

**The relative offsets are consistent (2026-10-12).** Re-basing confirms it: the
272- and 304-byte cbuffers carry the *same* field set at a constant 32-byte
shift - `view_proj` 32/64, `world_view_proj` 96/128, `world` 160/192, and the
trailing `795CF4A7` 224/256, `999C5B7A` 240/272, `429C277D` 256/288 - so their
offsets relative to the channel prefix are 0 / 64 / 128 / 192 / 208 / 224 in
both. The prefix is the material's channels' padded extent (32 there, 64 for the
416-byte `c_per_object`, which additionally carries `last_world` and `711B4ADC`).
So a cbuffer is identified by its **field set**, not its size, and the const's
offsets are the relative ones.

**The `mid`'s cbuffers are the head's cbuffer descriptors minus `global_viewport`
(2026-10-12).** Group 0's reflection gives `c_instance_offset` 16, `c_per_object`
416, `global_environment_settings` 400, `c_depth_only` 304 - exactly the `mid`'s
cbuffer order (the `W1` chain), and exactly the head's kind-0/1 descriptors minus
`global_viewport`. So the interface const can key on the cbuffer **name**: the
builder takes the head's cbuffer descriptors, drops `global_viewport` (which the
`between` handles), and looks each name up. `c_per_object` and `c_depth_only`
share a 272-byte variant in other groups, so the name - not the size - has to be
the key.

**The interface sets are four, and one is shared (2026-10-12).** Re-basing the
mined table (the prefix is the *minimum* offset in each cbuffer - `view_proj`'s)
leaves four distinct field sets: `c_instance_offset`'s single `material_id` row;
the 416-byte `c_per_object`'s five matrices (`view_proj`, `world_view_proj`,
`world`, `last_world`, `711B4ADC`) plus `ed9d8188`/`b9d685cb`; the
400-byte `global_environment_settings`' 58 fields; and a three-matrix set that
`c_depth_only` **and** one `c_per_object` variant (272 bytes) both use. So a
cbuffer's *name* does not map to one set - the set is chosen per group - which
means the const should key on the set (or carry the set per group) rather than on
the name alone.

**The `mid`'s headers interleave with the *next* cbuffer (2026-10-12).** Reading
the sections against the cbuffer sizes gives the exact layout: the header before
a section is `{the previous cbuffer's size, W2, the next cbuffer's field count}`,
so the fields after a header belong to the **following** cbuffer, and the first
cbuffer's own fields are not emitted at all. Group 2 confirms it: `{272, 128, 1}`
+ 1 record + `{16, 400, 7}` + 7 copies, where 272 and 16 are its two cbuffers'
sizes and the single record is the second cbuffer's field; the last header
carries the copy count. `W2` is the running `W1 + W2` of the previous section,
and `0` for a cbuffer with no packed data (group 0's third,
`global_environment_settings`). So the assembly is: for each cbuffer (the head's
cbuffer descriptors minus `global_viewport`, sizes from the reflection),
`{previous size, running W2, next field count}` + the next cbuffer's channels and
engine fields (`engine_cbuffer(size)` at `prefix + relative`), then the packed
copies.

**The material's channels pack sequentially (2026-10-12).** Group 0's first `mid`
section shows the layout: `bca`'s rows are `{5, 0, 4}`, `{1, 4, 8}`, `{1, 16, 8}`
(a 24-byte extent - it keeps a kind-5 row), then `orm`'s `{1, 0, 8}`, `{1, 8, 8}`
and `nm`'s `{1, 0, 8}`, `{1, 8, 8}` (16 bytes each), so the channels run
0..56 and the prefix is `align16(56) = 64`. The rows are *not* a fixed stride -
each channel's rows are packed, and the next channel starts at the previous
one's end - so a channel is `(its rows, its extent)` and the engine's channel
table gives both. Which channels a group has is its own: groups 3 and 4 carry
only `bca` (a depth pass samples one), while 0-2 carry all three. The group's
sampled set is what the material graph's evaluation resolves.

**That per-group subset is the last input (2026-10-12).** Everything else the
`mid` needs is now derived and verified, but *which* of the material's channels a
group carries is not in the declaration: the standard's reconstructed node has a
single code block listing all seven sampler slots, and the enumerated node has
none, while groups 3/4 carry only `bca`. It is a property of the group's *passes*
(a depth pass samples one channel), so it has to come from the pass-level
evaluation, not the flat sampler list. That is the one piece to wire before
`derived_mid` can replace the carried `mids` pool.

**...and the device blocks carry it (2026-10-12).** The per-group channel set is
in the blocks, not the declaration: tail 1-8 (groups 0-2) contain all three
channel hashes (`bca` `8CCCAD2F`, `orm` `9C075586`, `nm` `F3955EB4`), tails 9-10
(groups 3-4) contain only `bca`, and the vertex tail (0) and the smallest pixel
tail (11) none. So a group's channels are the union of its programs' blocks'
channel records - the `mid` derives from the *carried blocks*, not the material
graph. That makes the chain concrete: **blocks -> channels -> `mid`**, with the
group template (heads, `mid`s, `between`) derivable from the blocks plus the
engine interface. The blocks themselves (~23 KB) are then the one thing the file
has to keep, and the block grammar is what would finally remove them.

**So the `mid` needs no per-group bytes beyond the channel hashes
(2026-10-12).** With the channels in the blocks, every `mid` input is derivable:
the cbuffers from the head's kind-0/1 descriptors (their sizes from the
reflection), `x_end` from the head's per-draw total, the channel records from
`channel_records`, and the packed copies from the channels' kind-5 rows. So the
engine data could carry a **per-group channel list** (a handful of hashes)
instead of the whole `mid` pool - 9.6 KB down to tens of bytes - with
`derived_mid` doing the rest. That is the concrete next implementation.

**The blocks carry the channels as 16-byte records (2026-10-12).** The device
block for group 0's pixel program (823 B) holds, at byte 643,
`03000000 8CCCAD2F 04000000 01000000` - `{kind 3, name hash, size 4, 1}` - once
for `bca`, `orm` and `nm`, behind a count word (`0D000000`, 13). So a block is a
record stream of the same family as the `mid`, but with a **16-byte** record
`{kind, name, size, flags}` instead of the `mid`'s 20-byte one, and it is where a
group's channels come from. Its full grammar is still open - and the blocks are
now **71% of the engine data** (23,240 of 32,642 bytes), the one large decode
left.

**The blocks re-lay-out the preamble's body (2026-10-12).** The standard's
device blocks do **not** end with the preamble body verbatim (as the UI base's
apparently do). The body's records appear *spread* through the block at unrelated
offsets - the body's byte 12 at 582, byte 24 at 177, byte 48 at 225, byte 96 at
386 - while the block's own start differs (`1E000000` where the body has
`1D000000`). So a block is a **re-layout of the body's records plus the
program's own data**, not a copy. Nor is it a plain 16-byte record stream: a
block is 823 bytes, not a multiple of 16, even though its channel records are 16
bytes each. The grammar is still open, but it is a re-layout rather than an
opaque blob, which is the thread to follow.

**Every pixel block contains the whole preamble body (2026-10-12,
`block_probe`).** Probing each program's device block against the preamble's 30
12-byte `{id, flags, 0}` records:

```
program  1 Pixel: block  909 B; whole records 30/30, {id,flags} pairs 30/30
program  3 Pixel: block  863 B; whole records 30/30, {id,flags} pairs 30/30
...
program 25 Pixel: block   92 B; whole records  0/30, {id,flags} pairs  6/30
program  0 Vertex: block   52 B; whole records  0/30, {id,flags} pairs  3/30
```

Every pixel block contains **all 30** records; the tiny vertex blocks (52 B)
contain none, and the 92-byte tail holds six `{id, flags}` pairs but no whole
record. So "re-layout" is now precise: a pixel block is the preamble's body
**re-encoded** - every record present, at re-ordered offsets, in a wider record -
plus the program's own data. The grammar to find is the per-record re-encoding
rule and then the program's extra records.

**A block is head + the shared body + the program's data (2026-10-12).** The
probe now prints where the body's records sit in a pixel block:

```
144  record  0  [00000000, 00000000, 00000300]
156  record  1  [00000000, 00000000, 001D0000]
168  record  2  [00000000, 00000000, 08000000]
...  (in the body's own order)
348  record 17  ...  gap-before 12
...
480  record 28
```

The records are **contiguous and in the body's order**, not spread. The block's
first 144 bytes are the per-program **head**, then the body's 30 records verbatim
(with a 12-byte gap where the program's own record is inserted), then the
program's trailing data. So the doc's original "block = body + head" model holds
after all: the earlier "spread" reading was an artefact of comparing against the
*file's* preamble (473 B) instead of the runtime's `device_preamble` (863 B). The
per-program head (144 B here) and the trailing program data are what the grammar
still has to produce.

**The blocks share an identical 348-byte record run (2026-10-12).** Every pixel
block holds the body's 30 records contiguously, and only the bytes *around* them
differ - the head before and the trailing data after:

```
program  1:  909 B - head 144, records 144..492, trailing 417
program  3:  863 B - head  35, records  35..472, trailing 391
program 21:  625 B - head  84, records  84..432, trailing 193
program 19: 1310 B - head  96, records  96..444, trailing 866
```

So a block is `{head} + {the shared body} + {trailing}`, with the head (84-144 B)
and the trailing (193-866 B) per program. Factoring the shared body out of the
twelve pixel tails would remove ~4 KB of the ~23 KB immediately, and the
head/trailing are the program-specific parts a full derivation still has to
produce.

**The copies are the channel rows, once per channel-bearing cbuffer
(2026-10-12).** Group 0's last section (14 rows) is **two identical sets of
seven**: first `c_per_object` (`0xB5639618`) with `x = 2`, then `c_depth_only`
(`0xCFB440F0`) with `x = 4`. Each row is `{channel hash, a, b, offset, kind,
cbuffer, x}` and the seven are exactly the `mid`'s channel rows - `bca`'s
`{5,0,4}`, `{1,4,8}`, `{1,16,8}`, then `orm`'s and `nm`'s two each - with `b` the
copy set (0, then 1) and `a` the kind for the kind-5 row and the component's byte
offset (0, 8) for the float2 rows, which is the shape the old `derived_mid`
emitted for one slot. So the copies derive from the channels plus the cbuffers
that carry them; only `x` (2 and 4) is still unexplained.

**The head's own shape**, read off the point of the byte dump below:
`{header word, descriptor count, count x 16-byte {name, flags, x, y} records,
then two trailing words}`. The trailing pair is `{5, 69}`, `{5, 69}`,
`{3, 69}`, `{2, 2}`, `{2, 2}` for the standard's five groups, and **both words
are now explained** (2026-10-12): the first is **the number of cbuffer
descriptors** in the head - kind 0/1 - which for group 0 is `global_viewport`,
`c_instance_offset`, `c_per_object`, `c_depth_only` and
`global_environment_settings` = 5, for group 2 `global_viewport`, `c_per_object`,
`D0BA3B10` = 3, and for groups 3/4 = 2 (all five match); the second is the
**first table's record count** (the shared engine pool's 69 when the group has an
engine table, else the material table's 2). So **every word of a head is
derivable**: header word, count, each descriptor's name/flags/`x`/`y`, and the
trailer.

**The rebuilt group data's layout** (a byte dump of `GroupData::build`'s output
for the standard, 10,725 B, matches the shipped blob): a `u32` group count,
then per group `{u32 query id, head}` + the two record tables in
`material_first` order (stride **20 bytes**, the 12-byte `between` between
them) + `mid` + `tail`. Group 0 is: count 4 B, query 4 B, head 208 B, engine
69 x 20 B, between 12 B, material 2 x 20 B, `mid` 2140 B, `tail` 97 B. The
engine records are one shared 69-record pool; a group with `has_engine = false`
writes only its material table.

The structured reader round-trips every tail's rest byte for byte, and the
input list is now generated from the compiled container:
`TailLists::set_inputs` hashes each element's name upper cased, groups the
records by name in the order the name first appears in the signature and sorts
each group by semantic index (the container itself orders by register packing).
Across five files - the four carried families and the built UI base material,
166 programs - the rebuilt list is byte-identical to the tail's
(`tail_inputs`), and every tail's rest round trips. `dtmt build` now rebuilds
each program's input list from whichever container it picks (`Tail::with_inputs`
in `build_device`), so a mod shader with different IO gets a tail that matches
its programs; rebuilding the UI base material leaves its data file byte for byte
identical (SHA256 D8354426...).

Cbuffer entries are `{murmur32(name), index, size, register, 1, 0}`: the size at
`+8` and the register at `+12` are confirmed across five families, and the word
at `+4` (small: 0/1/2/7) is the resource's **index in the group's descriptor
list** - `derived_cbuffers` writes it there and the standard family's split
cbuffer prefixes come back byte-identical.

### The tail block is the program's device-table stream

`block_census` (every program of the five carried families and the UI base)
shows the block is per program, not a copy of the device preamble:

- vertex programs end in a 12-byte all-zero block (`0 0 0`); 004F18EA's seven
  vertex programs share one block across five different interfaces;
- a pixel program that uses no channel or resource ends in a 32-byte all-zero
  block (38ECBAD1's second pixel program);
- every other pixel block opens `{2, 0, ...}` and then carries that program's
  device records: the material's cbuffer variable descriptors, its resource
  records and the channel records it uses.

The channel record is one structure written in two places: it appears
identically in the device preamble and in the pixel block that uses it, and
never in a vertex tail. 38ECBAD1 and 004F18EA both carry
`{name_hash, kind 4, count 1, 0x100, 0, 0x200, 0x10000, 0, 0x30000, 1, 0, 0,
0, 0x15, 0}` - 60 bytes, the same body for both materials; only the name (the
per-material channel instance, e.g. `texture_map_1453a433`) differs. Generating
the channel records therefore covers the preamble and every block that uses
them.

The preamble itself is `{1, group_count, cbuffer_count, ...}` plus a 120-byte
header and the record table: the UI base's table runs +0x78..+0x1F1 with its
channel record at +0x1F5 (561 bytes total), 38ECBAD1's is 1178 bytes with its
channel record in the last 60. A block opens `{2, 0, ...}` instead. Some blocks
end with the preamble body verbatim (UI base: 7 of 48; 004F18EA: 1 of 7;
2A04418E: 1 of 13; 3F08AC44: 1 of 13; 38ECBAD1: 0 of 2) after a per-program
prefix - the exception, not the rule.

The engine-prologue claim does not survive: `linear_depth` is in 38ECBAD1's and
004F18EA's preambles but not the UI base's or 2A04418E's, and
`global_diffuse_map`, `sun_shadow_map` and `fog_volume` are in none of the four.
The records are the shader's own resource usage.

The channel record's body is byte-packed and mostly constant. Across 683 records
from 200 blocks: `{name, kind, count = 1}`, then a component-count word at +12
that is 0x100 for one class and 0x300 for another (the material doc's reading of the
second word is 4 = RGBA and 3 = RGB, but the flag words are a per-channel class,
following `texture_format_spec.config`'s rules - not the texture format, which
lives in the texture resource), constants `0`, `0x200`, `0`, `0x30000`, `0`,
and two per-name fields - a word at +44 in {0, 5, 6} and one at +52 in
{0, 2, 4, 8, 0x15}. Kind 4 is textures (render-set names plus material
channels), kind 5 is material variables: `E503152C` is the sampler index that
the group data's c_per_object table opens with. Kind 6 also occurs (w3 = 2 or
0x100/0x300, w11 = 0 or 5, w13 = 8 or 0x1E); kind 3 does not occur in the
sample. Every name carries a fixed field set, so the flags are per channel. No
kind-2 record appears in any of the 200 blocks sampled, so the earlier "kind 2
is global_texture2D in the pixel blocks" note does not hold for this build.

The documented model is exact where it applies: of 500 sampled preambles, the 87
whose head reads `{1, query_count, 2, 0, record_count}` all satisfy
`length = 20 + record_count x 60 + 7 or 20`, with all-zero tails (62 have no
records, the rest 1 to 4). Their records follow the class rule below - in that
sample the class happened to equal the component count - a record is
`{name, components, 1, C1, 0, 0x200, C2, 0, 0x30000, C3, 0, 0, 0, C4, 0}`, where
components = 4 (RGBA) gives `0x300`, `0x30000`, `0x03000000`, `0x15` and
components = 3 (RGB) gives `0x100`, `0x10000`, `0x01000000`, `0`. So a channel
record needs only its name, its component count and its class (see below). The other 413 preambles are
the richer form (their third word is 1, 2, 4 or 5 and their fourth is nonzero).

The two forms are one structure. Reading the records from byte 16 rather than
120 makes every rich preamble parse cleanly (16,440 records, 0 malformed) and
the head's fourth word is exactly the record count - the "120-byte header" was
the 16-byte head plus its first eight records. The preamble is:

``text
{1, query_count, w2, config_count}                  (16 bytes)
config_count x {u32 index, u8 0, u32 value, u32 0}  (13 bytes each)
u32 stream_count
stream_count x channel records                      (60 bytes, or 73 for kind 5)
[a 7 or 20 byte zero tail - only in the minimal form]
``

Walking that over 500 sampled preambles succeeds on 479 (21 edge cases left to
chase). The config values are the documented small masks (1, 0xF, 0, 8, 0xFF, 6,
3, 4, 2, 5, 7, 0x78, 0x60, 0x20, and 0xC0000000 a few times), and the 22-record
common prefix matches the notes exactly (`12:1 16:1 15:8 19:8 10:FF 14:1 18:1
1A:1 11:FF 13:1 17:1 5E:F .. 65:F 0C:1 0E:4 ..`). The rich form ends right
after the channel records (tail 0 on 390 of them); only the minimal form carries
the 7 or 20 byte tail. Channel record component values seen: 4 (1681), 5 (932),
3 (15), 6 (17). The head's w2 is 1/2/4/5 and is still unexplained.

The block is a per-program header plus a per-program copy of the preamble's body.
Across 2000 blocks the head is `{2, 0, 0, 0, 0, 0, 0, 0}` in most, and the words
right after it are the section preamble's own head fields - the config count
(37/39/40/43) and then 30 - so the block is the preamble with a different first
28, 32 or 36 bytes depending on the program (the 36-byte form carries an extra
1 or 2 word). The UI base's first pixel program is the exact case: a 32-byte
header then `preamble[12..]` byte for byte. The other programs' bodies are their
own variants, which is why only 11% of the whole corpus ends with the section's
body verbatim.

The config records' index space, measured on 413 rich preambles: an engine
variable block (0x0C..0x1A, 474 records each), eight channel slots (0x5E..0x65,
427-503), and shader-specific entries (0x0D 972, 0x1E 776, 0x26/0x36/0x2E 573,
0x0C 524, 0x01 510, plus 0x03 and 0x05 carrying the flag values 0xFFFFFFFF and
0xC0000000). Indices repeat - 0x0D averages 2.35 records per preamble - so the
records are a list of (index, value) pairs rather than a map, and the values are
small masks (0/1, 2/5, 96/120/255, 0/7/15) apart from those two flag words.

The head's w2 is not the program, query, channel or context count: it runs 1..6
and the same counts pair with different values (w2 = 2 with 2, 8, 16 or 48
programs per stage; w2 = 5 with 4 to 56), so it likely counts something
declaration-side that a section does not carry.
The channel records' fields, measured cleanly by walking 573 preambles (2976
records, kinds 3/4/5/6): the second word is the kind (3 and 4 are the component
counts RGB and RGBA; 5 and 6 are other record types), the third is always 1, and
the body is `{kind, 1, C1, 0, 0x200, C2, 0, 0x30000, C3, 0, [X], 0, [Y], 0}`
where (C1, C2, C3) is one of three per-channel classes - (0x100, 0x10000,
0x01000000), (0x200, 0x20000, 0x02000000) or (0x300, 0x30000, 0x03000000) - and
the class is fixed per channel name (material channels take 0x100, render-set
textures such as linear_depth 0x300). Kind 3 is fully constant at the 0x100
class, kind 6 fully constant with X = 0x05000000 and Y = 8, and kind 5 varies in
the class plus X {0, 5, 6} and Y {0, 2, 4, 8, 0x10, 0x15}. So a texture record
needs only its name, its component count and its class.

The channel table, with names resolved through the dictionary: kind 4 is the
engine's render-set textures (`fog_volume`, `global_diffuse_map`,
`linear_depth`, `global_specular_map`, `brdf_lut`, `temp_gbuffer1` - all class
`0x300`); kind 5 is the material's own channels and variables (`orm`, `nm`,
`texture_map_<hash>`, the sampler index `E503152C` - class `0x100`) together
with a few engine resources (`sun_shadow_map`, `static_sun_shadow_map`,
`cached_local_lights_shadow_atlas`, `local_lights_shadow_atlas` - class
`0x300`). So a material channel is kind 5 class `0x100`, not a kind-4 texture
record: the earlier "kind 4 is textures, kind 5 is material variables" split
needs that refinement. One sampled preamble derails the walk (a wwise path hash
appears where a kind should be), which is one of the 21 edge cases.

The kind-5 material channels' last fields, per name: library-declared channels
(`orm`, `nm`, `bca`, `base_em`, `base_orm`, `em`, `mat_mask2`,
`environment_noise_map`) carry X = 0 and Y = 0x15; per-material instance records
(`texture_map_<hash>` and the sampler index `E503152C`) carry X = 0x05000000 and
Y = 4 or 8. So a generated section's material channel records are:

```text
declared channel: {name, 5, 1, 0x100, 0, 0x200, 0x10000, 0, 0x30000, 0, 0, 0, 0x15, 0}
instance channel: {name, 5, 1, 0x100, 0, 0x200, 0x10000, 0, 0x30000, 0, 0x05000000, 0, 4 or 8, 0}
```

The UI base's own preamble walks the model exactly (561 bytes): head
`{1, 36, 2, 37}`, configs ending at 497, stream count 1, one channel record at
501 - `{E503152C, 4, 1, 0x300, 0, 0x200, 0x30000, 0, 0x30000, 0x03000000, 0, 0,
0, 0x15, 0}`, the class-0x300 kind-4 shape - and no tail. Its 37 config records
carry the mined common prefix's indices (12..26, 94..101) plus shader-specific
ones (1, 3, 5, 8, 28-30, 38, 46, 54) and repeats with other values (12:0, 13:0,
30:1, 94:7); only index 10 of the new build's prefix is absent, so the common
prefix drifts between generations while the shape holds.

The UI base's 48 pixel tails differ only from byte 252 on: the 252-byte record
(the cbuffer list and the nine lists) is byte-identical across all of them, and
the block carries the per-program data. Three block shapes occur - 549 bytes
starting exactly like the section preamble from byte 12 (the 801-byte tails, 12
of them), the same plus a leading 4-byte `{2}` word (the 805-byte tails, 35),
and a 4-byte all-zero block (one 256-byte tail, a program with no channels). The
801s among themselves still differ, from byte 729 - a per-program record inside
the body, which is where the job-specific data lives.

That per-job data is a channel bitmask, and it is the only per-program field in
the UI base's pixel tails. The 12 801-byte tails are identical except for one
byte at 729 - the second byte of the word at 728, which is the value of the
config record for index 0x5E (the first channel slot) - and the 805-byte form
carries the same value four bytes later. The 48 programs cycle through six
masks: 07, 01, 02, 04, 08, 0F, each twice per twelve programs and offset by one
between the two forms. So the masks are the channel subsets a variant uses
(0x0F all four, 01/02/04/08 one each, 07 three), the engine binds only those,
and the multi-job mapping's per-job data is exactly this: the channel masks in
the block.

The block is the preamble's body with the per-program channel masks patched: the
801-byte tails' blocks differ from the body at exactly one byte - offset 477,
the value of the config record for index `0x5E`, the preamble's last config
record at 484 - and two are identical to the body outright. The 805 form is a
leading 4-byte `{2}` word plus the same, and a program with no channels gets a
4-byte all-zero block. So a generated section's programs need only their channel
masks; everything else in the block is the preamble's body byte for byte.

The walk's edge cases are type records, not channel records: a stream can mix
`{name_hash, kind, count, ...}` channel records with records whose first word is
a small type (0, 1, 2, 3, 21), and only the channel records' count is 1. Dropping
the count check and treating every record as 60 bytes except the name-first
kind 5 (73) raises the walk from 479 to 593 of 600 sampled preambles; the 7
remaining failures carry other shapes still to decode. The sampled stream's
records: kind 4 1958, kind 5 1084, type 0 38, type 3 24, type 1 23, kind 6 22,
kind 3 16, type 2 10.

The 7 remaining walk failures carry records with a name hash but a kind that is
not 5 and a count above 1 (for example `{89BFEFF1, 1, 4}` in a 788-byte
preamble), so their length is not 60 - they are the variable-length records the
notes mention ("can carry 64 bit hashes"). Everything else reads at 60 or 73
bytes.

### The stream's composition: the material's channels plus the shader's resources

A base material data file's own template carries the channel list (`channels` in
the SJSON, `unk1` in the layout, at `material_offset + 24`); an instance
material carries none (0 of 374 instances in a 400 sample) and states its
channels as `textures` keys, as the SDK's authoring materials do. Across 400
sampled base sections (`stream_probe`), 164 have their whole device stream
covered by that list, and the other 236 differ only by a small fixed vocabulary
of engine names: `fog_volume` (194 sections),
`global_diffuse_map` (160), `linear_depth` (129), `sun_shadow_map` (101),
`B70645F7` (70), `brdf_lut`, `global_specular_map`, `local_lights_shadow_atlas`,
`static_sun_shadow_map`, `cached_local_lights_shadow_atlas`, `894E960C` and
`9DC2E982` (45 each), `39A56531`/`D1D67F3B` (17), `70A51854` (12) and a few
rarer ones. Streams also drop channels: a material with 8 channels carries 5 of
them, so the stream is the base material's channels intersected with the
shader's own, plus the shader's engine resources. The remaining names in the
aggregate
(`00000000`, `00000001`, `00000200`, ...) are the type-record edge cases of the
walk, not names.

The slots come from the declaration's `samplers` table, not from `channels`:
the SDK's declarations declare the stage-exchange channels in `channels`
(`tsm0 = { type = "float3" domain = "pixel" }`) and the texture slots in a code
block's `samplers` (`{ sampler_state, source = "material" | "resource_set",
slot_name, type }`, nested under conditions). `source = "resource_set"` is what
the recurring names in the stream are - `global_diffuse_map`, `sun_shadow_map`,
`linear_depth`, `fog_volume`, `brdf_lut` are engine render-set textures the
shader reads - while a material's own slots are the `source = "material"` ones
it binds with `textures = { <slot_name> = ... }`. So the base list's derivation
is the material's textures keys, plus the active `source = "resource_set"`
slots, plus the active `source = "material"` slots the material does not bind:
the first term is exact today, the other two need the conditions resolved
against the material's variables.

The record shape differs by source, not only by name: the UI base's material
binds the plain slot name `texture_map` and its record is the kind-4 class-0x300
shape (X = 0, Y = 0x15), while a shipped material binding the instance name
`texture_map_b7091917` carries the kind-5 class-0x100 shape (X = 0x05000000,
Y = 8). The same shader ships both, so the kind/class follows how the material
binds the channel.

### The preamble's stream is the material's channel list

The stream is the material's own channels, one record each, in the material's
order: the UI base's material declares `channels = ["texture_map"]` and its
561-byte preamble carries exactly one stream record, `E503152C` =
murmur32(`texture_map`); another shipped material of the same shader carries
five records, one of them `texture_map_b7091917` (a graph instance channel). So
a generated preamble's stream is derived from the material, not carried.

The config records are per shader, not per material: the mod's carried UI base
preamble and a mined material of the same shader carry byte-identical 37-record
config lists (same order, same values). A generated shader therefore needs the
engine's index/value table once per shader.

The head's `w2` is still not identified. Falsified against 400 sampled
sections: the cbuffer count (97/400), the query count (69), the context count
(69), the contexts that carry queries (105), the condition record count (75),
the distinct program-block count (91), the distinct interface count (155), the
program/tail count (70), the stream count (35) and the config count (0). Its
distribution is 1 x49, 2 x190, 3 x4, 4 x38, 5 x101, 6 x14, 7 x4, and it is a
per-shader property (the two materials of one shader share it). Partial
correlations: w2 = 4 on all 38 sections with two contexts and w2 = 5 on 100 of
101 with three contexts, so w2 tracks the context count for the higher values
but not for 1-3. The leading hypothesis left is the pass count - a
declaration-side number the section does not carry, which would explain both the
context correlation and the 1..7 range. `w2_probe` prints `w2` next to the
counts so a new hypothesis can be tested the same way.

Across 489 rich preambles there are only six distinct config index sets: a base
of 1, 3, 5, 8, 12..26, 28, 29, 30 and 94..101 (389 of them), the same without
38, 46 and 54 (86), and four variants adding rows strided by eight -
31/39/47/55, 32/40/48/56, 4, and 38/46/54. So the index space is a grid whose
columns run eight apart (30, 38, 46, 54, ...) and a shader emits the rows it
uses: a generator can start from the base plus the channel row 94..101 and the
rows its own passes need.

### The config records are a base table plus per-pass overrides

The index-set variants are the union of a base table and the override blocks
that follow it. The base is 29 records, byte-identical across the sampled
shaders: `30:0 3:0 29:0 8:1 28:0 1:3 5:0 15:0 18:1 22:1 21:8 25:8 16:FF 20:1
24:1 26:1 17:FF 19:1 23:1 94:F 95:F 96:F 97:F 98:F 99:F 100:F 101:F 12:1 14:4
13:1`. After it come override blocks - a block re-states the indices whose value
differs for that pass - e.g. `54:1 46:6 38:5 13:0`, or `26:20 29:1 15:1 18:1
22:1 21:8 25:8 16:78 20:3 24:3 17:78 19:1 23:1 98:1 99:F 100:7 101:0 1:3`; a
shader carries 0, 1 or 2 of them. So the per-index value distributions measured
earlier are the base values against the override ones: 16 is `FF` in the base
and `78`/`60` in overrides, 20/24 are `1` then `3`, 26 is `1` then `20`/`40`/
`0`, 94 is `F` then `7` - and the `7` matches the per-program channel masks the
blocks patch. Index 38's `2`/`5` reads as a render-state-like enum.

What selects a block and sets its values is still open. The block count is not
the context count (69 of 400), the contexts with queries (105), the condition
records (75), the distinct program blocks (91), the program count (70) or the
stream count (35); the pass is the leading candidate, since a pass has a render
state and a channel set. `config_probe` dumps each section's config list beside
those features so the next hypothesis is a re-run.

### The config index space is the engine's global_viewport variable order

The engine table in the group data - the 69-record `global_viewport` run whose
first record is `6BC91D73` - is the order the config records index into. On the
UI base's own table: 16 `time`, 17 `delta_time`, 19 `sampler_lod_bias`, 20
`frame_number`, 21 `back_buffer_size`, 22 `output_rt_size`, 24 `taa_enabled`,
25 `jitter_enabled`, 26 `upscaling_enabled`, 28 `gamma`, and 1/3/5/8/12 are
`camera_pos` / `camera_inv_view` / `camera_last_world` / `camera_projection` /
`camera_last_view_projection`. So the base set is the engine's standard binding
set - the camera matrices, the time/frame scalars and the render flags - indexed
by position in the engine's table, and the index-to-name mapping is a table the
tool can carry once per build. The indices above the table (94..101) are the
shader's texture slots; their value (`F`, overridden to `7`) reads as a per-slot
uv-set mask, which is what the per-program masks (`01`/`02`/`04`/`08`/`0F`/`07`)
look like too. `engine_table <engine_data|material>` prints the table, so the
mapping is checkable after a game update.

Measured on all 400 sampled sections: 319 share the exact 29-record base, 11
carry a variant (`camera_pos` 1 instead of 3), and the other 70 are minimal
shaders with fewer records. The values read as masks rather than sizes or
offsets: `camera_pos` 3, `back_buffer_size` 8, `frame_number` 1, `time` FF.

**The minimal form needs no config records at all.** All 70 minimal sections -
the single-pass shaders with no override blocks - carry `w2 = 2` and a **zero**
config count: their preamble is `{1, query_count, 2, 0}` followed by the stream
count and records and a 7 or 20 byte zero tail. So a from-scratch single-pass
shader needs no config base, no override blocks and no `w2` question: the head
is constants, the stream is derivable (verified), and the tail is a zero run.
The config base as an engine constant is a rich-shader concern only.

The texture slots' `F` is a mask too, and the first reading of it - one bit per
interpolated uv channel - is **refuted**: across 30 sampled materials, 75 of the
300 programs with a patched mask carry a bit with no matching interpolator
(`mask=08` beside `CUSTOM 0 1 2`; the UI base alone cannot see this because its
interpolators are `CUSTOM 0..3`, so every four-bit mask is a subset there). The
masks still cycle `01`/`02`/`04`/`08`/`0F`, which is what the notes' reading
says: the bits are the shader's texture channels and a program uses one of them
or all. Which four things the bits name is open. `mask_probe` prints the mask
beside the container's `CUSTOM` indices; its diff also picks up blocks with
other patches, so the odd masks it reports (`00`, `05`, `10`, `28`, `5E`) are
its own artifact, not data.

### VT2 ships no shader43 sections, so the pass count has no oracle

The VT2 install (`E:\SteamLibrary\steamapps\common\Warhammer Vermintide 2`) was
opened to pair the SDK's declarations - whose pass counts are known - against
shipped sections and settle `w2`. It cannot: a VT2 material's stream is
`{version 43, 1, 24, material_size, shader_size, shader_offset, ?}`, and the
blob at `shader_offset` is a small resource-binding list, not a section (592
bytes on a 1.2 MB material, 64 on a 61 KB one; the first words read
`{5, 9CA38F7D, 3, 0, 1, 0, "LFSF", 4, 1, 1, 0, 9B8038E0, ...}` - `9B8038E0` is
`linear_depth`). VT2's compiled shaders are the older Stingray `Shader` format,
not `shader43`; a section carries its programs and would be hundreds of
kilobytes. The bundles hold materials, textures, units and resource packages
(a `common_shaders` package lists ~300 resources by hash) but no `.shader`
resources and no sections.

So `w2`'s pass-count hypothesis has no oracle in either game: Darktide ships no
declarations, VT2 ships no sections. The remaining ways to settle it are an
in-game probe that can observe a compiled shader (the engine does not rewrite
our section, so it would need an engine-compiled one) or a declaration-identity
pairing that does not exist yet.

### The group data constructor

`GroupData::template` and `GroupData::build` cut a shipped group data into the
bytes a generator carries and put them back around new tables. Per group the
carried bytes are the head (query id, header words, descriptors, the first
table's header), the bytes between the two tables (the second's header), the
bytes after them (the packed run and the condition header up to the 28-byte
record it shares with the packed run's last copy) and the tail (the rest of the
header). The two tables come in either order - the UI base runs
material-then-engine, the small families engine-then-material - so the parts
record which is first and `build` rewrites both count words.

Measured: the UI base's group data (62884 bytes, 36 groups) rebuilds byte for
byte from 8160 carried bytes, the engine's 1380-byte table and the material's
252 records; a rename of one record moves exactly 4 bytes and of a three-record
channel 12. Across 40 random sections 16 verify that way; the rest have no
template - their groups do not walk, their condition headers do not read (the
`2A04418E` case), or a group holds no engine run - so the constructor is exact
where the readers are and refuses where they are not instead of guessing.
`group_build` runs the check and `group_parts` prints the part map.

Wired into the engine data and `generate`: `EngineData` carries the template
(the `group_template` table: the prefix, the engine's table only when it is not
the toolchain's, the deduplicated material tables, the deduplicated group parts -
a group's query id is not stored, the contexts carry it one to one and in the
same order - and one entry per group) instead of the whole `group_data`, and
`generate` rebuilds the group data from it. A section whose groups do not walk
keeps the `group_data` blob, so nothing regresses.

The text form is the Stingray source dialect (`key = value`, `{}` tables, `[]`
arrays, `"quoted"` hex blobs), the same dialect `.shader_node`, `.shader_source`
and `.material` are written in, so an engine data file is a source file beside
them rather than a format of its own; `EngineData::looks_like_text` tells it
apart from a material data file. Snoopy-mod's file was converted on 2026-09-30
and the build is byte-identical with it. The dialect's tall layout (a table
field or array element per line) costs bytes - 15605 -> 24189 on the UI base's
file - which the derivations below take back.

The file also stopped carrying three derivable things:

- **the containers**: a mod with sources compiles its own, and
  `generate_shader --engine-data --no-containers` leaves them out; the build
  fails loudly if the sources go missing;
- **the dependency**: it is the engine's one library (the renderer path's hash),
  so `generate` writes the constant unless the file carries something else;
- **the blocks' repeated bytes**: a tail is written as its lists plus its block,
  and a block that is the preamble's body with a few bytes patched - the UI
  shader's are a 0- or 4-byte header and one patched byte - is written as that
  diff.

Measured on snoopy-mod's `ui_default_base.engine_data`: **218950 -> 15605
bytes**, with the build producing a byte-identical material data file
(`FCD0965B...`) and the text round trip intact. The engine's `global_viewport`
table (`lib/sdk/data/global_viewport.hex`) and the 30-record standard config base
(`lib/sdk/data/config_base.hex`) are now toolchain constants, so a mod never
carries them; the conditions blob is dropped, verified in game; the dependency
is written from the constant; and the containers are compiled from the module's
own sources. `engine_data_check <file> [<source material>]` runs the round trip
and the group-data comparison. What is left: the tails' resource lists (the
cbuffer entries are derived now; the index rule is decoded, the names come from
the declaration's `samplers` and the bindings from the source's registers), the
contexts and groups (the engine needs them consistent with the shader's compiled
structure - see below), the per-program masks and `w2`.
(The section's identity word is no longer on this list: it is derived from the
material path.)

### The section's identity word is murmur32 of the owning material's path

The header's second word - `opaque` while it was unidentified, now
`material_hash` (a working name: no VT2 SDK `core/` name or engine-extraction
string grounds it yet, and named fields are working names until one does) - is
murmur32 of the resource path of the material whose shader the section is.

Measured: 40 of 40 sampled sections carry distinct values; 2094 mined base
sections carry 2094 distinct identities. The UI base's section carries
`439A40FC` = murmur32(`content/ui/materials/backgrounds/splash_screen_partner_logos`),
its own material: the mined data file `bundle/data/e3/e3370cb2107d8aca` is that
material's own file, named by murmur64 of the *file* path
(`.../splash_screen_partner_logos.material`) - which is why the dictionaries,
keyed by resource paths, missed it. An earlier parent-material guess is
falsified: the file's `parent` field is 0.

The toolchain derives it: `EngineData::generate` takes the generating material's
resource path and writes murmur32 of it, and snoopy-mod's file lost its `opaque`
line. Verified in game: built with `4CC21B79` =
murmur32(`materials/mods/snoopymod/ui_default_base`) it renders the title, and
the derived build is byte-identical to that tested one.

### The tails' cbuffer entries are derivable from the group data and the source

`tail_sources` checks every program tail's cbuffer entries against the section's
own group data: the name from the group header (`c_per_object`) or the cbuffer
descriptor (`global_viewport`), the size from the table's records - the largest
`offset + size`, rounded up to the 16-byte granularity the tails record. On the
UI base's source (`e3370cb2107d8aca`) all 144 entries match, including
`global_viewport`'s 1788 rounded to 1792.

Its index word follows the resource records' rule: word 1 is the cbuffer's index
in its group's descriptor list (matched against every group, since the
program-to-group mapping is not decoded). [48 sections: 2569 cbuffer entries,
2569 match; word 4 is 1 and word 5 is 0 in every entry, and word 3 is the
register, 0..5.] So a tail's cbuffer list is fully specified from the shader's
own cbuffer declarations (per stage, in register order - which the build has),
the group data's descriptor list and tables, and the engine table. `tail_build`
builds it and checks it against the shipped tails: **144/144 entries on the UI
base's section** (names and registers parsed out of `ui_default_base.shader_source`,
sizes 240 and 1792 from the two tables).

**Dropped from the engine data file.** A tail now writes the names it carries
(`cbuffers = [ "189663B5" ]`) and its `lists` start after the constant-buffer
list; `from_text` rebuilds the 24-byte entries - the index from the group's
descriptor list (every group must agree, since the program-to-group mapping is
not decoded), the size from the material table or the engine table, the register
from the order, and the 1/0 constants; a tail whose cbuffers are not the
engine's own keeps the whole bytes in `lists` (no `cbuffers` field, the carry
form), and a file that names them without a template is refused. snoopy-mod's
file went **24169 -> 23260 bytes** and the build is byte-identical.

**The lists are by role.** A tail's lists are written as a table - `engine`(list 2), `textures` (3), `buffers` (5), `samplers` (6), `sampler_arrays` (8) -
each record one hex string, and the lists that are always empty (0, 1, 4) and
the **inputs** (7) are not stored at all: the reader writes their zero counts
and the build rebuilds the inputs from the compiled container's signature - a
container with no signature is refused rather than written with no inputs. A
region the reader cannot split is carried as raw hex (the carry form). The UI
base's vertex tail is therefore `lists = {}` and its pixel tails carry only
`textures`/`buffers`/`samplers`/`sampler_arrays`. snoopy-mod's file went
**23260 -> 23007 bytes** and the build is byte-identical.

### The block's mask byte, first table

`tail_build` prints it: the byte a block patches into the preamble body, from the
UI base's shipped section. 40 of the 48 pixel programs patch it, the other 8
leave the body's 0; every vertex program leaves it. The values are 01, 02, 04,
08 and 0F - single bits and all four - in a pattern: 01 01 02 02 04 04 08 08 0F
0F, then two programs with 0, then the same, then 01 02 04 08 0F twice with one
program each. The note's earlier "07" does not occur in this section. So the byte
is a per-program channel set over a small vocabulary; which program gets which
set is the next correlation, with the declaration's passes and their conditions
the obvious candidates.

### Notes from Polychromatic's reverse engineering (2026-09-30)

[Polychromatic's colour
notes](https://github.com/Wobin/Polychromatic/blob/main/docs/how-each-effect-is-coloured.md)
are a second, independent extraction of the material layer. What they ground for
this work:

- **Names.** `lighting_far_range` (a `c_per_object` scalar some colour shaders
  ignore, hijacked as a live colour carrier), `lerp_color_a`, `color_a`,
  `beam_color` (material vectors), `offset_time_duration` (a burn timing
  vector), `rainbow_barrels_hue` (a custom shader export), and the shader
  variants `const_mask`/`const_hsv`/`live_hsv`. These are engine-extraction
  names, so they are the grounded form the naming convention wants.
- **Reflection rows are per material.** "Each child needs its own reflection
  row because children do not inherit the parent's" - which is what the group
  data's material table holds, and what a build writes back per material. A
  custom export costs one row (+24 bytes per child), so the table's content is
  the material's own variable set, not the base's.

  Decoding snoopy-mod's own table with that in mind: its 7 rows are the
  `c_per_object` layout - `texture_map` at 0 (kind 5: the sampler index),
  `texture_map` at 4 (float2: the texture index), `texture_map` at 16 (the
  second texture), `view_proj` at 32, `world_view_proj` at 96, `world` at 160
  and `dev_wireframe_color` at 224 (the material's own graph export). The first
  three are the material's `texture_map` slot feeding three offsets, the three
  matrices are the engine's standard fields, and the last is the material's.
  So the table is the material's reflection: the engine's standard rows plus
  what the material binds, and a next candidate for removal from the engine
  data (its inputs are the `.material`'s bindings and the engine's standard row
  names).
- **The format is 62** and a format-61 material crashes the current engine -
  matching what this toolchain reads and writes.
- **Shaders can be patched to read a cbuffer scalar they ignore**
  (`lighting_far_range`), so a shader's cbuffer schema is not necessarily fully
  used by its programs - worth remembering when a tail's cbuffers look wider
  than the source's reads.
- The mask byte is not addressed there: their "channels" are colour channels
  (vertex colour routing, red-channel routing), not the config record's channel
  set.

The other cbuffers a shader's tails name are its own, and they resolve as
shader-source names: `c_billboard`, `c_material_exports`, `lighting_data`,
`c_depth_only` - the VT2 reference's "cbuffers are declared in HLSL inside the
output nodes". So a tail's cbuffer list is the group data (the two it holds)
plus the shader's own declarations, with the register order from the source, and
it needs no container reflection. The resource lists are the next measurement:
their names are the declaration's `samplers` (`resource_set` and material) plus
the engine's bindless arrays, and their bindings/spaces look like an engine
convention rather than per-shader data.

### The resource records: names yes, index/set/binding not yet

`tail_resources` dumps every program tail's nine lists across a sample. The
shapes are confirmed: the 7-word records are `{name, ?, binding, flag, set,
FFFFFFFF-or-size, 0}` (a buffer's byte size replaces the array sentinel - 64 for
`18DEAD01`, 4 for `3181096C`), the 4-word list 6 and 3-word list 8 are the
samplers (`{4B42C5E6, 0, 1, 1F}` = `static_minlod_sampler` at space 31), and
list 7 is the signature run.

The names resolve to the engine's render-set textures (`9B8038E0` =
`linear_depth`, `DB7E5380` = `global_diffuse_map`, `87088C15` = `fog_volume`,
`B85584A2` = `sun_shadow_map`), the bindless arrays (`3AFC636C` =
`global_texture2D`, `41B1CFF8` = `global_feedback_buffers`, `DA560F03` =
`global_samplers`) and the shader's own resources - so the names are the
declaration's `samplers` plus the engine's vocabulary.

What is *not* derivable yet is the second word and the set: they follow the
shader's own resource table, not a fixed convention. On the UI base
`global_texture2D` carries 2 and the UAV 3, matching the group data's table
order (`c_per_object`, `global_viewport`, `global_texture2D`,
`global_feedback_buffers`), but across the sample the same engine texture
carries different second words in different shaders (`linear_depth` is 11 in one
and 3 in another), and the sets range over 0, 2, 9, 10, 12, 31. So a tail's
resource lists need either the container's reflection or a declaration paired
with its own section - the corpus has neither, so this stays carried.

### The decompiler names the interface, so the source carries it

`shader43 --reconstruct` now names the decompiled declarations from the tails:
the constant buffers by register (per stage - the vertex stage's `c_per_object`
is b0 where the pixel's is b1) and the resources by the register letter, space
and binding the tail records imply (lists 3/4 textures, 5 UAVs, 6/8 samplers).
The renames apply as whole words, since the body refers to the same identifiers.
On the UI base's source the result declares

```hlsl
cbuffer c_per_object : register(b0, space0)          // vertex
cbuffer global_viewport : register(b0, space0)       // pixel
cbuffer c_per_object : register(b1, space0)          // pixel
Texture2D<float4> global_texture2D : register(t0, space2);
RWBuffer<uint> global_feedback_buffers : register(u0, space31);
SamplerState global_samplers : register(s0, space2);
SamplerState static_minlod_sampler : register(s0, space31);
```

and `--compile --against` matches both interfaces. So the source now carries the
interface by name, which is what lets the build derive the tails' cbuffer and
resource lists from it instead of the engine data.

### The load needs the structure; the conditions tree it does not

`synth_engine_data` builds reduced wrappers from a carried engine data; the
in-game results (title-screen test) are:

- **the conditions can be dropped.** Keeping the carried contexts, groups,
  preamble and all 96 programs but rewriting every query's conditions offset to
  `FFFFFFFF` and emptying the blob loads and renders (one run, the title
  material applied, no `dispatch_loadtime` error). So a shader that does not
  permute anything needs no conditions tree, and the tree writer leaves the
  from-scratch critical path;
- **the program set is load-bearing.** Keeping everything else but reducing the
  96 programs to the first pair crashes at `dispatch_loadtime` on the group's
  query id. Reducing the contexts and groups to one context, one query and one
  group crashes the same way whether the preamble and blocks are the carried
  ones (561 bytes) or a minimal synthesized pair (100 bytes). So the engine
  expects the section's contexts, groups and programs to be consistent with the
  shader's own compiled structure, and a reduced wrapper is not accepted even
  when it is internally consistent.

The corpus agrees that no-permutation shaders carry no conditions: 70 of the
sampled sections are single context, single query, `w2 = 2`, zero config records
and a zero tail.

### A resource record's second word is its descriptor's index

A group's prefix is three words - `{query_id, an unidentified word (0x130 and
0x390 measured), count}` - and its descriptor list is the `count` 16-byte
entries that follow at +12: `{name_hash, flags, X, Y}`, `X` the entry's byte
offset in the per-draw binding table (24 bytes for a constant buffer, 8
otherwise). A 7-word resource record's word 1 is the resource's **index in the
descriptor list of the group its program belongs to**.

`resource_table` matches each program against every group's list, since the
program-to-group mapping is not decoded, and requires a single fully matching
group. On 27 sampled sections: **355 programs with 7-word records, 355 fully
matched by one group; 1004 records, 1004 matched** - no exceptions. The UI base
has 36 groups whose four entries are the same (`c_per_object`,
`global_viewport`, `global_texture2D`, `global_feedback_buffers`), so all 48
programs match any group and the 96 records all land. `eb09dd77` splits 26/26/2/2/16/16
across six groups (groups with identical lists cannot be told apart);
`3F08AC44` 7/7/1/2/2 across five. So the index is derivable from the descriptor
list the build generates, and with the names and registers now in the source the
resource lists stop being carried.

How the earlier readings were wrong: entries "at +8, +24 and +40" were a
different sample's header words; the list "at 32" with word1 = `1 + index` fit
because the UI base's `c_per_object` at 16 is the entry it skipped. The count
word at +8 is what fixes the boundary exactly; the previous probe's heuristic
(a first word above 0x10000, `X` advancing by 24 or 8) is gone.

The per-group program counts are also a lead on the program-to-group mapping
(the multi-job blocker): a section with distinct lists per group partitions its
programs among them (e.g. `eb09dd77` 26/26/2/2/16/16, `3F08AC44` 7/7/1/2/2),
though identical lists make it ambiguous.

### The UI base's program order, measured

`program_order` prints the section's shape: 96 programs alternate Vertex and
Pixel; every vertex program carries the *same* tail (one distinct vertex tail,
its lists `[0,0,0,0,0,0,0,3,0]` - three input records, POSITION/COLOR/TEXCOORD),
and the 48 pixel programs share 19 tails (`[0,0,0,1,0,1,1,4,1]` - one
`global_texture2D`, one UAV, one sampler, four inputs, one bindless sampler
record; the four inputs are SV_Position/CUSTOM/CUSTOM1/CUSTOM2, the mod's
source's structs). The (tail, mask) triples over the pixel programs:

- programs 1..23: tails 1..12, masks `00 01 01 02 02 04 04 08 08 0F 0F 00`
- programs 25..45: tails 1..11, masks `00 01 01 02 02 04 04 08 08 0F 0F`
- programs 47..93: tails 13..18, four rounds of `00 01 02 04 08 0F`
- program 95: tail 19, no mask

So the mask is a channel set (a single bit or `0F` for all), the same mask pairs
*different* tails (2 and 3 both `01`, 4 and 5 both `02`, ... - the tail carries
the interface, the mask the channels), the vertex side is constant, and the 48
pixel programs decompose 12 + 11 + 24 + 1. The 36 groups equal 12 + 11 + 6 + 6
+ 1, which is suggestive but not a rule yet.

### Deleting the engine data: the map

The file goes away when every field is an SDK constant or derived from the
sources. The state, field by field:

**Already out**: containers (compiled from the sources), the dependency (the
engine's one library), conditions (dropped, verified in game), the section's
identity word (murmur32 of the material path), the constant-buffer *entries*
(the names are carried for now), the tail *inputs* (rebuilt from the compiled
container's signature), the always-empty lists 0/1/4, the blocks' bodies (the
preamble's body).

**Derivable now, largest first**:

1. ~~**`groups[].query`** (~0.9 KB)~~ **done**: the contexts carry the query ids
   one to one with the groups and in the same order (that is what `group_starts`
   walks), so the file does not store them; the reader takes them by position,
   and a file whose contexts lack one keeps its own (the fallback).
1b. ~~**The blocks' repeated heads** (~1 KB)~~ **done**: the tails' blocks share
   four distinct heads, so they are a `block_heads` pool the diffs index rather
   than a hex string per block. The file went 14409 -> 13384 bytes.
2. ~~**The tails' remaining lists** (~9 KB)~~ **done**: they derive from the
   containers' own reflection. The containers are SM6 DXIL with no RDEF chunk
   (`SFI0, ISG1, OSG1, PSV0, STAT, HASH, DXIL`), but `lib/dxc` now calls
   `IDxcUtils::CreateReflection` + `ID3D12ShaderReflection`, and that gives the
   per-stage attribution the source text could not: the vertex binds only
   `c_per_object` (b0, space 0); the pixel binds `global_viewport` (b0),
   `c_per_object` (b1), `g_material_samplers` (s0, unbounded, space 2) and
   `g_material_textures` (t0, unbounded, space 2). The lists are built from that
   plus the engine's conventions - an unbounded texture array is
   `global_texture2D`, an unbounded sampler array `global_samplers`, and a
   sampling stage gets `static_minlod_sampler` and `global_feedback_buffers` -
   and are no longer stored: a tail that splits stores none of them, the reader
   writes the empty counts, and the build fills them. **A split tail now stores
   only its block**: the constant-buffer *names* are flushed too (the reflection
   supplies them in register order, so `derived_cbuffers` writes the entries),
   and the whole tail is carried only when its constant buffers are not the
   engine's own or its lists do not parse. snoopy-mod's file went **22107 ->
   14409 bytes** and every built asset is byte-identical - below the old line
   format's 15605, with far less carried.
3. `group_template` (~6.7 KB): `materials` from the `.material`'s bindings plus
   the engine's standard rows (decoded: the 7 rows over `c_per_object`); the
   framing parts (heads/betweens/mids/tails, ~1 KB) from the shader's
   descriptors plus the tables' own headers and extents (`build` already
   rewrites the count words); the prefix is nearly constant.

**`materials` is derived (2026-09-30, later).** The 7 rows decode as one
texture slot per sampled channel (`texture_map` kind 5 at 0, kind 1 at 4, kind 1
at 16), the engine's `c_per_object` matrices (`view_proj` 32, `world_view_proj`
96, `world` 160, all kind 4 size 64) and the graph's exported variables from 224
(`dev_wireframe_color` kind 3). `build.rs` emits them from
`Evaluation::samplers` + `Evaluation::exports` plus the engine's fixed rows, the
text's `materials` list is gone, and the built table is **row-for-row identical**
to the carried one (kind/hash/offset/size); the game material-sets in 16 s. The
new `ShaderOverrides.material_records` seam is what the per-group rebuilder will
use. What remains of `group_template` is the framing: the group head (84 B -
node, header and the descriptor list), the two 12-B gaps and the condition tail
(46 B).

4. `preamble` (~0.4 KB): the stream is the material's channel list (measured
   164/400 exact plus a fixed engine vocabulary); the config records are per
   shader but have only six distinct index sets across 489 preambles.

**The preamble is the device block (2026-10-01).** The 561-byte device preamble
parses under the existing `shader_block` model exactly: a 120-byte header, 29
13-byte config records (`{index, pad, value, pad}`), then the channel stream - a
count word and one 60-byte `texture_map` record. The same bytes sit in **every
program tail** (`block[32..] == preamble[12..]`, 549 bytes), so deriving it
derives the tails' trailing blocks too. `preamble_check` prints the parse, and
`BlockTemplate::from_preamble` + `build_block` already generate the shape from a
declaration - the module is present but **orphaned** (only its own tests use it).
The deletion path is thus: the header's three varying words (groups, cbuffers,
records+8) computed, the stream built from the material's channels, and the 29
records from one of the six index sets - the last piece still to decode.

**The config records are a fixed vocabulary plus a small append (2026-10-01).**
Grouping the corpus (`config_probe` over the game's `bundle/data`, 227k
materials): a base set of ~31 records appears in nearly every section, and the
varying part is a handful of appended rows. Our UI-base family (96 programs, 2
cbuffers, contexts `F2760503`+`E2C8865F`) is the base plus
`30:1 54:1 46:6 38:2 12:0 13:0 94:7`. The indices form the grid the probe's
header describes - base columns 28..30, optional rows eight apart (31/32,
38..40, 46..48, 54..56), channel row 94..101 - so the appended rows track which
channel rows are live. Deriving the preamble is therefore: the base vocabulary,
the append selected by the material's channels, and the header's three counts.

**The append's selector is the cbuffer count (2026-10-01).** Grouping the base-
prefixed corpus proves it: the config set does *not* track the channel count (the
base appears with 0..19 stream records), but the append tracks the constant
buffers. Cbuffers 1-2 - our family - append
`30:1 54:1 46:6 38:2 12:0 13:0 94:7`; 2-3 append `30:1 54:1 46:6 38:2`; 3-8
append two eight-spaced row groups (`30:1 54:1 46:6 38:5 13:0 54:1 46:6 38:2
13:0`); 4-6 add the `29:1 26:... 16:78 20:3 24:3 17:78 ...` variant. The rows run
eight apart (38/46/54), one group per constant buffer, with `12:0 13:0 94:7` as
the channel-row marker - so the append is computable from the section's cbuffer
list, which the tails already carry.

**The preamble is derived (2026-10-01).** The engine's block template lives in
`shader_engine_block` as the one engine constant of the device data - it cannot
be synthesised, a generated minimal block fails at shader load - and an absent
`preamble` in the file now means exactly it (the reader inserts it, and the
writer omits it when it matches). With `preamble`, `heads`, `betweens` and `mids`
all out, the engine data text is **12,389 bytes** (from 13,384) and the built
section is **byte-identical** (`7348B400A52BCB1F`) and `RENDER_OK`. The harness
also samples twice now: a slow boot (the deployment grows the bundle database)
can still be on the loading screen at the first sample, and the clearer frame
wins.

**What `tails`, `block_heads` and the group `tail` do when absent (2026-10-03).**
The reader requires all three - a group names a tail (`a group names a tail at
#0, which is missing`) and a program names a tail (`a program names tail #0,
which is missing`) are hard errors - so a from-scratch material cannot build
without them yet. But the *program* side is already stored in its minimal form:
a `TailText` keeps only its `block`, and the constant-buffer and resource
**lists** are derived at build time from the compiled container's own reflection
(`build_device`), which is why the split tails in the file carry no lists. What
stays carried there is only the **block** - the preamble body plus a per-program
head drawn from the `block_heads` pool (three distinct heads here), i.e. the
per-program mask the notes still list as the open correlation. The group `tail`
is different in kind: it names the source chunks the permutation's conditions
reference (`gui`, `gui_hdr`, ...), so a from-scratch shader must generate it from
its own source graph - which is why zeroing it renders black.

The distribution, measured on the UI base: **20 distinct tails**. Every one of
the 48 Vertex programs shares tail 0, whose block is the minimal 12-byte
`000000000000000000000000`; the 48 Pixel programs use 19 tails drawing on three
shared heads (head 0 x12, head 1 x11, head 2 x24, plus the raw block x1). So the
tails are **per-permutation**, not per-program, and the `block_heads` pool is the
small set of per-pass mask variants one permutation picks from. The remaining
decode is exactly that pick plus the heads' semantics (their leading `02` and the
trailing `01`/`02` word).

**The program blocks are load-bearing (2026-10-03).** Collapsing all 96 programs'
blocks to the minimal 12-byte zero block - the shape every Vertex program uses,
and the one 108 of 136 corpus sections carry - builds a smaller section
(394,800 B) but **crashes** the renderer (`access violation`). So the blocks are
the programs' device-table streams and cannot be flattened. What the corpus does
show: a universal 12-byte zero Vertex block (hash `6C9A7A05`, in every section
measured) and a small family of Pixel shapes - all-zero 28/32 bytes,
`02`-prefixed 16/36, a 52-byte one - plus, for some shaders, a full
preamble-shaped block. The pick and the mask remain the open decode.

**`materials` is redundant in the file (2026-10-03).** The build derives the
material table from the declaration, so the file's `materials` pool is
overwritten anyway: removing it leaves the built section **byte-identical**
(`7348B400A52BCB1F`, `RENDER_OK`) and the text at **12,082 bytes** (from 12,389).
`contexts` cannot go the same way yet - it carries the *shader's* declared
(context, query) pairs (the UI base's 30 `default` values + 6 `gui_render_pass`
passes), while the mod's declaration has one pass, so a derived contexts blob is
one query and would not match the group data's 36 groups. Removing it is the
full multi-permutation enumeration, not a redundant-pool deletion.

**What the 36 queries are (2026-10-03).** `default` carries 30 and
`gui_render_pass` 6, all `conditions = FFFFFFFF`. Verified: they come from no
plausible key (the UI base's path, the shipped material path, `default`/`gui` x
the key shapes, under `high32(murmur64)`), none is in the dictionary, and a scan
of the whole mod tree finds them **only inside the section** - the material and
its sources reference none of them. So they are the shipped UI-base *shader's*
declared variant values (the engine's surface/material-axis enum, per the earlier
reading), not a function of our declaration. The runtime needs only the one query
the engine demands (the reduced one-query section renders), so the other 35 are
the family's declared table rather than a runtime requirement - which is why the
enumeration is the one carried pool with no derivation path from our sources yet.

**A from-scratch section works (2026-10-03).** The last blocker was ordering: the
reader filled a group's query id from the *text's* contexts, which a from-scratch
file does not have, so it refused (`group 0 has no query and the contexts carry
only 0`). The group head's query now takes a zero placeholder in the reader and
is filled in `generate` from the contexts **it** derived - the engine-demanded
`6FA3FCCF` for our declaration. With that, `ui_default_base.engine_data` needs no
`contexts`, `permutations`, `materials`, `heads`, `betweens` or `mids`; it is
**11,444 bytes** and the built section (`1792F33AFAD2B4E6`) is **`RENDER_OK`**.
What a from-scratch file still carries: the per-permutation tail blocks +
`block_heads` (load-bearing device streams) and the group `tail` (the shader's
own source references), plus `group_template`'s framing for the groups it ships.
5. The blocks' per-program bytes: the head plus the mask byte (a channel set
   over `01/02/04/08/0F`), whose per-pass source is the open correlation.

**Blocking deletion** (all structural - the shader's compiled shape):

- the **contexts** and the **group and program order**: which queries exist,
  which group each program belongs to, and the order of both. The declaration's
  contexts x passes x conditions must produce them (the multi-job mapping);
- the **mask byte** and the blocks' heads, per program;
- **`w2`** (the preamble head's third word; the leading hypothesis is the pass
  count).

### The contexts, decoded

The section's contexts are the *shader's own* declared contexts, not the
material's: snoopy-mod's section carries two - `default` with 30 queries and
`gui_render_pass` with 6 (both names grounded: murmur32 of the strings, in the
dictionary). Every query there has `conditions = FFFFFFFF` (none), and the
queries are *not* in the dictionary and *not* among the conditions tree's
hashes, so they are the engine's (context, value) pair ids - the variants a
context supports (for `gui_render_pass`, its six passes; for the shipped
`default`, thirty values that look like the engine's surface or usage types).

That is the structural data the file still carries, and the deletion path is the
declaration: a from-scratch shader's `shader_contexts` names its contexts, and
the values each supports are the section's queries. The missing piece is the
mapping from the declaration's contexts and passes to the queries, the groups
and the programs (48 passes x 2 stages is 96 programs, but 30 + 6 queries is 36
groups, so the counts do not line up yet). The shipped section's queries came
from the game's own compiled shader, so a mod that starts from a shipped section
carries them; a from-scratch one must *choose* them.

**The minimal-section probe.** `minimal_section` reduces the engine data to one
context (`default`), one query (`5A5A5A5A`), one group and two programs - 9904
bytes against 421696 - and the game **finds the shader by our derived
identity**: its crash context names `shader #ID[4cc21b79]` (murmur32 of the
mod's base material path, which `generate` derives) under `material
#ID[62a04071]`. It then crashes, and the dump names the inner handle it wanted:
`SHADER(0xf036c545f9dfc27b, 2, PS, SINGLE, ...)` - a 64-bit permutation key for
a PS with no defines. So the identity word is the engine's *shader* handle and
the queries/programs are the *permutation* slots under it: the engine picks a
permutation (stage + defines) and the section must serve it. The bisect from
here: reduce *one* axis at a time from the working full section (programs first,
with the contexts and groups kept) and read the dump's `SHADER(...)` line when
it breaks.

**Both structural axes are load-bearing.** The follow-up runs: reducing only
the *programs* (contexts and groups shipped) crashes at
`stingray::D3D12RenderDevice::dispatch_loadtime` asking for `#ID[28b0ab00]` -
the first context's first query - so the engine looks queries up by id and the
section must pair each with its own programs. Reducing the *contexts and groups*
(programs shipped) gets past that lookup and crashes inside the shader. A
section with one query is forgiven a *missing* query (the crash moves deeper
in), but a *present* query whose programs are gone is fatal. So the structure is
the engine's compiled pairing, and neither axis can be reduced freely.

**The conditions tree is the source graph.** `conditions_walk` parses the blob
as what it is - 35 records of `{u16 tag=1, u16 words, u16 payload_offset, u16
count}`, `count` hashes then `words` u16 children whose top nibble is an op (2,
1, 7, 5, 9 seen) and whose low bits an index - and its hashes are murmur32 of
*source* names: `9FCFE126` = `gui`, `BC4EE226` = `gui_hdr` (two hits of the 744
names mined from the VT2 SDK's sources, against the tree's 82 distinct words).
So the tree is the shader source graph (the includes), not engine-side data -
the user's correction holds.

**The queries are the last unknown**, and the key: they are not the murmur32 or
murmur64 (either half) of any of the 874k dictionary names nor of the 744
source names, and not any hash of the tree's records or children. The candidate
that fits is an id the engine's compiler assigns per *pass*, hashing the pass's
own fields (`layer`, `code_block`, `render_state`, its defines) from the
declaration - which a from-scratch declaration would let us compute. The
decisive test: build a section holding one context, one query with a chosen id
and two programs, run it, and read the id the engine demands in its log; then
match that id against the declaration's pass fields.

### The query ids are derivable: the hash is pinned (2026-09-30, later)

The engine's identities are **MurmurHash64A (seed 0) over their canonical name
string**. Two independent confirmations, from the VT2 SDK's own data compiler
(`bin/stingray_win64_dev_x64.exe`, run offline on the SDK's example mod):

- 263/264 entries of its `debug_file_index.sjson` satisfy
  `filename == murmur64a(name)`: `gui:diffuse_map:one_bit_alpha.shader_library`
  -> `1ae3055122b27b54`, `linearize_depth.shader_library` -> `0205d79d3be91999`,
  `gui_gradient:diffuse_map:gradient.shader_library` -> `01b5b82d032ec091`, ...
- The compiler logs the canonical permutation keys as it compiles:
  `[ShaderCompiler] Compiling shader: `gui:DEPTH_TEST_ENABLED:DIFFUSE_MAP:ONE_BIT_ALPHA``
  - `<shader>:<defines, uppercase, colon-joined>`; the full key appends the
  platform/renderer (the VT2 string tables carry
  `<shader>:<context>:<defines>:PLATFORM_WIN32:RENDERER_D3D11|D3D12`).
- The **query id** stored in a compiled shader library is the **high 32 bits of
  `murmur64a(full key)`**. Verified: `murmur64a("gui:default:DIFFUSE_MAP:ONE_BIT_ALPHA:PLATFORM_WIN32:RENDERER_D3D12") >> 32`
  = `E39F328F`, which appears in `gui:diffuse_map:one_bit_alpha.shader_library`
  twice, each time in a record of the shape
  `<id> <count> <word> 02 00 00 00 4D 3F 72 28 ...`; likewise
  `...:CIRCULAR_MASK:UV_SCALE:PLATFORM_WIN32:RENDERER_D3D12` -> `2B314E79` in
  `gui_gradient:diffuse_map:circular_mask:uv_scale.shader_library`.

So the ids are a function of the *declaration*: shader name, context, the
material's defines, platform/renderer. The `renew` probe's crash therefore only
proves those particular values were wrong - not that the ids must be carried.

Caveat: the VT2 compiler is an older engine. None of our 36 Darktide ids equals
the hash (any of the four forms) of any of the 876,995 collected
VT2/dictionary strings, so Darktide's key strings - its define vocabulary and
context naming - are still missing. Next: (a) run the reduced-section probe with
a *controlled* material and read the id the engine demands in
`dispatch_loadtime`, then fit the key string (the hash is now known); or (b)
mine Darktide's own shader-library resources and brute-force their keys with a
Darktide vocabulary.

**The rule schema, confirmed in the SDK's own nodes (2026-09-30, later).** The
VT2 SDK ships 187 `.shader_node` files whose rules generate the macros, e.g.
`{ if: "num_skin_weights() == 2" define: { "macros": ["SKINNED_2WEIGHTS"]
stages: ["vertex"] } }`, `defines={ macros: ["BILLBOARD"] ... }` on passes, and
`permute_with` links between options. The engine binary carries
`PLATFORM_WIN32/PS4/XB1/XB12/LINUX` next to a render-profile enum (the
`RENDERER_*` tokens) and the murmur64A constant (92 sites); `gui_render_pass` is
absent from VT2 entirely (engine and shaders) but is in the Darktide
dictionary (`E2C8865F...`), so it is a Darktide engine context - consistent
with our sections.

Darktide's own sources are not reachable from the shipped data: no
`*.shader_node`/`*.shader_source` in the bundle database, none under the SDK's
paths as loose files, none in `packages/boot`, none in `bundle/build_output`;
`shader_cache.hans` is opaque. (The mod's old `core/stingray_renderer` tree was
copied from the VT2 SDK, not extracted from Darktide.)

`permutation_search` (mode landed) exercises the rule: `--ids`/`--hashes`,
shader names, contexts, and sorted define subsets. ~133M candidate keys built
from the VT2 vocabularies so far produced one hit
(`title_screen_background:gui_render_pass:skinned_2weights:uv0:write1:
PLATFORM_WIN32:RENDERER_D3D12` -> `9678EEC6`, the first `gui_render_pass`
query). With ~1.1 chance collisions expected over that search, one hit is *not*
evidence; its `title_screen_background` token cannot be the shipping section's
material either.

**The probe is the oracle**: compile a simple material from our declaration and
a section whose query ids are the values the rule computes for candidate keys.
If the engine renders, the rule is confirmed; if it fails, the log prints the
id it wanted, and that pair (our known declaration + its id) fits the key. The
`SHADER(0xf036c545f9dfc27b, 2, PS, SINGLE, ...)` handle from the earlier `min`
probe is not `murmur64a` of any key built from our material's obvious tokens;
it remains unidentified.

**The probe ran, and the rule held (2026-09-30, last).** `probe2` - a min-style
section (one `default` and one `gui_render_pass` context, 32 query ids *computed*
by the rule from candidate keys over {`materials/mods/snoopymod/ui_default_base`,
`ui_default_base`} x {`SINGLE`, `HAS_BASE_COLOR:SINGLE`} x {lower, upper,
byte-reversed}) with the shipped two programs - **reached the material stage**:
the log shows `[snoopymod] material set: background_image -> ...`. So the engine
found the permutation it needed among our computed ids: the id derivation and
the key format are confirmed in game.

The failure then moved *past* the id lookup to the programs - `E_INVALIDARG,
assert: shader '#ID[6fa3fccf]'` - which is expected: the section reuses the
shipped two programs for every permutation. Next: bisect the 32 candidates to
the one the engine accepted (halving runs), then enumerate the permutation sets
(from the node rules) and pair each with its own compiled programs.

**The winner, and the exact key form (same day, later).** That assert named the
id the engine was building a pipeline for, and it is exactly our computed one:

`high32(murmur64a("materials/mods/snoopymod/ui_default_base:SINGLE:PLATFORM_WIN32:RENDERER_D3D12")) = 6FA3FCCF`

So the accepted key is: the lowercased shader/material path, then the
define/render-state tokens upper-cased and sorted (`SINGLE` - the same token the
engine's own `SHADER(...)` dump carried), then `PLATFORM_WIN32:RENDERER_D3D12`;
there is no context token, and the id is the plain high 32 bits (not
byte-reversed). The search over the shipped 36 ids found nothing, which is
consistent: those were compiled for another material with its own define sets.
The ids that matter are the ones the *engine demands* for our shader, and those
are enumerable - the `SHADER(...)` complaints and this assert name them.

The engine then failed the pipeline (`E_INVALIDARG` on that same id) because the
section served the shipping programs for a different permutation. Each
permutation therefore needs its own compiled programs: the remaining half is
compiling the code blocks per variant (the node rules) and pairing them.

### The render regression (2026-10-01)

A build that rendered is now crashing, and it is not the derivations. The exact
`ui_min_single.engine_data` that drew the tint and wave for **104 s** at 16:09
(rebuilt, deployed and launched in the current tree) crashes **~0.8 s** after
`material set`, in `ShaderTemplate::initialize` - as does every build since
~16:58. `Y` and the group node are **load-bearing**: zeroing the descriptor `Y`
fields or setting the node to an arbitrary `0x12345678` both crash the same way,
and setting the node *equal to* the context query crashes earlier, at
`dispatch_loadtime`. So a section needs the shipped `Y` `{0, 5, 10}` and the
shipped node `28B0AB00`; neither is derivable yet.

Steam revalidation (`steam://validate/1361210`) restored the pristine files
(`bundle_database.data` 16421204, the boot bundle, `settings_common.ini`
`boot_script = scripts/main`), and a clean deploy from that state (db ->
16481012) **still crashes** the 16:09 engine data. So the game files are not the
cause; the change is in the toolchain or the deployment. Suspects: dtmt's
compiled programs (the build still says "Generated", 9904 bytes) or dtmm's deploy
writing the patched `packages/boot` bundle into the deployment. Next: a bisect
over the dtmt/dtmm commits (`8758c94` and earlier vs now) with
`ui_min_single.engine_data`, reading whether `ShaderTemplate::initialize`
completes.

Caution: every "material set" success reported between 16:58 and now is the Lua
assignment only - the render itself was crashing. The in-game verdicts for the
derived contexts, prefix and material table therefore need re-running once the
regression is cleared.

**The structural floor (2026-10-01).** A downward bisect from the full control
(36 queries, 2 contexts, 96 programs), keeping every other byte shipped, found:

- queries/groups reduce freely: N=18, 9, 4, 2 and **N=1** all `RENDER_OK`. One
  query is enough - the min-section's failure was never the group count.
- programs do not: N=1 with **8, 24 or 48** programs fails at
  `dispatch_loadtime`, the error context naming the very query kept
  (`shader #ID[28b0ab00]`); **96** (48 vertex + 48 pixel) works.

Bisecting upward pins it exactly: 72, 84, 90, 93 and 94 programs all
`RENDER_FAIL`; only the complete **96** renders. So the demanded program for
`28B0AB00` sits at the *end* of the list, and the program list must be complete.

So `ShaderTemplate::initialize`/`dispatch_loadtime` index the program *list by
position* for the query, and the query-to-program-slot mapping is the last
structural unknown: query `28B0AB00` (the first `default` query) is served by a
slot in the upper half, not by the first pair. A section that keeps one query
but all 96 program slots renders - the minimal shape a derived section can
target once the slot mapping is modelled.

Also fixed while chasing this: `dtmm`'s deployment hashes are written `h<hex>`
(a bare digit-leading token lexed as an integer and made the file unreadable,
including by dtmm), and `minimal_section` gained an `n <count> [programs]` mode
(first N queries/groups, and the first N/2 programs per stage) for this bisect.

**The regression was the *reduced* section, not the toolchain (2026-10-01,
resolved).** Once the harness classified the crash, the discriminator was the
original verified control: the full section (421,696 B, 36 groups, carried
contexts + derived tables) reports **RENDER_OK** - material set, benign unload
crash at t=101 s - while the reduced one-group min-section reports RENDER_FAIL in
`ShaderTemplate::initialize`. So the environment, toolchain, programs, `db` and
PSO cache are all sound. The earlier "a build that rendered is now crashing"
reading was wrong: the 16:09 min-single run almost certainly still had the
*control* deployed (that era's dtmm failed the min deploy silently and the old
script only checked the Lua line), so the reduced section was never shown to
render.

Two consequences. The **material-table derivation is validated on the real
section**: the control build substitutes the derived records (replicated across
all groups, which the shipped family's identical tables make exact) and renders.
And the from-scratch work must **keep the full section structure**: the reduction
to one query/group is what `ShaderTemplate::initialize` cannot survive, so the
next step is deriving the contexts/ids *within* the full section (the
multi-permutation enumeration) rather than shrinking it.

`docs/scripts/shader-render-test.ps1` is the harness that keeps this honest: it
classifies the crash (`RENDER_OK` / `RENDER_FAIL` / `NO_LOAD`), treats only the
known unload crash as benign, and prints an environment snapshot (section hash,
db size, boot_script, pso size and mtime, deployment bundle count) with every
verdict.

**The framing is decoded; `Y` is the blocker (2026-09-30, later).** The group
head reads `{group count, library hash, 0x130, descriptor count, descriptors…,
0x02, material table count}`. Everything but two fields is derivable: the
descriptor names come from the shader's resources (`c_per_object`,
`global_viewport`, `global_texture2D`, `global_feedback_buffers` here), `flags`
is `space << 16 | kind` (0 material cbuffer, 1 engine cbuffer, 3 texture, 5 UAV),
`X` is the byte offset in the per-draw binding table (24 per cbuffer, 8 per
other, in list order), `0x130`/`0x02` are engine constants and both table counts
are rewritten by `build`. The one blocker left in the head is `Y`, the packed
2-bit-per-slot per-program binding counts (`{0, 0, 5, 10}` here, `{0, 5, 10}` /
`{0, 1, 2}` per permutation in the shipped UI base).

**The group head's first word is the query, not a library hash (2026-10-01).**
The full control's 36 groups each start `00ABB028`, `17B45F9B`, `B44912CB`,
`D585A0D0`, … - the contexts blob's 36 queries, one per group and in order. So
the head's first word is **derivable** (it is the group's query), and the earlier
"shipped library identity" reading was wrong. It also explains the old pm6 crash:
setting the node equal to the context query was *correct*; that build only had
two programs, which is what `dispatch_loadtime` actually wanted. The framing is
1838 bytes per group either way:

```
head    84   query + 0x130 + descriptor count(4) + 4 descriptors + 0x02 + table count
table  140   the material rows (derived)
between 12   F0 40 <engine table count>
engine 1460  global_viewport (73 records)
mid     96   07 00 00 00 / 0 / 3  +  3 packed 28-byte copies
tail    46   the condition header
```

The packed copies are the three `texture_map` rows re-emitted as
`{hash, a, b, offset, kind, B5639618, 0}` (offset, kind and hash all match rows
0-2); `a` and `b` are the template's own constants, below.

**`between` and `mid` are derived (2026-10-01).** Both turn out to be engine
templates, not per-material data. `between` is two constants (`F0`, `40`) then
the engine table's count word, which `build` rewrites. `mid` is the engine's
packed template: one three-record entry per texture slot (a kind 5 row), each
record `{slot hash, a, b, offset, kind, B5639618, 0}` with the slot's channel
name hashed into all three - so it derives from the material table's kind 5 rows,
`a`/`b` being the template's own constants. Removing both pools from the control
(13,384 -> 13,120 bytes) builds a **byte-identical** section
(`03D2B050B67DC530`, 421,816 B) and reports `RENDER_OK`.

**The tail is load-bearing (2026-10-01).** Setting every group's tail to its
all-zero variant builds and the game runs - and reports `material set` with no
crash - but the title screen is **black**: the material is not drawing. So
`material set` + survival is not a render verdict, and the harness now samples
the screen (mean luminance; near-zero is `RENDER_BLACK`, screenshot saved).
The tail carries the shader's condition/source references (`9FCFE126` = `gui`,
`BC4EE226` = `gui_hdr`, `625D415E`, ...), so it is not an engine constant a
from-scratch section can replace with zeros; it has to name the section's own
sources, which is the same source-graph data the notes list as the open area.

**The head is derived too (2026-10-01).** The descriptor list comes from the
section's own resources under the engine's names - `c_per_object`, then
`global_viewport`, then `global_texture2D` and `global_feedback_buffers` when a
stage samples - with flags by role, `X` cumulative (24 per constant buffer, 8 per
other) and `Y` the engine's packed usage counts. `Y` turns out to be **loose**:
`{0, 0, 5, 10}` and `{0, 0, 1, 2}` both render (shipped groups 0-11 use the
first, 12+ the second); only `0` breaks. Deriving with the constant
`{0, 0, 5, 10}` therefore differs from the control only in groups 12+'s `Y` and
is **`RENDER_OK`** (section `7348B400A52BCB1F`). The engine data text is down to
**12,770 bytes**. The one framing part left is `tail`: the end of the condition
header, per-permutation and tied to the conditions tree.

Side-finding: the engine's 32-bit ids are `high32(murmur64)`, confirmed by
`high32(murmur64("default")) = F2760503`, the `default` context header. The SDK's
`Murmur32` is that convention, not classic MurmurHash3-32.

**The declaration drives the contexts (2026-09-30, last).** The engine data no
longer names the variants. `EngineData` still accepts a `permutations` block for
a rebuilt/foreign section, but when a material's declaration is available the
build *derives* the block: the `.shader_node`'s compile jobs grouped by context,
each query the job's macro set, and `generate` hashes those macros by the pinned
rule. The token belongs in the declaration, so the UI base's pass now writes
`defines=["SINGLE"]` in its `.shader_node` (the VT2 convention: passes carry
`defines`, e.g. `MOTION_BLUR`/`SUPPORTS_FOG`/`CALCULATE_LIGHTING`); the engine
data carries no contexts, no `permutations` and no subject query. The build is
**byte-identical** to the carried version (10024-byte file, the 20-byte
`default` context with `6FA3FCCF`), and the game material-sets in 16 s - a strict
replacement.

**The group data's first word (2026-09-30, later).** Our built section's group
data starts `24 00 00 00` and `Section::check` reads that as the group count
(rejecting the section: "the contexts carry 1 queries and the group data has 36
groups") while the game renders it. `GroupTemplate.prefix` is exactly those four
bytes - the shipped UI base's group count (30 + 6 queries), which the reduction
to one group left stale. The engine finds the group through the context's query
id, not this word, so the reduction is tolerated. A **derived** template has to
write it (either the group count, or confirm the engine ignores it), and
`Section::check` has to stop treating it as a hard equality for reduced sections
before `group_parts`/`group_build` can run on them.

**Settled: the word is the count, and the group's node is not the query
(2026-09-30, later still).** Writing `1` renders (material set, 16 s), so the
first word is the group count and a derived template writes the real one. The
reader and the tools were then unblocked: `Section::check` no longer asserts
`first context query == group hash` (a shipping convention, not a rule) and
`GroupData::group_starts` falls back to the group data's own hash when a single
query id is not in the data.

The fallback was forced by a **measured negative**: the next word, the group's
node (`28B0AB00` here), must stay the shipping value. Deriving it as the
context's query id (`6FA3FCCF`) - which the shipped sections' invariant suggests
- puts `6FA3FCCF` in both places and the engine **crashes at
`dispatch_loadtime`** (error context `shader #ID[6fa3fccf]`). With the node left
at `28B0AB00` and the context query `6FA3FCCF` it renders. So the section holds
*two* distinct identities: the group node (the shipping group's own hash, still
to be derived) and the context query (the engine-demanded permutation id). The
`group_starts` walk that assumed they are equal is why the tools had been unable
to open our own builds.

### Notes from RainbowFlame's reverse engineering (2026-09-30)

[RainbowFlame's RE
diary](https://github.com/Vansinnet/RainbowFlame/blob/main/REVERSE_ENGINEERING.md)
is a third, independent extraction, of the particle/material/shader chain. What
it grounds or confirms:

- **The header's field names**: `version`, `opaque` (their name for what we call
  `material_hash`), `contexts_offset`/`context_count`, `conditions_offset`,
  `default_data_offset`, `dependency_offset`/`dependency_count`,
  `group_data_offset`/`size`, `device_data_offset`/`size` - word for word our
  layout. Their section version 43 is *ours* too: `rdef_dump` confirms the
  containers parse at 43 (the 60-62 in the material header is the *material
  stream*'s version, not the section's).
- **The device framing**: `u32 envelope` (observed 1), `u32 frame_length`, the
  frame, `u32 metadata_kind` (observed 5), `u32 decoded length`, `u64 frame_key`,
  then the metadata tables and opaque state - ours exactly.
- **The frame key is `MurmurHash64A(frame, seed=0)`** over the whole frame
  (`8c 06`, the quantum header and the payload; the envelope and metadata
  excluded) - what `encode_frame`/`generate` compute. [their eight records]
- **The material's trailing section** is their "following section"; ours is
  `unk2_data`, a candidate for a better name.
- **A profile observation, not a law**: their 32-program impact shader has eight
  resource groups with the colour pixel shaders at indices 1, 5, 9, ... - one
  quad per group - and the flamer's 48-program parent has twelve colour pixels,
  again every fourth. `program_census` over 41 sampled sections says the ratio
  is not universal: 4-per-group 13 times, 2-per-group 10, 8-per-group 4, then
  1.75-9; V = P in almost all (exceptions: `0f4688a9` 14/18, `25b4747f` 4/5).
  The UI base is 2 contexts, 36 queries, 96 programs - 48 passes x 2 contexts
  fits - so "programs = passes x contexts" is the better lead, with the groups
  coming from elsewhere.
- **The material buffer carries the descriptor indices**: "the pixel shader
  obtains descriptor indices from material data at runtime", i.e. the
  `c_per_object` rows our `materials` table decodes to (the `texture_map` slot
  at 0/4/16).
- Their shader clock is the viewport constant at byte 1440 (`c90.x`) - the
  `global_viewport.time` our UI base's source also reads at 1440.

### The road to deleting .engine_data (2026-10-03)

Goal: a purely from-scratch `.material` + `.shader_node` + `.shader_source` +
`.texture`, with no `shader_engine_data = "…"` side file. What the side file
still holds and what each needs:

**Already derived** (the file no longer names them; verified in game):
contexts/queries via the declaration's compile jobs (when the file carries no
`contexts`), the context/group pairing, the material table, the group head, the
group framing constants (`between`, `mid`), the device preamble, and - as of
2026-10-03 - **the program list and its tails**: `derived_programs` emits the
family's whole `(head, seam, mask)` table (the UI base's 48 pixel slots, read via
`family_table`), and with `programs`, `tails` and `block_heads` removed the built
section is **byte-identical** (`F3EE2651139FDEB0`, 421,508 B) and `RENDER_OK`.

**Left in the file:**

1. ~~`programs` / `tails` / `block_heads`~~ **done** (see above). The table is
   currently the UI base's recorded one; generating it for *any* family is the
   enumerator in "Priority order".
2. ~~**`group_template`'s group list and `prefix`**~~ **done** - the group list is
   written as `repeat` and the count word derives from it (2026-10-04).
3. ~~**`context_count`**~~ **done** - the count is the plans' contexts when the
   file names them (2026-10-04).
4. **the group `tail` pool** - proven **load-bearing even with empty conditions**
   (zeroing it renders `RENDER_BLACK`), so it is the same source-graph decode as
   the conditions tree, not a constant. The text form writes each tail as its
   condition-hash list (2026-10-04), so the pool is six names, but it is still
   carried - deriving it is the conditions-tree/enumerator work.

**The permutation plan is derived, not carried (2026-10-04).** `dtmt build` reads
the declaration's compile jobs and builds the plans (`permutation_plans`), so a
file with no `permutations`/`contexts` and no plan-named contexts takes them from
the declaration. The mod keeps its plan because the declaration's single
`default` pass is narrower than the built section's UI-base family shape (six
tails, 36 groups): a declaration-derived plan is one query and would mismatch the
group list. Widening the declaration to the family's passes/conditions - the
enumerator - lets the plan derive too, after which the file is the tail pool
alone.

**The one hard requirement**: `ShaderTemplate::initialize` wants the *program
list* complete - reducing it to the demanded program fails `dispatch_loadtime`
while the complete list renders - so a generated file must emit all 96 (or as
many as the declaration's jobs produce). That is generation, not a smaller file.

**The program and tail construction, measured (2026-10-03).** The 96-program list
is fully regular:

- **every Vertex program is the same 12-byte zero block** (`tail 0`);
- the program list is `(Vertex, Pixel)` pairs, 48 of them, the pixel side cycling
  a 6-entry tail run as the context alternates;
- a **tail block = `head` + the preamble body (549 B) + one mask byte**. The heads
  are generated padding - `02` then zeros (32/36 bytes, three variants) - and the
  **only content is one byte at body offset 477**, whose values are
  `0, 1, 2, 4, 8, 15`: the per-program **channel mask** (body[477] reads `7` in the
  preamble, the shared default). Reconstructing `head + body + mask` reproduces
  the exact block lengths `preamble_block` reported (581 for head 0, 585 for head
  1), so the rule is confirmed byte for byte.

So `programs`, `tails` and `block_heads` are all a function of the permutation
list (`(context, permutation)` positions) and the mask - no shipped data. The
generator is `EngineData::derived_programs`, which emits one `(Vertex, Pixel)`
pair per permutation slot with the vertex's zero block and the pixel's
head+body+mask. Remaining before it replaces the carried list: pick the head per
permutation, and the mask from the permutation's definitions.

**Why the slots cannot all be one program (2026-10-03).** A generated list
substituting our single program into all 96 slots **crashes** (and it is the
shader, not state: reproduced at the same `db` size the good runs use). The
reason: each shipped pixel tail is *distinct* - its head and its mask byte are the
pass's own - so whichever permutation the engine asks for, it gets a block whose
mask matches that pass's channel usage. One block in every slot claims the wrong
mask for 47 of the 48 pixel passes, and the engine faults. So the slot **count**
and the head variants are engine constants, but the **mask is per permutation**:
a correct generated list needs the declaration to enumerate the permutations (48
passes x 2 contexts for the shipped family), not the single pass we declare today.
The generator therefore needs the multi-pass declaration to feed it; with one
`SINGLE` pass there is one mask and the list cannot be correct.

**The generator's shapes are now exact; only the per-slot data is missing
(2026-10-03, `program_diff`).** Diffing `derived_programs` against the carried
list pins every part:

- the **vertex tail is 52 bytes** = `4` prefix + nine empty list counts (`36`) +
  the 12-byte block - and the generator now matches it byte for byte;
- the **pixel tail is `4 + 36 + head + 549`** = 621 (head 32) or 625 (head 36),
  the block being `head + preamble body`.
- the heads are **32 and 36 bytes** (not the 32 I first used), and a tail with
  **no patch** keeps the body's default mask byte (`7`), so the mask is written
  only for tails that override it.

So the *construction* is solved; what the generator cannot invent is **which head
and which mask each slot uses** - the plan in our file lists 48 identical
`SINGLE` queries, which carries no such information. A correct generated list
needs a declaration that distinguishes the passes (the multi-pass enumeration),
which is the next real step.

**Families differ in size and mask set (2026-10-03, `family_probe`).** Reading
four shipped families' sections:

| family | contexts | programs | mask bytes used |
| --- | --- | --- | --- |
| UI base (`007bf44b`) | `F2760503 E2C8865F` | 96 | `1, 2, 4, 7, 8, 15` (8 each) |
| `01788473` | `F2760503 5852A5B1 3100C3D2` | 32 | `8` |
| `0028686a` | `F2760503 9E1FDE62 3100C3D2` | 4 | `6` |
| `01991281` | `F2760503` | 2 | (none - its device data is not preamble-shaped) |

So the pass structure is **family-specific and usually far smaller** than the UI
base's 96, and the mask set differs per family. That settles the approach: carry
**one pass table per engine family**, read from a shipped material of that family
(as the UI base's was), and let the generator emit each family's programs from
its table. Nothing needs the shipped *declaration* - each table is observable
from a shipped section. The 2-program family also warns that not every family
frames its device data the preamble way, so a family's shape has to be checked
rather than assumed.

**The UI base's pass table is readable (2026-10-03, `family_probe`).** Walking
each program's own tail gives the 48 pixel masks exactly:

```
07 | 01 01 02 02 04 04 08 08 0F 0F 07 07 | 01 01 02 02 04 04 08 08 0F 0F 07 |
01 02 04 08 0F 07 | 01 02 04 08 0F 07 | 01 02 04 08 0F 07 | 01 02 04 08 0F 07 | -
```

The domain is `01 02 04 08 0F` (single channel bits and all four) plus **`07`**
the body's default (a tail with no patch). The pattern is symmetric and
deterministic, so it can be carried as this family's pass table verbatim. The
other families read mostly unresolved because their device body is **not 549
bytes** - each family needs its own body length before its table can be read, which
is the concrete next step for particle/standard support.

**Families differ structurally, not just in masks (2026-10-03, `device_shape`).**
The device preamble's second header word is the family's own count, the
pre-program region scales with it, and the per-program tail lengths differ:

| family | header `{1, a, b, ..}` | pre-program | programs | vertex tail | pixel tails |
| --- | --- | --- | --- | --- | --- |
| UI base | `1, 36, 2` | 866 | 96 | 136 | 1154 / 1158 |
| `01788473` | `1, 5, 6` | 1710 | 32 | 88-228 | 557-2058 |
| `0028686a` | `1, 1, 2` | 1085 | 4 | 184-296 | 168-1280 |
| `02cbef80` | `1, 5, 5` | 1009 | 30 | 64-228 | 240-1562 |
| `01991281` | `1, 1, 2` | 40 | 2 | (no preamble body) | - |

So the vertex tail is **not a universal 12 bytes** (the UI base's is 136), and
the pixel tails follow each family's own repeating run. The construction
principle holds - `split base + head + body + mask` - but every parameter
(prefix, lists, body length, head sizes, mask run) is **per family**. A family is
therefore a small recorded table of its own; `derived_programs` needs the
family's table, not one global shape.

**The standard's blocks are not preamble-derived (2026-10-12).** `split base +
head + body + mask` describes the UI base's family; the standard's 14 tails (12
raw hex, 2 diffs) do **not** contain the preamble's body anywhere - the body's
first bytes `30000000 1D000000` appear in none of them - and the 11 non-trivial
blocks share **no common prefix at all** (0.5 B). They are independent
per-program record streams, several of odd length (585, 617, 687, 791, 807,
823), so they are not even word-aligned. Their openings are `{0|2, ...}` -
`0, 3, 10, ...`; `0, 2, 3, 0xFFFFFFFF, ...`; `2, 0, 2, 2, 1, ...`. So the standard
needs its own grammar for the block (the program's own device records), not a
family table of the preamble shape.

**The `477` mask belongs to the UI base only (2026-10-03, named families).**
Resolving real materials by name and shape:

| material | family shape | pre-program | programs |
| --- | --- | --- | --- |
| `content/fx/materials/weapons/force_staff/force_staff_arcs` | `1, 1, 2, 0x27` | 1072 | 4 |
| `content/parent_materials/substance_basic_wd` | `1, 10, 5, 0x30` | 936 | 60 |
| `content/parent_materials/decal_aoe` | `1, 1, 2, 0x26` | 754 | 4 |
| `content/fx/.../autogun_muzzle` | `1, 1, 2, 0x27` | 866 | 4 |

Dumping a small family's device data shows why the UI base's model does not carry
over: its preamble header is `1, 1, 2` with **no 36-group list**, its pre-program
region is plain data, and its tails are **ordinary program records** (envelope 1,
frame magic `8c 06`, the container's ISG1 signature visible in the bytes) - there
is no 549-byte body and therefore **no mask byte at 477**. So the `head + body +
mask` construction is the **UI base's** shape; a small family's tail is the
program record plus its own interface, and the mask concept does not apply. Each
family's generator is its own small shape, which is what makes "one table per
family" the honest model.

**Small families' tails are ordinary tails (2026-10-03, `program_records`).** The
particle (`force_staff_arcs`) and standard (`substance_basic_wd`) programs' tails
open `{u8 phase 01/02/03}{constant-buffer count}{hash, size}` - i.e. the same
`count + cbuffer entries + lists + block` tail structure the UI base's programs
use, just with the family's own cbuffer set and without the UI base's shared
prelude body. So a small family needs no new format: its programs are ordinary
tails, and the existing tail machinery (`Tail`, `TailLists`, `build_device`'s
reflection fill) already models them. The UI base is the special case (the shared
549-byte prelude and its mask byte), and it is the one that needs the recorded
table.

**The UI base table is wired (2026-10-03).** `UI_BASE_PASS_MASKS` records the 48
masks, and `derived_programs` uses them: with a 48-query plan the generator emits
96 programs whose **vertex tails are byte-identical** (52 B) and whose pixel tails
have the **right lengths** (621/625, heads alternating 32/36). The only residual
difference is **4 bytes at tail offset 72** on the 36-byte-head pixels - the seam
between the head and the body, which needs the 36-byte head's own bytes rather
than a `02`+zeros reconstruction. That is the last step to a byte-exact generated
`programs`/`tails` for this family; the SDK's `shader_engine_data` tests stay
green with the table in place.

**The tables are measured and the last field is identified (2026-10-03).** A tool
(`program_diff`, now emitting the carried family table as Rust arrays) confirms the
mask and head-size tables exactly, and shows the residual: the 36-byte head's
4-byte seam word at tail offset 72 is **not constant** - it is `02 00 00 00` on
some slots and `01 00 00 00` on others (programs 3 vs 47). So the family table is
`(head size, seam word, mask)` per pixel slot, all three read off a shipped
section. With that third field the generator should be byte-exact; the tables and
the tooling are in place, only the per-slot seam value needs recording.

**The pass/per-permutation tokens are the shader's own `#if defined(…)`
branches (2026-10-03).** `copy`'s declaration declares one pass
(`{ hlsl_shader="copy" defines=["SAMPLE_RGBA"] render_states="filter" }`) and one
`compile` entry (`{ defines=[""] }`), yet **10 `copy:` keys** compile:
`copy:clear`, `copy:flip_y`, `copy:encode_rgbm`, `copy:red_to_rgb`,
`copy:point_sampler`, `copy:cube_capture`, `copy:project_to_far_plane`,
`copy:alpha_to_rgb`, `copy:red_to_rgb:gamma`, `copy:cube_capture:encode_rgbm`.
Their tokens are exactly the `#if defined(FLIP_Y)` / `#if defined(RED_TO_RGB)` /
`#if defined(ENCODE_RGBM)` branches **inside the shader body** - so the compiler
permutes the code block over the conditions the source tests, and each branch
becomes a token in the key, sorted with any declared defines.

That closes the rule end to end:

- **key** = `<shader>` + the tokens, sorted and lowercased, colon joined;
- **tokens** = the entry's declared `defines` **plus** the `#if defined(…)`
  conditions the shader body branches on (its `permutation_sets`/source `#if`s);
- the entry's **condition** (`on_renderer(…)`) is evaluated, not part of the key;
- there is no separate context field.

This is the same shape as the UI base's 96 programs: its passes each contribute a
shader + render-state and the source's conditionals expand the set.

**The token set comes from the block's condition tables, not just `#if`s
(2026-10-03).** Reading `utilities.shader_source`'s `copy` block shows the three
places a token is introduced:

- the block's `code` tests it: `#if defined(FLIP_Y)`, `#ifdef ALPHA_TO_RGB` /
  `#elif RED_TO_RGB`, `#if defined(CUBE_CAPTURE)`, `#if defined(COPY_HALF)`, …;
- the block's tables nest under it: `samplers = { ndefined_INTERLEAVE_BUFFER = {
  defined_POINT_SAMPLER = { … } ndefined_POINT_SAMPLER = { … } } defined_MASKED = { … } }`
  - the `defined_X` / `ndefined_X` prefixes are the positive/negative condition
  keys, the same shape this project's `.shader_node` reader already models as a
  channel/sampler entry's `conditions`;
- the shader's own `compile`/`static_compile` entries add declared `defines`.

So the compiler's token candidates are every condition name the block tests or
nests under (`defined(X)` and `ndefined_X` both name `X`), and it permutes the
ones the pass does not already fix. Our own `ui_default_base.shader_source`
contains only `STAGE_VERTEX`/`STAGE_FRAGMENT` (compiler built-ins), so its block
contributes one token set - the 96 programs come from the **inherited UI-base
family**, not from our source. Building a VT2-faithful enumerator is therefore a
matter of walking a `.shader_source` block's condition keys, which the SDK's
reader already parses for the `.shader_node` dialect.

**The VT2 declaration -> key rule is proven from both sides (2026-10-03,
`vt2_key_check`).** The compiler ships both its inputs (the SDK's
`.shader_source` files) and its outputs (`debug_file_index.sjson`), so the rule
can be checked directly. Two declaration mechanisms feed the same key rule:

- **`static_compile`** entries: `{ if: "on_renderer(D3D11, D3D12)" shader="sun_flare"
  defines=["RADIAL" "BILLBOARD_CAMERA_ALIGNED"] }` compiles to
  `sun_flare:radial:billboard_camera_aligned.shader_library`. The condition is
  *evaluated* (not part of the key); the shader name and the sorted lowercased
  defines build it. This alone matches **150 of the 220** compiled keys.
- **passes** (inside `shader_contexts`/shaders entries): a pass names an
  `hlsl_shader` plus its `defines`/`render_states`, and those identifiers become
  the tokens - which is why the "context" token of a key mixes define-like
  (`diffuse_map`) and state-like (`clear`, `flatten`, `shadow_mapping`,
  `write_mask`) names. Those are the 70 keys `static_compile` does not cover
  (`copy:clear`, `apply_hdr_transparent`, `bright_pass:eye_adaptation`, ...).

So the key is `<shader>` + the entry's tokens, **sorted and lowercased, colon
joined** - with the tokens coming from the entry's `defines` and (for a pass) its
render-state/branch identifiers. There is no separate "context" field: the first
position is simply the first sorted token. The one value the engine demanded of
this project's own declaration (`6FA3FCCF`, from
`materials/mods/snoopymod/ui_default_base:SINGLE:PLATFORM_WIN32:RENDERER_D3D12`)
fits this rule exactly.

**The shipped 36 queries are not in the key space (2026-10-03).** With the key
rule now confirmed 220/220 against the official index, the Darktide side was
re-searched with the *real* vocabulary drawn from the toolchain - 102 context
tokens, 130 define tokens, 69 shader names (`debug_file_index.sjson`), plus the
community tool's imported names and this project's own. Every combination tried
(engine shader names, output-node paths, our material path, `default` /
`gui_render_pass` / empty context, 0-2 sorted defines, both platform tails):
**zero hits** against the 36 queries. As controls, the rule still reproduces the
one value the engine accepted (`6FA3FCCF`, exactly), and hashing the 220 real VT2
keys produces no collision with Darktide's 36.

So the two id spaces are disjoint, and no material/context/define combination
reaches Darktide's shipped query values. That is consistent with the earlier
reading: the 36 are the *shipped shader family's own* declared values (its
compiled declaration), not values derivable from a material's path - while the
value the engine demands of **our** declaration (`6FA3FCCF`) *is* derived, because
it is ours. This is a clean, falsifiable negative rather than an open question.

**`debug_file_index.sjson` confirms the key rule 220/220 (2026-10-03).** The
clean index (`data/xx/<hash> = <name>`) has 264 entries, 220 of them
`.shader_library`. Testing the two readings of the key:

- "all tokens sorted" fits **187/220**;
- **"`<shader>:<one context token>:<the rest, sorted>`" fits 220/220** - removing
  exactly one token leaves a sorted list in every single key, and no key needs
  more than one removal.

The counterexamples are what distinguish them: `gui_gradient:atlas_color_lookup:diffuse_map:masked:anisotropic`
is unsorted as a whole but sorted once `atlas_color_lookup` is taken as the
context. So the rule this project derived from the VT2 compiler's logs -
`<shader>:<context>:<defines, sorted>`, lowercased for the file name - is
confirmed against the official toolchain's own index. The context token mixes
define-like and pass-like names (`diffuse_map`, `flatten`, `shadow_mapping`,
`write_mask`), which is why the two were hard to tell apart in the raw cache.

**What `shader_cache.db` does and does not show (2026-10-03, corrected).** Read
from the bytes (string lengths and the byte gaps between them, no field meanings
assumed), the file is a sequence of `{u32 len}{ascii}` strings interleaved with
binary. Three things are directly evidenced - strings whose gaps are zero, i.e.
contiguous lists:

- a **`.shader_source` path** (`core/stingray_renderer/shader_libraries/lens_flare.shader_source`),
  then a run of **pass names** (`flare_bright_pass`, `flare_merge_pass`,
  `ghosts_bright_pass`) and the **shader name** (`lens_flare`);
- **`.shader_library` names** of the form `<shader>:<token>[:<token>].shader_library`
  where the tokens are lowercase define-like names (`diffuse_map`,
  `yuv_video`, `depth_test_enabled`);
- **condition strings** (`on_renderer(D3D11, D3D12, GNM)`, `!on_renderer(GL)`).

What it does **not** show, and an earlier reading of this note wrongly asserted:
the key's second field is **not** the pass name (98 of 99 such tokens are not in
the file's pass-name list - they are define/render-state tokens), and there is no
clean fixed record stride (the inter-string gaps vary 58..129, so binary fields
sit between records). So the file **confirms the naming shape** of library keys
and supplies the **pass-name and condition vocabularies**, but it is not a key ->
hash table and it does not establish the key's field meanings.
 **The official VT2 SDK binaries name the writer (2026-10-03).** Fatshark's own
Vermintide 2 mod tools ship two dev binaries - the shader/asset compiler
(`sdk/engine/win64/dev/stingray_win64_dev_x64_compiler_1.exe`, 22 MB) and the
engine (`bin/stingray_win64_dev_x64.exe`, 21 MB) - and unlike Darktide they are
*dev* builds, so their export symbols are present. Those symbols name the
structures this project has been decoding blind:

```
shader_compiler::make_device_data          <- writes the device block
shader_compiler::ShaderLibrary::Shader::Pass   <- the pass table's type
shader_compiler::StateBlock / StateVariable
shader_compiler::InputVariable / InstanceData::Variable / StreamOut::Channel
shader_compiler::ShaderProgram / ShaderBytecode
shader_compiler::PlatformD3D12::compile / convert_shader / convert_pass
shader_compiler::`anon'::reflect_cbuffer / validate_cbuffer_reflection
shader_data::update_sort_keys
ShaderManager::add_global_constant_buffer_from_sd / ShaderRef / ShaderTemplateRef
```

So the engine's own vocabulary for our structures is **Pass** (the per-permutation
program pair), **StateBlock/StateVariable** (the descriptor + render state),
**InputVariable** (the channel/interface), and the cbuffer reflection path that
`ShaderManager::add_global_constant_buffer_from_sd` (`_from_sd` = from shader
data) feeds the `global_viewport` table from. Both binaries reference an unshipped
PDB (`stingray_win64_dev_x64.pdb`, guid `86155e7b1c372e4dad70e78a9f65a40b`); the
compiler pins the path to Fatshark's own build machine
(`C:\BitSquidBinaries\vermintide2_mod_tools\engine\win64\dev\...`) and the
engine's copy to the mod-tools CI scratch (`D:\a\w\vt2-modding\refs\heads\...`).
So the symbol *universe* is Fatshark's official one - the tooling that wrote the
format - which makes the type and function names above authoritative rather than
a community guess. Only the field offsets need a symbol source.

**The SDK ships a complete writer run (2026-10-03).** `TEMP/streamable_resources_compile`
holds the output of a full compile the SDK itself performed: the VT2 `data/` tree
with its section files, `debug_file_index.sjson`, **`shader_cache.db`**,
`strings.txt`, `exploded_database.db`, and `modified_files/`. This is the writer's
ground truth - the same kind of artifact set Darktide's own compiler would emit -
and `shader_cache.db` in particular holds the compiler's manifest with literal
strings:

```
shader = "gui"
gui:depth_test_enabled:diffuse_map:one_bit_alpha.shader_library
condition = "on_renderer(D3D11, D3D12, GNM)"
condition = "!on_renderer(GL)"
```

So the library file names (our key format), the conditions, and the per-shader
pass lists are all written out by the toolchain in text. That is the best decode
source found so far: the format's *writer* left its own manifest.

 The
**The community tool's new name lists were imported (2026-10-03).** The
`Darktide-Asset-Compiler` update added `game_shaders.json` (22 shader presets with
their stream paths and variable names), `particle_schema.json` and a refreshed
`shader_names.json`. Every string in them (plus the Blender addon's Python) was
extracted, merged into the corpus and the dictionary (2,468 new entries, group
`asset_compiler`) and the permutation vocabulary (2,598 tokens). It resolved one
more name: the condition root `8FB860CF` = **`write_channels`** (with `gui`,
`gui_hdr`, `gui_render_pass` and `alpha` already known). It did **not** resolve
our group tail's hashes (`26E1CF9F`, `26E24EBC`, `5E415D62`) under either
`murmur32` or `high32(murmur64)`, against 740k names - so the tail's names are
still not in any string set we have.

**The group `tail` is load-bearing even with empty conditions (2026-10-03).**
Zeroing the group tail (a constant 46-byte zero entry, every group pointing at it)
builds but renders **`RENDER_BLACK`** (79.9% black). So despite our conditions
section being 0 bytes, the group tail's hashes (`26E1CF9F` = `gui`, `26E24EBC` =
`gui_hdr`, `5E415D62`) still select the group's source set - it is not vestigial
and cannot be a constant. It stays carried for now, and it belongs with the
conditions tree work (it is the same source-graph data).

**Can we derive the conditions tree? - the exact gap (2026-10-03).** The format,
branches and vocabulary are decoded (`condition_tree`), and the tree's meaning is
visible in the real example (`contracts_top_candles`, 1436 B):

```
rec0 (root): hashes [gui, BDF72706, B5F45768, red, green, blue, 8FB860CF]
             branches test subsets -> result index; fallback 7
rec1..4:     the same, one channel swapped (red / green / blue / alpha)
rec5 (base): hashes [gui, BDF72706, B5F45768] -> one result
```

So a record's hashes are the condition names and each branch is a conjunction of
them returning a small result index. What is **not** established is what that
index selects - the module's own note says "the result values are small indices
(0..7) whose mapping to groups or interfaces is not established", and names the
experiment: a generated family with a crafted tree and an in-game observation of
which group the engine selects. Until that is run, a permuting material's tree
cannot be *generated* (only carried).

For the current goal this is not on the critical path: our material is
**non-permuting** (1 channel, 0 condition bytes), and so are `decal_aoe` and the
other small FX materials - their trees are empty, i.e. a constant. The tree only
becomes real data for materials that switch on the shader's optional inputs
(`contracts_top_candles` 5 ch / 1436 B, `substance_basic_wd` 17 ch / 220 B,
`force_staff_arcs` 7 ch / 24 B), which is what standard and particle materials do.
 **Cross-family validation (2026-10-03, `family_table`).** Reading three examples
of each family confirms the model - each family is a small, consistent table:

| family | examples | programs | masks | seams |
| --- | --- | --- | --- | --- |
| GUI (UI base) | `007bf44b`, `08158dd8`, `0888983b` | 96 | `7 1 1 2 2 4 4 8 8 F F 7 7 ..` | `0 2 0 2 ..` |
| standard | `0679f3f8` (60), `0b0b05ed` (8), `03543251` (7) | 60/8/7 | `7 .. 8` every fifth | mostly 0 |
| particle/FX | `025e1dca`, `0f463291`, `09337f59` | 2 | all `7` | all `0` |

The three **GUI** sections are identical in masks and seams, so the recorded table
is validated. **Standard** and **particle** have their own tables (standard a
`7..8` pattern, particle a uniform `7`/`0`), so each family needs its own small
record - and the `head` column `family_table` prints is probe-relative, so the
real per-family datum is `(body length, program count, seam run, mask run)`.

**The program list is generated and live (2026-10-03).** `derived_programs` now
emits the family's **whole** table (it does not tie programs to queries - the UI
base declares one query yet carries 48 program pairs). With `programs`, `tails`
and `block_heads` removed from the file, the built section is
**byte-identical** to the carried one (`F3EE2651139FDEB0`, 421,508 B) and
**`RENDER_OK`**: the first carried pool to become generated rather than carried.


The resource record's word order, as the tail dumps and the working naming pass
read it, is `{name, index, binding, flag, set, FFFFFFFF-or-size, 0}`:

- word 0 is the **name hash** - the naming pass resolves `3AFC636C` =
  `global_texture2D` and `41B1CFF8` = `global_feedback_buffers` through it;
- word 1 is the **index into the shader's own resource table** - the resource's
  position in its group's descriptor list (measured: the UI base, `e1e0f38f` and
  others match completely; see the section above);
- word 2 is the **binding**, word 4 the **set**.

The binding and set words are confirmed against the containers' reflection
(`dxil-spirv` then `spirv-cross --reflect`): 38ECBAD1's pixel container
reflects `separate_images` set 2 binding 0 (`global_texture2D`), set 0 bindings
0-2 (`linear_depth`, `39A56531`, `D1D67F3B`), `separate_samplers` set 0
bindings 0-2 and set 31 binding 0 (`static_minlod_sampler`), and `ubos`
bindings 0/1 with sizes 1776/384 - matching the tail records field for field.
The engine records (`bones`, `idata`) are buffer records of the same shape, with
the byte size in word 5 where an array has `FFFFFFFF`.

An earlier reading in this note put the index in word 0 and the binding in word
1; that is a miscount, superseded by the measurement. The sampler records do
read `{name, binding, 1, set}`. The `B3A2EB88` cbuffer entry saying 7 where the
resource table's 7 is `linear_depth` stays odd - either the cbuffer entries
index a second table or that entry needs another look.

### The compiled shaders carry the engine's names

The names behind the tail hashes are not only in the executable: the shipped
DXIL containers' global symbol tables carry the shader-source names. Decoding
the containers out of an engine-data file (`container_strings <file> 0 out.bin`)
and mining them with `mine_strings` (now hashing every letter-starting substring
of a printable run, since names sit next to printable bitstream bytes) resolved:
`global_feedback_buffers` (the buffer array), `static_minlod_sampler` (the space
31 sampler), `global_samplers`, `global_texture2D`, `temp_gbuffer1`, `idata`,
`bones`, `noise`, `SV_POSITION` (uppercase - the engine's own semantic name,
where the container's ISG1 says `SV_Position`), `TANGENT_BINORMAL`,
`BLENDWEIGHTS`, and a channel instance name `texture_map_1453a433`. Channel
names are instantiated per material (`<slot>_<hash>`), which is what the
preamble's channel records key on. Cbuffer names themselves are not in the
symbol table, so the samples' unnamed cbuffers (`B3A2EB88`, `E7DFA2E1`) stay
unknown for now. `tail_hashes` collects the distinct hash-like words from a set
of tails for such a mining round. Both the executable and the compiled shaders
change with a game update, so a name mined from one build should be re-checked
after an update before it is trusted.

Update: variables and cbuffers turned out not to be in the block (see the block
notes below), so the block's remaining authority is resources/channels - and its
record stream is now framed: record lengths follow the record's `kind` (4 -> 60
bytes, 5 -> 73 bytes), the engine prologue (`linear_depth`, `global_diffuse_map`,
`sun_shadow_map`, `fog_volume`) is at fixed offsets and the stream ends exactly
at the preamble's end. `shader43 --records` parses it end to end on three shipped
shaders, and the (since removed) `clone_channel` line cloned a record into the
preamble, every tail's block and the group data's variable records
(unit-tested). In game the cloned name **binds**: with the clone and a material
naming it, the title screen renders the mod texture with the cycling tint (a
block-only clone left the title black).

Block notes from a byte-precise dump of the chain base (863 byte preamble):

- The channel records are **byte packed**, not word aligned: the `bca` record
  starts at `+0x283` (`{u32 3, u32 bca_hash, u32 5, u32 1, ...}`) and its hash
  sits at `+0x287`. Record strides differ with content (`bca` -> `orm` is 74
  bytes, `orm` -> `nm` is 73), so records are variable length with the format
  flags from `texture_format_spec.config` in their tails.
- `orm` and `nm` are preceded by a different word than `bca` (`c5 0a 8d bd`
  before the `orm` hash), so the word before the hash is not a stable "kind".
- Channel records appear in the preamble *and* after every pixel program (the
  same block), so a channel rename has to patch every copy. The group data
  carries the same names in its canonical records and in the cbuffer-keyed
  packed form, so a rename must patch all three places. This was implemented as
  the (since removed) `channel <shipped> <new>` line (`replace_hash`): it
  replaced the 4 byte name hash in the group data, the preamble and every tail.
  Verified offline against the UI base (264 occurrences: 216 group, 48 device).
- Section-level evidence from an external binary-patch mod (RainbowFlame),
  diffing a patched shipped material against the original: the programs are
  replaced, the **tails** change only in their per-program cbuffer lists
  (`c_material_exports` 80 -> 112 bytes, one program gains a `c_billboard:144`
  entry), the **group data** grows by 40 bytes (variable records and count
  words) and the **default data** by 44 bytes. The **preamble/block, contexts,
  conditions and dependencies are byte-identical**. Cbuffers and variables are
  therefore not part of the block: their interface lives in the group data's
  name-to-slot tables and the per-program tails, and both sizes and entries can
  change. The block stays the library's - the remaining authority for
  resources/channels. (Our earlier `clone` test that grew the UI base's cbuffer
  rendered black, so something else about that patch was wrong - most likely the
  base material's variable list, which RainbowFlame patches in its materials as
  well.)
- Channel records in the block: the record stream sits at the end of the
  preamble and parses cleanly. A record is `{u32 name_hash, u32 kind,
  u32 count = 1, ...}` and its length is fixed by `kind`: **kind 4 -> 60 bytes,
  kind 5 -> 73 bytes** (kind 2 is `global_texture2D`, which appears in the
  per-pixel blocks only). The stream parses end to end with `shader43
  --records`, exactly reaching the preamble's end (staff-49: 6 records from
  `+0x20F` to `+0x391`; the enemy warpfire material `bb79ba7a5b92d132`: 7
  records from `+0x20F` to `+0x3E7`; the UI base: 1 record at `+0x1F5`). Every
  shader has the same engine prologue - `linear_depth` (kind 4, `+0x20F`),
  `global_diffuse_map` (kind 4, `+0x24B`), `sun_shadow_map` (kind 5, `+0x287`)
  and `fog_volume` (kind 4, `+0x2D0`) - and shader channels follow (the first at
  `+0x30C`). The stream is preceded by its **record count** in the word right
  before the first record (1 for the UI base, 6 for staff-49), and a clone has
  to bump it. In-game channel-clone tests: cloning the block record **and** the
  group data variable record made the game fail with an out-of-memory fatal
  error at boot (the same signature as a malformed device preamble), and fixing
  the count word alone did not help; a **block-only** clone (preamble record +
  count, no group data, no tails) boots normally. The group data clone is
  therefore wrong and was dropped. The per-pixel blocks repeat the stream with
  kind 2 records mixed in, whose length is still unknown, so tail blocks are not
  cloned yet. Inspecting the failing group data shows an unrelated record
  changed (`camera_world` became `world`), i.e. the generic run heuristic
  inserted clones at false-positive record occurrences; cloning a channel's
  group data needs a precise per-table parser instead. In game, the block-only
  clone boots but the title texture stays black: the cloned channel does not
  bind, so the group data records (not the block record) are what the engine
  resolves when a material names a channel.
- The block's **config portion** (the preamble minus the record stream; the whole
  preamble is embedded verbatim at the end of every pixel tail, after the cbuffer
  and resource/signature lists - the 112-byte vertex tails do not carry it) is
  120 bytes of header (group count, table counts/sizes) then byte-packed 13-byte
  records `{u32 index, u8 0, u32 value, u32 0}` and the stream count. The same
  records appear across shaders: a **22-record common prefix** (`12:1 16:1 15:8
  19:8 10:FF 14:1 18:1 1A:1 11:FF 13:1 17:1 5E:F 5F:F 60:F 61:F 62:F 63:F 64:F
  65:F 0C:1 0E:4 0D:1`) then shader-specific records, so the index space is the
  engine's global variable order (16..26 is `time` .. `upscaling_enabled` in the
  UI base) and the values are small per-shader masks/counts (1, 8, `0xF`,
  `0xFF`, `0x60`, `0x78`). Shaders with fewer programs have *more* block
  records (a 4-program shader: 40-58 vs the 96-program UI base's 29), so the
  table is sized by the variable set, not the program count. The 120-byte header
  is otherwise **constant across all seven shaders**; only three words vary:
  `+0x04` = the group count, `+0x08` = the number of cbuffers the shader uses (2
  for the UI base, 3/5 elsewhere), and `+0x0C` = the block's record count + 8
  (verified against the stream offset in all seven). Practical generation: copy
  the template's block and rewrite those three words; the records themselves are
  engine-variable entries and an over-inclusive set is harmless.
- The group data holds a channel in **two framings**, however many records the
  channel has per group unit (a texture channel is three, a scalar one is one;
  108 + 108 = 216 for the UI base's `texture_map`; `shader43 --channel
  <name>` dumps them). **Canonical** 20-byte records
  `{type, flags, name_hash, cbuffer_offset, size}`: type 5/offset 0/size 4,
  type 1/offset 4/size 8, type 1/offset 16/size 8. **Packed** copies sit in a
  run of their own: a `{u32 count = 3}` word, then 28-byte records
  `{name_hash, a, b, c, d, B5639618, 0}` (the `c_per_object` hash near the end),
  e.g. `{hash, 0, 0, 4, 1, B5639618, 0}`. A channel clone therefore needs 3
  canonical + 3 packed insertions per group unit (216 total for the UI base)
  with the run counts bumped. The records are byte-packed (any alignment): the
  UI base's canonical records sit at 4-, 2- and 1-byte-aligned offsets, so the
  scan probes every byte offset and the run/count check rejects the shifted
  views. `clone_channel_group_data` inserts all 216 records on the UI base
  (108 canonical + 108 packed, unit-tested) and is **verified in game**: with
  the block record, the group data clone and the material naming the new
  channel, the title screen renders the mod texture with the cycling tint,
  where the block-only clone stayed black. The flag words are byte-packed, and a record's tail carries the
  same packed 2-bit-per-slot usage counts as the descriptor `Y` field (`0x15` =
  21 = 1+4+16, `0x55` = 85 = 1+4+16+64). A channel can therefore be added by
  cloning a record of the same kind and substituting the name hash, the same
  mechanism as `clone_variable`, now with a known record length. The miner's
  dictionary CSV carries the murmur32 hash in its third column, which is how
  `sun_shadow_map` was resolved.

1. **Group descriptors** (`{name_hash, flags, X, Y}`): `X` is the resource's
   byte offset in the per-draw binding table, allocated in descriptor-list order
   (24 bytes per constant buffer, 8 bytes per other resource) - decoded and
   reproducible. `flags` keeps the space in bits 16+ and a small kind in the low
   bits (0 material cbuffer, 1 engine cbuffer, 3 texture, 5 UAV). Still open:
   what `Y` measures - but its *shape* is now clear: it is a packed array of 16 two-bit fields (values 0..3). Vertex-data resources take exactly `1, 5, 21, 85, 341` = `sum(4^i)` (`bones`, `idata`, `hmap`), which is what "one count in each of 1, 2, 4, 5 slots" looks like, and the UI base's `41B1CFF8` reading 10 = `2 + 8` next to `global_texture2D`'s 5 = `1 + 4` is the same packing with the fields at 2. So a likely reading is "per program/pass, how many times the group binds this resource", saturating at 3 per field; the exact slot meaning is still to confirm. Also open: the compact copies' exact record order.

   The group data's overall structure is mapped now (UI base, 62,884 bytes):
   a 32-byte global header (`{u32 group_count = 36, u32 library_hash,
   u32 0x130, u32 4, u32 c_per_object, 0, 0, 0}`), then **36 groups**, each
   holding its descriptors (3 x 16 bytes, above), the channel table (`{u32 2,
   u32 count = 7}` + 7 canonical 20-byte records), the variable table (`{u32 2,
   u32 count = 69}` + 69 canonical records), the packed copies (`{u32, u32 0,
   u32 count = 3}` + 3 packed 28-byte records) and a byte-packed group header
   (74 bytes in groups 0-11, 57 in 12-34, 29 in the last; it carries the
   permutation's hash, e.g. `9FCFE126` in 12 groups). The channel table, variable
   table and packed run are **byte-identical across all 36 groups**; only the
   descriptors' `Y` and the group header vary (`Y` = `{0, 5, 10}` in groups 0-11,
   `{0, 1, 2}` from group 12 on - the per-permutation binding counts). The
   "compact copies" are the same records re-emitted per group at whatever byte
   alignment the group lands on (the group size, 1758/1741 bytes, is not
   4-divisible), not a different encoding. Generation can therefore emit the
   tables once and replicate them across the template's group count, keeping the
   template's descriptors and headers. The tables' contents are identified: the
   69-record table is the **`global_viewport` engine cbuffer** (hash 516D5CCD,
   1776 bytes) - `camera_*`, `time`, `delta_time`, `frame_number`,
   `taa_enabled`, `gamma`, `viewport` and 40+ unnamed engine internals (28 named
   by the dictionary) - and the 7-record table is the group's **`c_per_object`**
   variables: `texture_map` x3 plus `view_proj`, `world_view_proj`, `world` and
   `dev_wireframe_color`. The packed 28-byte copies are keyed by `c_per_object`.
   The group descriptors are `global_viewport` (kind 1 cbuffer), an unnamed
   texture (kind 3) and an unnamed UAV (kind 5). Shaders differ in how they
   organize the tables: the UI base has two runs (the `c_per_object` variables
   and the `global_viewport` variables), while a 4-program environment shader
   (`38ECBAD13742E4E1`) has a single merged run with engine and material
   variables interleaved by offset (`camera_unprojection` 0, `camera_pos` 16,
   `texture_map_1453a433` 24, `camera_view` 32, `world_view_proj` 48, ...).
2. **Program tails from DXBC**: the tail is the per-program binding map and it
   is load-bearing - zeroing everything but the cbuffer entries crashes the game
   at shader load (`dispatch_loadtime`, `shader #ID[<the group's query id>]`),
   restoring them renders again. Decoded: `{u32 cbuffer_count}` + 24 byte
   cbuffer entries (`{name_hash, size}` at entry `+0`/`+8`, register order), then
   counted lists (empty = one `0` word) carrying 7 word resource records
   (textures, UAVs with kind 3 and `0xFFFFFFFF` bindless + space, samplers) and
   3 word signature runs (`POSITION`/`COLOR`/`TEXCOORD`/`CUSTOM`/... resolved
   hashes; the pixel program's interpolated-input run has no count of its own,
   its length matching the container's `ISG1`), ending with the shared block
   (the preamble's tail, 549 bytes on the UI base). `shader::Tail` parses and
   serialises the cbuffer list and round-trips every sampled tail byte for byte,
   `patch_tails` (since removed) rewrote sizes in the engine data (hygiene: a 256 byte shader cbuffer
   with 240 byte tails still renders, so the size is not load-bearing). The
   shared block is *byte packed*, not word aligned - `texture_map` sits at an odd
   offset (501) inside it with `{u32 size = 4, u32 count = 1}` after it, so
   modelling it needs the packed record stream, not u32 lists. Next step: model
   the resource/signature lists (their counts, kinds and how many words each
   kind uses) and the block, so a tail can be generated from the compiled
   container's reflection instead of the engine data's bytes. Concrete shapes seen in
   the UI base's pixel tail: after the cbuffer entries, counted lists of 7-word
   resource records (empty list = one `0` word) - a texture `{3AFC636C, 2, 0,
   FFFFFFFF, 2, FFFFFFFF, 0}`, a UAV `{41B1CFF8, 3, 0, FFFFFFFF, 1F, FFFFFFFF,
   0}` and a third `{4B42C5E6, 0, 1, 1F, 4, DC0548BC, 0}` whose differing shape
   is the open part - then signature runs of 3-word `{hash, index, ordinal}`
   records (`{96B9600E, 0, 1} {96B9600E, 1, 2} {96B9600E, 2, 3}` with no count
   of their own, matching the container's ISG1) and a counted run
   `{DA560F03, 0, 0}`. **The signature hashes are murmur32 of the semantic
   name** - `POSITION` = `3FFEABD6`, `COLOR` = `FCDCBA12`, `TEXCOORD` =
   `B77A0F36`, `CUSTOM` = `96B9600E`, `SV_POSITION` = `DC0548BC` (verified with
   the `hash` example) - so the runs can be emitted from the DXBC `ISG1`/`OSG1`
   chunks: the vertex tail's counted run is its output signature
   (`POSITION`@0, `COLOR`@1, `TEXCOORD`@2), the pixel tail's count-free run is
   its interpolated inputs (`CUSTOM`0@1, `CUSTOM`1@2, `CUSTOM`2@3). The vertex
   tail is just the cbuffer entry, empty lists and one counted run of three
   `{hash, 0, index}` records. The list sequence is fixed per stage: the UI
   base's pixel tails all show **eight counted resource lists** with counts
   `0,0,0,1,0,1,1,0` (lists 4, 6 and 7 carry the texture, the UAV and a
   vertex-data-like record), then the count-free input run and a counted run;
   the vertex tails have seven empty lists then their counted output run. The
   resource lists are **stage-level**: pixel tails 1/3/5 have byte-identical list
   regions, and the pixel shader's DXBC resources line up with them (list 4 = the
   `Texture2D` array at `t0 space2`, list 6 = the `RWBuffer` at `u0 space31`);
   the vertex shader has one cbuffer and no resources, so its lists are empty.
   Only the cbuffer list (vertex = `c_per_object`; pixel = `global_viewport` +
   `c_per_object`) and the signature runs differ per program. Across the seven
   shaders the lists map to: list 3 = engine textures (`linear_depth` seen),
   list 4 = the shader's texture (`3AFC636C`), list 6 = the shader's UAV
   (`41B1CFF8`), list 7 = the vertex-data record (`4B42C5E6`); lists 1, 2, 5 and
   8 are empty in every shader. One shader's list 7 carries four records, so the
   7-word record shape is not universal - the record size may depend on the
   resource kind. Which lists exist by index and the pixel tail's trailing
   `{DA560F03, 0, 0}` run are still open. **Superseded (2026-10-12):** the list
   indices in this older note are off by one and `linear_depth` is in list 2, not
   list 3; see "The tail's rest is nine counted lists" for the corrected map and
   record shapes. The trailing run is **shader-independent**
   (identical in the UI base and the 4-program shader), and the 4-program
   shader's pixel tail confirms the run shapes: `{SV_POSITION, 0, 0}` then four
   `{CUSTOM, i, i}` records, the counted `{DA560F03, 0, 0}` run and a final `2`.
   The tail's block placement varies per shader: the UI base's pixel tail embeds
   `preamble[12..]` (549 bytes) at +252, while the 4-program shader's block is 840
   bytes and mostly matches the preamble's first 840 bytes (116 differing bytes),
   so the tail's block is not always a straight preamble copy.

   Current understanding, verified in game: **material variables bind by name**
   to the shader library's own variable names (its compiled cbuffer members and
   offsets). A known name fills its slot wherever it sits in the material; a name
   the library does not know stays out of the layout (verified: unknown
   `zz_probe_a` declared first plus known `dev_wireframe_color` second has the
   shader reading the known one's colour, and only-unknown names leave the slot
   at zero; the known name driven from Lua gives the full hue rotation). The
   material's `offset` field is an offset into the material's `variable_data`,
   not a cbuffer offset. So a mod drives the library's known variable values and
   channels and ships its own programs; renaming a variable works, adding one is
   under test.
2b. **Generator plan** - what a generated section is made of:

| Piece | Source |
| --- | --- |
| Header, section offsets | generated |
| Contexts | generated (one `default` query, `0xFFFFFFFF`) or the shader's |
| Conditions | empty for a single group, the shader's for permutations |
| Dependencies (8 bytes) | the shader's |
| Group data: units, descriptors | generated (`X` = running 24/8 byte allocation, `flags` = space/kind) |
| Group data: canonical tables | **ours**, from the material's channels and variables - this is the upload layout |
| Group data: packed copies | the **library's** (required to parse, not used for uploads) |
| Device: programs | ours (built from our HLSL) |
| Device: tails | cbuffer list ours, resource lists the library's |
| Device: preamble/block | the **library's** compiled interface |
| Default data | ours (empty, or the material's defaults) |

   The library constants (block, packed copies, resource lists, engine cbuffer
   variable names) are a small per-shader file; everything else the tool can
   write. New *channel names* still require the library's block to already list
   them, since the block is the library's own record set.

2c. **Mod-defined parameters: what works** - the library's known variable names,
   driven from Lua or the material (`dev_wireframe_color` on the UI base, a
   float4 at offset 224, is the one custom slot its tables expose), plus its
   texture channels and our own programs. Values can be packed four floats at a
   time into a known vector4. Renaming a parameter works (verified in game:
   `dev_wireframe_color` -> `mod_tint`); adding one is under test (a clone at
   offset 240 is deployed). The set of offsets is still the library's.
3. **Conditions payload**: decode the u16 list per node (structure, names and
   node bounds are known). A material that does not permute anything needs no
   conditions at all: the minimal two program material ships an empty conditions
   section and a single `default` context pair `{query_id, 0xFFFFFFFF}`, with
   the group header carrying that same query id. So this only has to be decoded
   to support several groups/permutations in one material. Measured on the UI
   base: 35 records of `{u16 tag=1, u16 payload_words, u16 payload_offset, u16
   count}` + `count` hashes + `payload_words` payload words. The framing is
   self-delimiting (`payload_offset = 8 + 4 x count`), read and written byte for
   byte by `filetype::condition_tree`, and the record starts are exactly the 29
   + 6 conditions offsets of the two contexts. The payload is a u16 bytecode
   (`0x20xx` test, `0x10xx` jump, `0x70xx` count, `0x90xx` end) that is mapped,
   not decoded. The condition hashes are material-side names (`gui` = 9FCFE126,
   `red` = 9B8DE7E4, `green` = 4BA4BD58, `blue` = 0977913D, `alpha` = 3F697354),
   so the tree is derivable from a
   declaration that names them (see the Stingray format in
   `Shader Section Generation Notes.md`). The material side declares `channels`
   (its texture channels), `textures`, `variables` and `material_contexts` (a
   key = value map, e.g. `surface_material = "bone"`; the shipped base material
   uses `"dirt"`), and the base material ships an empty conditions section, so
   the tree is only needed for permuted shaders. The condition hashes are *not*
   in the group's variable tables, so they are neither cbuffer variables nor the
   shader's channels; and they are not `material_contexts` values either (the
   shipped corpus only uses `surface_material = bone/metal_solid/dirt/...`, none
   of which match). They do read as the shader's **optional input names** - the
   Stingray `type = { vector3: ["HAS_BASE_COLOR"] }` pattern, where a material
   provides a subset and the tree maps that subset to a group - which also
   explains why the base and 4-program shaders ship an empty conditions section.
   Still to confirm by permuting one material's declared inputs and watching the
   conditions change.
4. **Device preamble**: split the engine-constant middle from the per-material
   suffix (two same-shader materials differ by one list entry). Lead: the
   preamble's first words track the material's contexts - `{1, query_count, 2, …}`
   reads `{1, 1, 2, 0, …}` on the minimal one context material and `{1, 36, 2,
   37, 30, …}` on the UI base (36 = 30 + 6 queries), and the tail of the
   preamble holds the material's variable list (`texture_map` sits at `+0x1F4`).
   Next step: dump the preamble of a few hundred varied materials next to their
   contexts/conditions/programs and fit the table, then generate it.
5. **Mod-side shader declaration**: `filetype::shader_node` reads the
   toolchain's own `.shader_node` (entry points, channels, variables/defaults,
   permutation sets and contexts), so a declaration is already a file next to
   the material rather than a dialect of ours. What is left is the generated
   group data and conditions tree, wiring `Section::build` into `dtmt build`,
   and a new shader generated end to end and verified in game.

## Bounties

`unknown.txt` (hashes seen in Darktide shader structures that the dictionary
cannot name) is generated by the miner. Caveat: 32-bit short hashes collide
(about 5 of ~2100 resolved names are wrong - VT2 leftovers like
`dwarf_cave_rock_*`, `weave_death_ground_264`, `pes_*`). The fix if needed:
verify a name against Darktide-only sources (a local dump of the game's decompiled Lua/scripts +
the game bundle) before trusting it.

## Toolbox

- `shader43` example: `--section <name>`, `--preamble`, `--tail <n>`,
  `--slots --hlsl <dir>`, `--variables <dict>`, `--decompile <dir>`,
  `--rebuild <dir>`.
- `compile_unit` example: `<unit> <bsi> <out payload>` compiles an authoring
  pair into the runtime payload (the same code the build uses).
- `decompile_unit` example: `<payload> <out dir>` writes the `.unit`/`.bsi`
  pair back out; static units only (skins, animations and actors are rejected
  with a clear error).
- `generate_shader` example: `--engine-data <out.txt> <material data file>`,
  `--generate <engine_data> <base.material> <out.material> --vs/--ps`.
- `mine_materials` example: `--dict <csv> --out <dir> <game data dir>`.
- Helper scripts (cargo/rustfmt wrappers, capture and analysis scripts for the
  desktop, e.g. title-screen captures and condition/descriptor dumps).
- Game data: the install's `bundle/data` directory.
- Miner output: `materials.csv`, `variables.csv`, `groups.csv`, `defaults.csv`,
  `contexts.csv`, `conditions.csv`, `tails.csv`, `known.txt`, `unknown.txt`.
