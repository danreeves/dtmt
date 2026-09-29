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
   lengths`, and there is no link table. The carried list is: the opaque and
   default-data header words, each context's second word (0 on every section
   measured), the conditions blob, the slack between group data and programs, the
   programs, and the bytes after them. The substitution test covers the half a
   round trip cannot: a renamed context is 4 bytes at +48, a renamed variable
   4 bytes inside the group data, and a query added moves every later offset by
   the sum with the group data as written. See `Shader Section Generation
   Notes.md`.
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

Across 400 sampled sections (`stream_probe`), 144 have their whole stream
explained by the group data's own record hashes - the material's channels - and
the rest differ only by 48 recurring names. Those resolve to the engine's
render-set textures and the shader's own channels: `fog_volume` (194 sections),
`global_diffuse_map` (160), `linear_depth` (129), `sun_shadow_map` (101),
`brdf_lut`, `global_specular_map`, `static_sun_shadow_map`,
`local_lights_shadow_atlas`, `cached_local_lights_shadow_atlas`, the unresolved
`B70645F7` (70) and a ten-name set at 45; plus rarer declaration channels
(`gbuffer1`, `diffuse_map`, `skydome_map`, `source`, `normal_map`,
`render_target`, `curve_map`, `material_map`). So a generated preamble's stream
is the material's channel list - every name of which also appears in the group
data's tables - plus the shader's own resource usage, which the compiled
container reflects (the earlier notes have the engine records first:
`linear_depth`, `global_diffuse_map`, `sun_shadow_map`, `fog_volume`).

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
(69), the program/tail count (70), the stream count (35), the distinct block
count (91), the distinct interface count (155). Its distribution is 1 x49,
2 x190, 3 x4, 4 x38, 5 x101, 6 x14, 7 x4, and it is a per-shader property (the
two materials of one shader share it). Partial correlations: w2 = 4 on all 38
sections with two contexts and w2 = 5 on 100 of 101 with three contexts, so w2
tracks the context count for the higher values but not for 1-3. `w2_probe`
prints `w2` next to the counts so a new hypothesis can be tested the same way.

Across 489 rich preambles there are only six distinct config index sets: a base
of 1, 3, 5, 8, 12..26, 28, 29, 30 and 94..101 (389 of them), the same without
38, 46 and 54 (86), and four variants adding rows strided by eight -
31/39/47/55, 32/40/48/56, 4, and 38/46/54. So the index space is a grid whose
columns run eight apart (30, 38, 46, 54, ...) and a shader emits the rows it
uses: a generator can start from the base plus the channel row 94..101 and the
rows its own passes need.
### The records' binding fields, and the first word as a group-data index

Decompiling a shipped container (`dxil-spirv` then `spirv-cross --reflect`)
settles the resource records' fields: the second word is the **binding** and the
fourth the **set**. 38ECBAD1's pixel container reflects `separate_images` set 2
binding 0 (global_texture2D), set 0 bindings 0-2 (linear_depth, 39A56531,
D1D67F3B); `separate_samplers` set 0 bindings 0-2 and set 31 binding 0
(static_minlod_sampler); and `ubos` bindings 0/1 with sizes 1776/384 - matching
the tail's records field for field, with the sampler records reading
`{name, binding, 1, set}`. The engine records (bones, idata) are buffer records
of the same shape: 004F18EA's vertex container reflects `usamplerBuffer`s at set
9 and 12, and the records say set 9/12 with the buffer size (16/64 bytes) in the
fifth word where a texture has `FFFFFFFF`.

The first word is an index into the group data's record table. 38ECBAD1's table
reads global_viewport, c_per_object, global_texture2D, linear_depth, 39A56531,
D1D67F3B, global_feedback_buffers - indices 0..6, exactly its tails' first
words; the UI base's reads c_per_object, global_viewport, global_texture2D,
global_feedback_buffers - 0..3, exactly its tails' (which is why its two
cbuffers are numbered the other way round from 38ECBAD1's). 004F18EA's resources
match too (6, 7, 8), but its `B3A2EB88` cbuffer entry says 7 where the table's
index 7 is `linear_depth`, so either the cbuffers index a second table or that
entry needs another look. So both the cbuffer entry's `+4` word and the resource
records' first word are derived from the group data the build generates.

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
