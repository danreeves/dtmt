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

The labels are now the reference's own field *names* (`min`, `max`, `radius_min`,
`angular_max`, ...), with the field *kind* (`channel`, `f32`, `vec3`) as the
fallback for any op the name table does not cover; the name list is pinned
field-by-field to the layouts by the test, so it cannot drift. `data` remains
authoritative - the compiler writes it back verbatim - so the round-trip is
untouched either way. The remaining step on this item is teaching the compiler to
*accept* authored fields, so an effect becomes editable rather than only readable.
**Typed since (2026-10-14):** the view is now a typed map rather than a list of
strings, because strings cannot be written back - `fields = { channel = 40 min = 1.2
max = 2 }`, using `FieldValue` (`U32` / `I32` / `F32` / `Vec3`, untagged so it
writes as a bare SJSON value).

Two caveats showed up in a shipped effect's dump, and both are now **handled**:

- an `f32` prints widened (`1.2000000476837158`) - exact, but verbose;
- an integer-valued float prints as an integer (`max = 2`), so the untagged enum
  reads it back as the *integer* variant. The encoder coerces it: a field's kind
  comes from the layout, not from the variant it happened to read as, so `U32(2)` in
  an `F32` slot encodes as `2.0f32` - the same four bytes as the original, which is
  why the drift was invisible to byte-exactness all along.

`fields` is now what the compiler uses when it is present: `encode_initializer`
rebuilds the operands from the layout and the names, and a table-driven test proves
it reproduces the bytes for **every** fixed layout. The authored `data` remains the
fallback for an op with no layout and for `spline_control_points`, whose block is
opaque to the view (it emits only the size). So a `max = 2` can now be edited to
`max = 5` and compiled - the effect is editable, not just readable.
**Validated at scale (2026-10-14).** `examples/particles_probe.rs` decompiles every
effect in a bundle (or a directory of them), recompiles it and compares the bytes.
Over **400 bundles it finds 150 effects and all 150 are byte-identical**, in about
eight seconds. Because the compile path now prefers `fields`, those 150 had their
operands rebuilt by `encode_initializer` from the typed view and the ported layouts -
so the layouts, the names and the encoder are validated *together* on real data, not
just by the table-driven test. It is cheap enough to re-run, so it is the regression
guard for the simulator and visualizer work that follows.
**Curves and gradients are fixed size.** The reference defines

```cpp
constexpr std::size_t kCurve = 0x54, kGradient = 0xa4;
```

so a `C` field is 84 bytes and a gradient 164 - which is what makes any layout
containing one *walkable* at all: the curve's contents stay opaque, but its width is
known, so the fields after it can still be reached and named. That is the cheapest
way to bring in the simulator ops that were left out for carrying a curve
(`position_integrate_scaled`, `integrate_float_scaled`, `spiral`).

The remaining unknown for the simulator table is `STATE` / `SELF`: `STATE` is the
per-op runtime scratch the reference's spec records as its `state` field
(`rate_emitter` 12, `burst_emitter` 8, `local_space` 0x44, ...), and `SELF` is a
self-reference. Those few ops stay on the opaque data block until they are settled.
## Visualizers

Their records are the last opaque part, and they differ structurally from the
initializers and simulators - which is why they are not just another row in the same
table. The reference keeps four specs in `kVisualizers`, plus a `gpu_layout()`:

| visualizer | channel block at | fields before it | fields after it |
|---|---|---|---|
| `billboard` | `0x1c` | hash, material, sort, mode | - |
| `light` | `0x00` | - | light_type, flags, falloff/spot angles, volumetric_intensity |
| `mesh` | `0x20` | unit, material_count, ..., flag_1c | - |
| `ribbon` | `0x18` | hash, material, field_14 | - |

Three things make this harder than the earlier slices:

- the `before` / `after` fields are addressed by **payload-absolute offsets**
  (`field_08`, `field_1c`, `field_f8`), so a sequential walker needs the `PAD(n)`
  entries the reference itself uses, computed from those offsets, rather than a plain
  field list;
- between them sits a **block of up to 10 vertex channels** - `kVertexTypeSize` is
  `{4, 8, 12, 16, 4}` for `float1`..`ubyte4` - followed by the vertex stride;
- after the payload size come the **writer list** (and, for a mesh, material
  entries), which is a variable-length tail.

So a visualizer wants the same three-part shape as the reference (`before`,
channels, `after`) plus a tail, not the flat layout the initializers and simulators
use. The widths needed to compute the pads are already known - `kCurve` 0x54, and
`kSizeAt = {0xf4, 0xf8, 0xf8, 0xf0, 0xa0}` for the payload sizes by type.
**The simulator offsets are an independent check on the layout sizes.** They ascend
through the data block, so each simulator's span is the next one's offset minus its
own. For `fire_lingering_edge` that gives:

| span | bytes | against the layout |
|---|---|---|
| `age_age` 0 -> 8 | 8 | 8: two channels - exact |
| `integrate_float_scaled` 204 -> 304 | 100 | 100: four channels + a curve - exact |
| `position_integrate_scaled` 96 -> 204 | 108 | 105 + alignment |
| `burst_emitter` 8 -> 96 | 88 | **nothing**, with a `state` of 8 |

So the offsets corroborate the layouts where they can be checked - which is a second
line of evidence beyond "the fields looked plausible". The exception is
`burst_emitter`, whose block is far larger than its (empty) layout, suggesting its
data carries runtime state beyond the fields. That is worth settling *before* the
`STATE` / `SELF` ops are ported, since it is the one place where the block and the
layout visibly disagree.
**The block is span-driven, not layout-driven.** That resolves the `burst_emitter`
surprise without any new format work: each simulator's data runs from its offset to
the *next* offset, and a layout names only the **leading** fields - the rest of the
span is opaque to the reference too, which validates by the offsets rather than by
`layout_size`. That reading fits all four measurements at once:

| simulator | span | layout | consistent? |
|---|---|---|---|
| `age_age` | 8 | 8 (two channels) | yes |
| `integrate_float_scaled` | 100 | 100 (four channels + curve) | yes |
| `position_integrate_scaled` | 108 | 105, aligned | yes |
| `burst_emitter` | 88 | named prefix is empty | yes - the 88 bytes are simply not named |

which is a better fit than "the layout disagrees with the block". It has a practical
consequence: `describe_simulator` should be **bounded by the span**, not by the end of
the data block, so a layout can never read the next simulator's bytes. That belongs
with the visualizer work, which restructures this area anyway.
**The visualizer record's size is already known.** Reading the parse shows more is
modelled than the name suggests: it reads a size word at a per-type offset
(`VISUALIZER_SIZE_AT`, the reference's `kSizeAt = {
0xf4, 0xf8, 0xf8, 0xf0, 0xa0}`), slices the record body with it, and walks to the
next record - so a visualizer's *shape* is already handled and only its *fields* are
opaque. That makes this a smaller job than "port the visualizers".

The fields are **record-relative**. `light` has no `before` fields and its `after`
fields start at 0, so its first seven - `light_type`, `flags`, `falloff_start`,
`falloff_end`, `spot_angle_start`, `spot_angle_end`, `volumetric_intensity` - are a
plain 28-byte run that the existing layout machinery can name as-is. The eighth,
`field_e8`, sits at 0xe8: the reference expresses that gap by *naming the offset*
rather than padding, so naming it needs the pad resolved first - and the same is true
of `billboard`'s `before`, which must reach its channel block at 0x1c. After the
fields come the channel block (up to 10 entries, `kVertexTypeSize = {4, 8, 12, 16,
4}`) and the writers tail.
## The visualizer record grammar (settled)

`read_visualizer` in the reference gives the whole record, and it is regular enough to
walk sequentially because every pad is *static per type*:

```
type        u32
before      spec.before fields               (billboard's must reach channels_at = 0x1c)
channels    u32 count + 10 x 20-byte descriptors      = 204 bytes, always
after       spec.after fields
stride      u32     (checked: must equal the channels' vertex stride)
writers     u32
first       u32
size        u32     (checked: record start + size == the record's end)
tail        spec.tail fields
(mesh only) `count` material entries at `p + 0x0c`, 16 bytes each
```

The channel block is **fixed at 4 + 10*0x14 = 204 bytes** whatever the count - which is
what makes it expressible as a single field - `after` starts at
`p + channels_at + 204`, and the stride/writers/first/size words follow `after`. The
reference also cross-checks two fields (`stride` against the channel widths, `size`
against the record's end) and, for `billboard`/`ribbon`, a hash over the channels
(`type == 0 || type == 3` in the code, where the spec order is billboard, light, mesh,
ribbon, gpu).

**The VT2 SDK corroborates the channel *content* from the shader side.** The community
compiler's `component_name` maps 0/2/3/5/6 to position/tangent/binormal/texcoord/color,
and `kVertexTypeSize` gives float1..float4/ubyte4 as 4/8/12/16/4. The SDK's
`core/stingray_renderer/shader_libraries/particle_billboard.shader_source` declares the
same set for a billboard vertex:

```
float4 position : POSITION;
float2 corner_info : POSITION1;
float2 size : TEXCOORD7;
float4 color : COLOR;            // under VERTEX_COLOR
```

so the descriptors, the widths and the shader agree - the first independent check the
particle work has had beyond the community compiler itself. `particle_ribbon.shader_source`
and `core/shader_nodes/billboard_particles/*` (9 nodes) are the same kind of help for
the ribbon and the writers.
**Correction: `kSizeAt` is the offset *of the size word*, not the size itself.**
`VISUALIZER_SIZE_AT` (`kSizeAt = {0xf4, 0xf8, 0xf8, 0xf0, 0xa0}`) locates a `u32`
**within** the record - the parse reads `u32_at(bytes, vis + 4 + VISUALIZER_SIZE_AT[kind])`
- and *that word* is the record's size. The earlier note here calling them "the payload
sizes" was wrong.

The two readings agree, which is a useful check. For `billboard` the size word is at
0xf4, and the layout reaches it at `stride_at + 12 = 0xe8 + 12 = 0xf4` - the same
place. For `gpu` it is at 0xa0, and the fields reach it exactly:

| from | bytes | lands on |
|---|---|---|
| 3 x `ID+CONST+RES` (`ID`=4, `RES`=8) | 48 | - |
| `PAD(16)` | 16 | **0x40** - the reference's own `field_40`, `field_44` |
| `ID,ID,ID,CONST` | 16 | **0x50** - `flags_50` |
| `U,U` then `F` x5 | 28 | 0x6c - `field_5c`..`field_6c` |
| `U` x4, `ID`, `U` | 24 | **0x84** - `field_80`, `field_84` |
| 24 + 4 | 28 | **0xa0** - the size word's home |

So the field *names* the reference gives (which encode their offsets) confirm `ID` = 4
and `RES` = 8 for a third time, and they also show the layout is self-consistent with
the size-word location - which is what makes `gpu` mechanical.
**The mesh material entries: partly validated (2026-10-14).** The probe prints them
now, and the *slot* field resolves: `093fc9345f166874` is `lambert1` and
`130f2a9240fb2cb3` is `decal` - Maya-style **material slot names**, which is exactly
what a mesh particle's slots should hold, so the field split is right. The *material*
ids (`3dcbbdf39b71af73`, `decf50c97efd035a`, `db91c504109c760c`) are **not** in the
dictionary, unlike the billboard's, which resolved to a `.material` shipped in its
own bundle. So the binding list is decoded and bounded, and its slot half is
confirmed, but the material half is inconclusive rather than verified - either those
mesh materials are unnamed resources or their names are simply not in the dictionary.