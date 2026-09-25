# File Type - Unit

Status: **Partial.** DTMT compiles a `.unit` (SJSON) plus a `.bsi` (SJSON
geometry) into the runtime unit payload, and a static single-mesh unit compiles,
deploys and spawns in game (`World.spawn_unit_ex`). Skins, animations, LOD
objects and decompilation are not implemented yet.

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
   `ffffffff 00000000`), visibility groups, flow data, the 4-byte triangle
   finder (`00000000`), physics data.
7. `default_material_resource` and the `materials` list of
   `{murmur32(slot), murmur64(material path)}` pairs.
8. Empty trailing sections and the skeleton name.

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
  comma/newline-separated form), packing and payload writing.
- `crates/dtmt/src/cmd/build.rs`: package entries of type `unit` are compiled
  from the `.unit` file and the sibling `.bsi`.
- The snoopymod (`units/mods/snoopymod/cube.unit|bsi`) is the test case; in game
  the Lua hotkey F6 spawns `units/mods/snoopymod/cube` near the player.

## Open questions

- Mesh flag words (`0x000C2001, 3, 1` on shipped static props) and the four
  bounding-volume extras are copied, not derived; their exact meaning is
  unknown.
- Skin/animations, real LOD objects, streamed meshes (external `.stream` data)
  and decompilation (payload -> `.unit`/`.bsi`) are not implemented.
- The `lod` SJSON field is parsed but not compiled into LOD objects yet.
