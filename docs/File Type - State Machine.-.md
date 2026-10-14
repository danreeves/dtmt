# File Type - State Machine

A `state_machine` (`0xa486d4045106165c`) is an animation state machine: layers of states, the
events and variables that drive them, blend sets, constraints and ragdolls.

**4,006 of 4,006** shipped machines decode and re-encode byte for byte, across 9,322 layers and
101,560 states, from 221 bytes to 3.6 MB.

## The layout

```text
layers                  Array(Layer)
event_names             Array(u32)
variable_names          Array(u32)
variables               Array(f32)
unknown_pairs           Array({f32, f32})
blend_sets              Array(BlendSet)
constraint_targets      Array(u32)
constraint_positions    Array({f32, f32, f32})
constraints             Array(byte)
ragdolls                Array(Ragdoll)
unknown_records         Array({u32 x 6, f32})
unknown_last            f32
```

`Array(x)` is a `u32` count followed by that many `x`, so **every count is derived on write** from
the vector it covers. `Layer` is `Array(State)` plus a `default_state` index; `State` is the
reference's `unk1`…`unk14` sequence; `BlendSet`, `Ragdoll`, `Marker`, `Transition`, `Switch` and
`BlendSettings` are the small fixed shapes the reference names.

Fields the reference calls `unkN` keep that name here, and `unknown_*` where the reference itself
has no name - nothing is renamed into a guess.

The reader is **strict**: it walks the whole body and rejects anything left over rather than
silently stopping early, which is what makes a wrong field order show up as a failure on the sweep
instead of as quiet corruption.

## Grounding

The layout is the DT variant of the community reader (`bitsquid-blender-tools`,
`stingray/animation_state_machine_resource.py` and the modules it pulls in: `animation_layer`,
`animation_state`, `animation_blend_settings`, `animation_transition`, `blend_set`, `ragdoll`,
`generic_structures`).

A bundle variant stores the **bare body**; the reference reads a `DTHeader` first because it is
written for the standalone cooked form.

## Open

A non-finite float does not keep its payload bits through SJSON: the binary path is exact for all
4,006, but 2,386 of the smaller machines re-encode to slightly different bytes on the SJSON path.
The engine treats any NaN alike, and the writer now emits the `inf`/`-inf`/`nan` words its own
reader accepts - a defect that blocked those machines from building at all until it was fixed.
