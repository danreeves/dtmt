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

**None.** No reader or writer exists yet. A first step is the container
(effect header, system header, the curve/gradient blocks) and a raw-component
view; the component registry is the bulk of the work. The community
`particles_resource.{h,cpp}` decodes and encodes every shipped effect exactly and
publishes the component schema, so it is the reference to port.

## Reference

`C:\dev\Darktide-Asset-Compiler` (`github.com/fviuff/Darktide-Asset-Compiler`),
`src/stingray/particles/particles_resource.{h,cpp}`; see `TODO.md`.
