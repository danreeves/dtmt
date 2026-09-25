# File Type - Unit

Status: **Partial.** DTMT compiles a `.unit` (SJSON) plus a `.bsi` (SJSON
geometry) into the runtime unit payload, and a static single-mesh unit compiles,
deploys and spawns in game (`World.spawn_unit_ex`). Static units also decompile
back into a `.unit`/`.bsi` pair that recompiles to the same payload (four
shipped payloads round-trip at identical size). Skins, animations, streamed
meshes and actor/camera/light units are rejected with clear errors.

References used: the Bitsquid/Stingray `unit` structures as parsed by the
Bitsquid Blender Tools (`stingray/unit/dt.py` and friends), the VT2 SDK example
units (`example_mods/endurance_badges/...`) for the authoring schema, and the
game's own inline props (`content/environment/.../chain_8m_01`) as the working
compiled shape.

## Authoring files

A unit is authored as two SJSON files with the same base name:

- `<name>.unit`: `materials` (slot name -> material path), `renderables`
  (name -> flags) and optionally `lod` (steps of `renderables` with a
  `visible_height_range`). `editor_metadata` and `lights` are ignored by DTMT.
  A decompiled unit also carries the opaque payload sections it read back as hex
  strings - `dynamic_data`, `flow`, `flow_dynamic`, `physics` and `trailer` -
  which the compiler writes back verbatim; mod-authored units omit them and get
  the compiler's defaults (the `ffffffff 00000000` dynamic sentinel and empty
  sections).
- `<name>.bsi`: `geometries` (each with `indices`, `streams`, `materials`),
  `nodes` (a hierarchy with `local` 4x4 matrices and `geometries` lists) and
  optionally `animations`. A `.bsi` may be wrapped in the `bsiz` container
  (4-byte magic, 4-byte length, then zlib data); DTMT accepts both.

Geometry streams are authored in conventional formats (`CT_FLOAT3` positions and
normals, `CT_FLOAT2` texcoords, `CT_FLOAT4` colors). The compiler packs them the
way Darktide stores them:

| Channel | Compiled form |
| --- | --- |
| POSITION | half4 (`w = 1`), stride 8 |
| NORMAL | octahedral half2, stride 4 |
| TEXCOORD | half2, stride 4 |
| COLOR | half4, stride 8 |
| BLENDINDICES | ubyte4, stride 4 |
| BLENDWEIGHTS | half4, stride 8 |

`TANGENT`/`BINORMAL` streams are skipped: the compiled Darktide vertex
declaration has no such component and shipped units do not carry them either.

The BSI indexes each attribute independently - `indices.streams[i]` indexes
`streams[i]`'s own vertex array - while the compiled geometry uses one vertex per
unique attribute tuple and a single index list. When a BSI has several index
lists the compiler gathers the streams: it walks the corner lists together,
emits a vertex for every distinct tuple of per-stream indices and writes the
resulting unified index list. A BSI with a single index list (the common case
for hand-written sources, and what the decompiler emits) already has one vertex
per index, so the streams pass through unchanged: a decompiled geometry keeps
its vertex order and any unreferenced vertices. The VT2 SDK example
`endurance_badges/units/props/endurance_badges/prop_endurance_badge_01`
(five independently indexed streams, `bsiz`-wrapped, with `editor_metadata`,
`lights` and `animations`) compiles through this path; the tool
`examples/compile_unit.rs` turns a `.unit`/`.bsi` pair into a payload file
directly.

The SDK's metadata sections (`editor_metadata`, `lights`, `animations`,
`source_path`) are declared in the compiler's schema even though they are not
compiled. That is deliberate: fields the schema does not know are skipped
generically, and the SJSON tokenizer cannot skip a float (it tokenizes
`4.579212` as `4` plus leftovers and then fails), so any unknown section with
float values would break the parse. New metadata fields therefore have to be
declared to keep the reader working.

Each compiled stream carries its channel as `{component, type, set, stream,
is_instance}`; the channel type codes are the Darktide ones (2 = float3,
15 = half2, 17 = half4, 19 = ubyte4).

## Compiled payload

The bundle stores the payload inline (no resource wrapper). The 38-byte
resource header limn writes on extraction (type hash, name hash, lengths and the
optional stream file name) is reconstructed by the tools; the game stores the
same information in the bundle entry, so a mod only ships the payload.

Payload layout (little-endian, version word `0x73` first):

1. `mesh_geometries`: version `1`, streams (each a length-prefixed byte array +
   `{validity, stream_type, vertices, stride}`), channels, index stream
   (`{validity, stream_type, format, count, byte array}`), batch ranges
   (`{material_index, start, size, bone_set}`, start/size in triangles),
   bounding volume (lower, upper, 4 unknown floats), the geometry's material
   slot ids (murmur32) and an unknown word.
2. `skins`, `simple_animation`, `simple_animation_groups`.
3. `scene_graph`: node count, per node a 3x3 rotation, position and scale, then
   the world matrices, then `{parent_type, parent_index}` per node, then the
   node names (murmur32).
4. `meshes` (`MeshObjectDT`): name (murmur32 of the renderable), node index,
   1-based geometry index, skin index, three flag words, bounding volume and an
   unknown word.
5. Empty sections for actors, cameras, lights, terrains, joints and movers.
6. `animation_state_machine`, `dynamic_data` (inline units use the sentinel
   `ffffffff 00000000`; a decompiled unit writes its own blob), visibility
   groups, flow data, the 4-byte triangle finder (`00000000`), physics data.
7. `default_material_resource` and the `materials` list of
   `{murmur32(slot), murmur64(material path)}` pairs.
8. Trailing sections and the skeleton name: the standard zero words, or the
   exact bytes a decompiled payload carried (`trailer`).

The working inline prop (`chain_8m_01`) has **no LOD objects**: its single mesh
is attached to the scene graph directly. LOD objects are only needed for real
LOD sets and reference streamed mesh data, so DTMT omits them for inline units.

## Names

Resource names are murmur64 of the path without extension
(`units/mods/snoopymod/cube` -> `951F1DFAA69816DB`), the same scheme as every
other resource. Unit material slots are murmur32 of the slot name
(`m_cube` -> `44F4A503`); the slot must appear both in the geometry's material
list and in the unit's `materials` map.

## Tooling

- `lib/sdk/src/filetype/unit.rs`: SJSON/BSI parsing (with a normalizer that
  accepts both the SDK's space-separated values and the parser's
  comma/newline-separated form), per-stream index unification, packing and
  payload writing. Unit tests cover the cube case, the normalizer and the
  gathering of independently indexed streams.
- `lib/sdk/examples/compile_unit.rs`: compile one `.unit`/`.bsi` pair into a
  payload file, for testing the compiler outside a mod build.
- `lib/sdk/examples/decompile_unit.rs`: decompile a compiled payload into a
  `.unit`/`.bsi` pair (static units only; unsupported payloads fail with a
  reason).
- `lib/sdk/examples/unit_roundtrip.rs`: decompile a payload, compile the pair
  back and decompile again; the corpus sweep reports 4 static payloads round
  tripping at identical size and 99 unsupported ones skipped with a reason.
- `crates/dtmt/src/cmd/build.rs`: package entries of type `unit` are compiled
  from the `.unit` file and the sibling `.bsi`.
- The snoopymod (`units/mods/snoopymod/cube.unit|bsi`) is the test case; in game
  the Lua hotkey F6 spawns `units/mods/snoopymod/cube` near the player.

## Open questions

- Mesh flag words (`0x000C2001, 3, 1` on shipped static props) and the four
  bounding-volume extras are copied, not derived; their exact meaning is
  unknown.
- Skin/animations, streamed meshes and actor/camera/light units are not
  implemented: a payload that uses them is rejected with a clear error (skins,
  simple animations, animation groups, actors, cameras, lights, visibility
  groups, a non-empty device blob, and vertex streams that declare vertices but
  carry no bytes - streamed geometry).
- The decompiler preserves the payload's scene-graph parent chain; sibling order
  is normalized to the compiler's sorted order (BTreeMaps), and the four shipped
  static payloads round-trip at identical size. Round-tripped normals are
  re-encoded (octahedral half2 is lossy), so the `.bsi` text can differ in
  `NORMAL` stream data only.
- Decompiling a shipped static prop (`chain_8m_01`, 26033 bytes) and recompiling
  the emitted pair gives 26033 bytes with the same geometry (4 streams, 972
  indices, one batch) and the same parent chain, scale and material slot.
- The decompilation path was validated end to end with a Python prototype: a
  compiled cube payload was parsed, its streams unpacked and a `.unit`/`.bsi`
  pair emitted, which `compile_unit` recompiled back into a 1277 byte payload
  with the same four streams (24 vertices, strides 8/4/4/8), 36 indices, one
  batch and the same material resource. The only difference is the material slot
  name hash, which the compiled form cannot recover; the Rust decompiler should
  therefore accept `#HEX` names for slots, renderables and nodes so a
  decompiled unit can reproduce them exactly. The compiler also now rejects
  streams with too few components for their channel instead of panicking.
- LOD objects compile from the unit SJSON's `lod` entries (validated: names,
  step ranges and step meshes). The `bounding_volume` and `orientation` names are
  parsed but unused, and a step's `stream_offset` stays 0 because the compiler
  always writes inline vertex data.
