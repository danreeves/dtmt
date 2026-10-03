# Shader RE TODO

Working notes and next steps for the `shader43` reverse engineering. See
`File Type - Material.-.md` (field-level findings) and
`Shader Section Generation Notes.md` (the target and what each section needs).

## Priority order

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

| list | record | content in the samples |
|---|---|---|
| 2 | 7 words | engine records: render-set textures (`linear_depth`, `temp_gbuffer1`) and engine cbuffer variables (`bones`, `idata`) |
| 3 | 7 words | the bindless texture array (`global_texture2D`) |
| 4 | 7 words | no record in any sample |
| 5 | 7 words | the bindless buffer array (`global_feedback_buffers`) |
| 6 | 4 words | static samplers (`static_minlod_sampler`, space 31) |
| 7 | 3 words | the stage's inputs, system values included (`{murmur32(name), semantic index, register}`) |
| 8 | 3 words | the bindless sampler array (`global_samplers`) |

Lists 0 and 1 are empty in every sample, so their record size is unknown. The
observed field layouts are `{name, index, binding, flag, set, FFFFFFFF, 0}` for
resources (`flag` is `1` for a single texture, `FFFFFFFF` for an array),
`{name, index, binding, flag, set, size, 0}` for buffer records (`bones`,
`idata`), `{name, binding, 1, set}` for samplers and
`{name, semantic index, register}` for inputs. The binding and set fields are
confirmed against the compiled containers' reflection (see below).

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

Cbuffer entries are `{murmur32(name), ?, size, register, 1, 0}`: the size at
`+8` and the register at `+12` are confirmed across five families; the word at
`+4` is small (0/1/2/7) and still unread.

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
group framing constants (`between`, `mid`) and the device preamble.

**Left in the file, in size order:**

1. **`programs` (96 entries)** - the program list. Its shape is fully regular:
   every Vertex program shares the minimal 12-byte block (`tail` 0), and the
   Pixel programs walk a small set. `programs = passes x contexts` fits (48 x 2),
   so the list is derivable from the declaration's `(context, permutation)` jobs
   once the pass-program ordering is pinned.
2. **`tails` (20 blocks)** - a tail block is `head (<=64 B) + the preamble body
   (549 B) + per-program patches`. The blocks here are `head 0/1/2` plus **one
   patch at body offset 477**, whose values are `1, 2, 4, 8, 15` - a **per-program
   channel bitmask** (offset 477 is the preamble's field that reads `7`), the same
   `01/02/04/08/0F` set the notes record. So a tail is a head plus a mask patch;
   the head choice and the mask are the two things left.
3. **`block_heads` (3 entries)** - the `02`-prefixed 28-byte heads the tails draw
   from; they fall away with (2).
4. **`group_template`'s group list and `prefix`** - one group per query (36 for
   the shipped family; 1 for a from-scratch declaration), plus the count word.
5. **the group `tail` pool** - the source references; still the open decode.

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
   `{DA560F03, 0, 0}` run are still open. The trailing run is **shader-independent**
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
