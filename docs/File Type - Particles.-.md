# File Type - Particles

A `particles` resource is a compiled **particle effect** (the game's FX), a
standalone resource under `content/fx/particles/...`. dtmt has not started it;
the type hash is known (`0xA8193123526FAD64`) and the community compiler has a
full codec to work from.

Status of each statement:

| Tag | Meaning |
|-----|---------|
| **Verified** | Confirmed against a decoder and the community codec. |
| **Observed** | Seen in a file, but a wider sample could add cases. |
| **Guess** | Our inference; not verified. |

## Format (from the community compiler)

The format is **version 102**: an effect header, the effect's settings and
variables, then one or more **systems**. Each system has its own header and a
list of components, named after the Stingray particle editor:

- **channels** — the numbers every particle carries (position, velocity, age,
  life, size, ...); 4 bytes is one number, 16 a vector, up to 16 channels.
- **initializers** — run when a particle is born (`position_sphere`,
  `velocity_cone`, `random_float`, ...).
- **simulators** — run every frame (`rate_emitter`, `age`, `velocity_accelerate`,
  `position_integrate`, ...).
- **visualizers** — draw the particles (a billboard with a material and vertex
  writers, mesh/light/ribbon/gpu).

Fixed-size sub-structures (from the community codec):

| structure | size | layout |
|-----------|------|--------|
| effect header | `0x2C` | effect settings + variables |
| system header | `0x240` | one system's header |
| curve | `0x54` | `f32 x[10]`, `f32 y[10]`, `u32 n`; unused `x = 10000 + 10n`, unused `y` = the last `y` (all zero when `n = 0`) |
| gradient | `0xA4` | `f32 x[10]`, `f32 rgb[10][3]`, `u32 n`; padded like a curve |

The components and their fields are a registry the community compiler publishes
as a **schema** (field kinds: channel, vertex, uint, int, float, byte, bool,
curve, gradient, vector3, name, resource, bytes, and the lists `bursts`,
`control_points`, `points`). Resources an effect names by id (materials its
visualizers draw with, units mesh particles spawn) are hashed with the same
`IdString64` the engine uses. Status: **Verified in code** (against
`stingray/particles/particles_resource.cpp`).

## dtmt status

**Limited.** `filetype/particles.rs` reads and writes the **effect header**
(lifetime, culling distance and flags), the effect **variables** (name hash +
three floats), each **system header** (name, capacity, `min_capacity_scaling`,
the channel sizes/stride, and the initializer / simulator / visualizer counts),
the **initializer list** (each initializer's op name and raw operand bytes; the
community's 25 ops, the three variable-length ones sized from their operands) and
the **simulator list** (each simulator's op name and its data-block offset; the
community's 50 ops) and the **visualizer list** (each visualizer's type -
`billboard`, `light`, `mesh`, `ribbon`, `gpu` - and its record bytes). `dtmt
build` / `bundle inject` / `bundle extract --decompile` are wired and three
round-trip tests cover the container. Still opaque: the initializer **operand
fields** (the per-op layouts), the **simulator data** block and the
**visualizer** record bodies; the community `particles_resource.{h,cpp}` decodes
those and is the reference to port next.

**Verified on shipped effects** (2026-10-12): six of the game's effects - from
`lasgun_beam_yellow` (1,848 B) to `fire_grenade_player_initial_blast` (22,862 B)
and `expeditions_explosion_trap` (22,838 B) - decompile (initializer types
`random_float` x160, `zero` x37, `position_sphere` x28; simulator types
`age_age` x35, `burst_emitter` x30, `position_integrate` x19; visualizer types
`billboard` x28, `light` x4, `mesh` x3, `gpu` x2) and **recompile
byte-for-byte** (each re-injected body hashes identical to the original), so the
whole container walk matches the game's own resources.

## Reference

`C:\dev\Darktide-Asset-Compiler` (`github.com/fviuff/Darktide-Asset-Compiler`),
`src/stingray/particles/particles_resource.{h,cpp}`; see `TODO.md`.
