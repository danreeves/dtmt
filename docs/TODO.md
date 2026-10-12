# TODO

The project-wide worklist: what every Stingray file type still needs, what the
current focus is, and where community work has already solved something.

Two focused lists sit under it:

- [`File-Type-Status.md`](File-Type-Status) - the per-type decode / compile /
  decompile status table.
- [`Shader RE TODO.md`](Shader+RE+TODO) - the material-shader (`shader43`)
  workstream, which is the current focus.

This page is the umbrella. Update it when a file type moves, or when a
community project is found that already knows part of a format.

**Legend** (the same as `File-Type-Status.md`):
*Complete* - the format is fully implemented/documented; *Partial* - enough is
known for usable results, gaps remain; *Limited* - some work begun; *None* -
nothing known.

## The shader goal (top priority)

**Every shader should compile from its own `.material` + `.shader_node` +
`.shader_source`, with no carried `shader_engine_data` side file.** Per family,
as of 2026-10-12:

- **`ui_default_base` (GUI / title): done.** No engine data - it builds from the
  declaration alone (section `CD9854D69AC9A5B7`), verified in game.
- **`standard_base`: the `mid` now derives.** `standard_base.engine_data` is now
  **32,642 B** (down from 375,410, and from 42,091). Its programs' cbuffer and
  resource lists already derived; now the whole group **`mid`** does too - the
  file carries each group's **channel list** (a handful of hashes) and the build
  assembles the `mid` from it, the head's cbuffers, the engine interface and the
  packed copies, with the section **byte-identical**. Still carried: the group
  template's heads/`between`/engine/materials, the
  **contexts/conditions/preamble**, the **program blocks** (~23 KB) and the
  **program list**. The remaining decode is the **program block grammar**; the
  heads and `between` are decodable but need a lossless reconstruction.
- **Particle shaders: not started.** The `particles` file type decodes the
  particle **effect resource** (`filetype/particles.rs`), *not* a shader; no
  particle `.shader_node`/`.shader_source` work exists.

Also outstanding for **any** custom shader: our own **interface** (our own
cbuffers, resources and channels) is still blocked on the device block's record
grammar - a custom base material can only use a shipped interface today.

## Where things stand

- **Compile + decompile work today:** Bundle, Bundle Database, Package, Lua,
  Strings, Texture (partial), Material (partial), Unit (partial).
- **Active workstream (top priority):** the material **shader** - deleting
  `.engine_data` and deriving the `shader43` section from `.material` +
  `.shader_node` + `.shader_source`. See "The shader goal" above and
  `Shader RE TODO.md`.
- **Formats with real work left:** the shader section, Particles, Level, Sound,
  and the resource side a Unit pulls in (Bones, Animation, State Machine, Flow,
  Physics).
- Anything not listed is unknown.

## File types

| File type | In dtmt | Next step |
|-----------|---------|-----------|
| Bundle | Partial | A few header fields and their interaction with external `data/` files is unknown. |
| Bundle Database | Partial | See [`File Type - Bundle Database.-.md`](File+Type+-+Bundle+Database.-). |
| Package | Complete | Nothing. The community writer's `package_v43` corroborates the layout. |
| Lua | Complete | Nothing. |
| Texture | Partial | Side-file names and the streamed name-slot length are not reproduced exactly; a few `flags` bits and the full `category` set are unverified. Cross-check the community writer's texture profiles. |
| Material | Partial | Base + instance round-trip byte-for-byte. Custom shaders work **against a shipped interface**; adding channels / our own cbuffers is blocked on the device block's record grammar. The community `material_v61` writer is a v61 reference. |
| Shader section (`shader43`) | Partial (generating) | The `standard_base` family now derives from source: its engine data carries no resource lists (per-program blocks only) and the built section is byte-identical, verified in game. Remaining: the per-program device **block** grammar (still carried), the group template (blocked on the descriptor **`Y`** usage field), and the from-declaration family (the enumerator). See `Shader RE TODO.md`. The community `shader_section` (version `0x2b`) decodes/encodes shipped sections exactly, and its `shader_compiler` gives the root-parameter numbering. |
| Strings | Partial | The format is done; the bundle-property / variant interaction is not (upstream issue #3). |
| Unit | Partial | Static payloads round-trip and spawn; skins, animations, streamed meshes and actor/camera/light units are rejected. The community `unit_v115` writer is a reference. |
| Bones | Partial | The cooked body and 38-byte envelope are decoded (`filetype/bones.rs`: joint names, their 32-bit ids and the LOD counts), and `dtmt build` compiles a `.bones` SJSON back to a bundle file; three tests round-trip the body and the SJSON. A shipped-sample check is pending. The community `bones_writer` builds and validates a cooked BONES. |
| Animation | None | The community `animation_writer` writes cooked clips (with a fitter). |
| State Machine | None | The community `state_machine_writer` writes states, blends, layers and ragdoll states. |
| Flow | None | The community `flow_resource` decodes/encodes a unit's compiled flow graph. |
| Physics | None | The community compiler cooks PhysX collections/scenes. |
| Particles | Limited | The effect header, the effect variables and each system header (name, capacity, channel layout, component counts) are decoded and compiled (`filetype/particles.rs`); a shipped effect round-trips byte-for-byte. The component bodies are still opaque hex; the community `particles_resource` (version 102) decodes them. |
| Level | None | Nothing started. |
| Sound (Wwise) | Limited | Bank / Event / Stream: the version and the post-processing. VT2's pipeline is the reference. |

## Community references (re-check these)

These move on their own schedule, so revisit them when a format's decode is
picked up - someone may already have cracked it.

### VT2 SDK (the writer's ground truth)

`C:\Program Files (x86)\Steam\steamapps\common\Vermintide 2 SDK` - the official
Vermintide 2 SDK. Under `TEMP/streamable_resources_compile/` it ships the
toolchain's **own** compiler output: 250 compiled `.shader_library` files (the
VT2 device data), `debug_file_index.sjson` naming every one, and
`shader_cache.db`. Its dev binaries carry RTTI/assert strings that name the
writer's types and functions (`stingray::shader_compiler::make_device_data`,
`stingray::D3DShaderPassData` and its `ResourceBindInfo`/`ConstantBufferBindInfo`
/`SamplerBindInfo`, `D3DShaderPass::initialize_shader_program`, ...), and the
source file names (`platform_d3d12.cpp`, `platform_interface.cpp`,
`library_compiler.cpp`). It is the closest thing to a spec for the `shader43`
writer. See `Shader RE TODO.md`, "The VT2 SDK is the writer's ground truth".

### Darktide-Asset-Compiler

`C:\dev\Darktide-Asset-Compiler` - <https://github.com/fviuff/Darktide-Asset-Compiler>

A Blender addon + C++ compiler that authors native Darktide resources (unit,
bones, animations, materials, textures, particles, physics, state machine,
flow). It is a **writer** for the same formats dtmt reads, and it is updated
often (commits through 2026-10; check `git -C <dir> log --oneline`). The parts
worth reading, under `src/stingray/`:

- `material/shader_section.{h,cpp}` - the material stream's shader section
  (version `0x2b`): the compiled shaders of every context and pass, with
  `decode()`/`encode()` exact inverses for shipped sections. Names are
  `IdString32`; a stage is Kraken-compressed DXBC plus the bindings the engine
  builds the root signature from.
- `material/shader_compiler.{h,cpp}` - compiles HLSL with dxc (SM 6.0 DXIL,
  validator 1.7) and builds the **root table**: the order a group's distinct
  constant-buffer / resource names are first bound across its passes and stages.
  That is the numbering our tail records index. It also documents the engine's
  resource-name rule (strip a DXC `__tex_` / `__samp_` prefix) and the per-kind
  tail record shapes (SRV / UAV / structured / raw / sampler / inputs).
- `material/material_v61.{h,cpp}` - a material version 61 writer (`payload
  offset 28`, absent-section `0xffffffff`, header 68, `surface_material` context
  `0x0254A04B`).
- `particles/particles_resource.{h,cpp}` - the particle effect resource
  (version 102) decode/encode plus a component schema (initializers,
  simulators, vertex writers, visualizers, curves/gradients).
- `unit/unit_v115.{h,cpp}`, `bones/bones_writer.*`, `animation/animation_writer.*`,
  `state_machine/state_machine_writer.*`, `flow/flow_resource.*`,
  `physics/*`, `texture/texture_writer.*`, `package/package_v43.*`.
- `murmur_hash.*` and `cooked_resource.*` - the hashing and the cooked-resource
  envelope.

Its `README.md` has an **"On the list"** section (custom shaders, chain-bone
physics, a second UV map, more particle preview) that marks what the community
has *not* cracked yet - a useful "still open" signal.

### Other references

- The VT2 SDK writer artifacts and the `.shader_library` decode - see
  `Shader RE TODO.md` ("The VT2 SDK is the writer's ground truth").
- Upstream issue tracker: <https://git.sclu1034.dev/bitsquid_dt/dtmt/issues>.
