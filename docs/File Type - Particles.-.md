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

**Verified on shipped effects** (2026-10-12): **25 of 25** sampled `content/fx/
particles/...` resources - from `lasgun_beam_yellow` (1,848 B) to
`fire_grenade_player_initial_blast` (22,862 B) and `expeditions_explosion_trap`
(22,838 B) - decompile **and recompile byte-for-byte** (each re-injected body
hashes identical to the original). Across them the types are initializers
`random_float` x160, `zero` x37, `position_sphere` x28, ...; simulators
`age_age` x35, `burst_emitter` x30, `position_integrate` x19, ...; visualizers
`billboard` x28, `light` x4, `mesh` x3, `gpu` x2 - so the whole container walk
matches the game's own resources.

## Reference

`C:\dev\Darktide-Asset-Compiler` (`github.com/fviuff/Darktide-Asset-Compiler`),
`src/stingray/particles/particles_resource.{h,cpp}`; see `TODO.md`.

## Decoding the component bodies

What is still opaque is the *inside* of a component: an initializer's operand
bytes, a simulator's data block and a visualizer's record are all kept as hex.
The containers already walk and round-trip, so this is the next layer.

The reference is **declarative data, not code**, which is what makes it portable:
`particles_resource.cpp` holds

```cpp
struct OpSpec { const char* name; Layout layout; std::uint32_t state = 0; };
initializer_specs()   // 25 ops
simulator_specs()     // ~30 ops
```

where a `Layout` is a list of named fields over a small vocabulary - `CH` (a
channel reference), `U` / `I` / `F` (u32 / i32 / f32), `V3` (three floats), `C`
(a curve), `B` / `B32` (byte and 32-bit bools), `STATE` and `SELF` (runtime
scratch, with the op's byte count as the spec's `state`), and `Bytes{size}` - and
one generic reader walks a layout by name (`read_initializer` /
`read_simulator` / the visualizer records), with a matching writer for the
compile direction.

So the port is: the field vocabulary as an enum, the `OpSpec` tables as Rust
data, and one reader plus one writer that walk them. The invariant to keep is the
one the container walk already meets - **a shipped effect must still round-trip
byte-for-byte** - which makes each op safe to land on its own.

**What is actually missing is the *field* names, not the sizes.** The op codes are
already named (`INITIALIZERS: [&str; 25]`, the registry order of the community's
`initializer_specs`), and `particles.rs` already has an `INITIALIZER_SIZES` table
and an `initializer_size` that handles the three variable-length ops (`0x0d`,
`0x0e`, `0x11`), walking each system's initializers with it - so every operand
length is already known and, because the shipped effects round-trip byte-for-byte,
already validated. What the reference adds is the *declarative layout per op*:
which channel reference or float sits at which offset and what it is called. So the
work is to turn `operands = "<hex>"` into `random_float { channel = 3 min = 0.0 max = 1.5 }` -
taking the field names and the field order from `OpSpec`, reusing the sizes that
are already right, and keeping the round-trip byte-exact (which is what proves the
interpretation did not disturb the bytes).

The edit points are settled, and the change is *additive* rather than a format
break: `Initializer` carries `kind` (the op name) and `data` (the operand hex), and
the compile path turns `kind` back into the opcode and appends `from_hex(&data)`
verbatim - so a readable field view can be added alongside `data` with no risk to
byte-exactness, and the compiler can be taught to prefer authored fields later, once
the layouts are proven against shipped effects.

Order the work by the occurrence tallies above rather than by opcode:

1. **initializers** - `random_float` (160), `zero` (37), `position_sphere` (28),
   then `velocity_cone`, `position_box`, `random_int`, `copy`; these use only
   `CH` / `U` / `I` / `F` / `V3`, plus `spline_control_points` for the one
   `Bytes{0x40}` case.
2. **simulators** - `age_age` (35), `burst_emitter` (30), `position_integrate`
   (19), then the ones needing `STATE` / `SELF` (`rate_emitter`, `local_space`,
   `grid_collision`), whose runtime scratch sizes are the spec's `state`.
3. **visualizers** - `billboard` (28), `light` (4), `mesh` (3), `gpu` (2).

Curves (`C`) and gradients are their own sub-format inside a field, so they are
worth doing once rather than per-op.
**Validated on a shipped effect (2026-10-14).** `content/fx/particles/liquid_area/
fire_lingering_edge.particles` decompiles with the readable view filled in, and the
values are semantically right - which is the evidence the ported layout is correct,
not merely self-consistent:

```
type = random_float
data    = 280000009A99993F00000040
decoded = [ "channel = 40", "f32 = 1.2", "f32 = 2" ]        # a random float's min/max
type = random_float
data    = 2C00000000000000DB0FC940
decoded = [ "channel = 44", "f32 = 0", "f32 = 6.2831855" ]  # 0 .. 2*pi radians
```

The labels are the field *kinds* (`channel`, `f32`, `vec3`), not yet the reference's
names (`min`, `max`, `radius`); taking those from `OpSpec` is the next refinement.
`data` remains authoritative - the compiler writes it back verbatim - so the
round-trip is untouched either way.