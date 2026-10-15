# Shader experiment - can the device block be grown?

This is the concrete plan for the question `Shader RE TODO.md` calls its "Next action". It is written
out separately because it is the one experiment that decides a whole direction, and because it cannot
be run in this tree.

## The question

A section's per-program **device block** carries that program's resource records and the material's
cbuffer variable descriptors. The question is whether that block can be **grown** - whether a compiler
can generate the records for a *new* material variable - or whether the record stream is
engine-compiled and fixed.

The answer decides whether custom shader interfaces are:

- **a compiler problem** - if the block grows, generation is a matter of modelling its record stream,
  and a mod can declare its own variables; or
- **a dead end** - if it does not, a custom base material stays bound to a shipped interface.

## Why the VT2 SDK rather than this tree

This tree cannot run it, for two reasons read from the code:

- the engine data is deleted, so there is no side file to clone a record into;
- `material_records` (`crates/dtmt/src/cmd/build.rs`) takes a material table's variables from the
  **graph's exports** when the material has an evaluation, and from the node's `variables` only when
  it has none - and a `.shader_node` material is the first case. So a declaration's `inputs` never
  reach the table. Measured: adding an input to `ui_default_base.shader_node` left the generated
  section **byte-identical** (`CD9854D69AC9A5B7`, 421,684 B).

The Vermintide 2 SDK is the right vehicle because it ships the authoring files *and* the compiled
results, it has example mods to carry a change, and it is the same engine - see
[Shader Authoring - VT2 SDK Reference](Shader%20Authoring%20-%20VT2%20SDK%20Reference.md), which
records that the SDK's `.shader_node` files carry the HLSL inline, cbuffer members included.

## What is on disk

| path (under `C:\Program Files (x86)\Steam\steamapps\common\Vermintide 2 SDK`) | what it gives |
|---|---|
| `core/stingray_renderer/output_nodes/standard_base.shader_node` | the output node whose HLSL declares `CBUFFER_START(c_per_object)` - the file to edit |
| `core/shader_nodes/*.shader_node` | the node definitions (texture inputs, permutation defines) |
| `core/stingray_renderer/shader_libraries/**/*.shader_source` | the library sources (`common` 71 KB, `lighting_common` 59 KB, `noise` 130 KB, ...) |
| `example_mods/endurance_badges`, `example_mods/streamable_resources` | the test mods, each with `resource_packages/*.package` and a `.mod` |
| `TEMP/` | the compile pipeline's output (the notes already cite it for the `.wwise_bank` sources) |
| `BUILD/mods/streamable_resources/*.mod_bundle` | a compiled mod bundle to compare against |
| `compile_streamable_resources.bat` | the SDK's own build entry point |

Vermintide 2 itself is installed at `E:\SteamLibrary\steamapps\common\Warhammer Vermintide 2`, so the
in-game check is possible.

## The experiment

1. **Pick the node**: `core/stingray_renderer/output_nodes/standard_base.shader_node`, whose cbuffer
   the material's table already describes.
2. **Add one member** to its `CBUFFER_START(c_per_object)` block, keeping HLSL packing - a `float4`
   starts a 16-byte row, a `float2` takes an 8-byte alignment, and the rules the shipped tables
   follow are the ones `pack()` in `crates/dtmt/src/cmd/build.rs` implements.
3. **Build** with the SDK's toolchain (`compile_streamable_resources.bat`, or the `vmb` workspace at
   `C:\dev\vmb\mods` - the exact invocation is still to be confirmed).
4. **Run** Vermintide 2.
5. **Observe**: does a material that declares the new name **bind**?

**No record patching is needed.** The VT2 toolchain derives the block, the canonical records, the
packed copies, the descriptors and the tails *from the sources* - which is the whole point, and which
makes this a much smaller experiment than the plan in `Shader RE TODO.md` suggests. That plan's
"patch it consistently" step is an artefact of its era: it assumed a hand-carried section, which is
what Darktide's engine data was.

## What each outcome means

- **It binds.** The block can be grown. Generation is then a matter of modelling its record stream -
  the shapes are already documented in `Shader RE TODO.md` ("The tail block is the program's
  device-table stream", "The channel table, with names resolved through the dictionary") - and custom
  interfaces become a compiler problem with a known shape.
- **It does not.** The interface is engine-compiled. Custom base materials stay bound to shipped
  interfaces, and the material workstream's ceiling is what it is today: our own programs, textures
  through the library's channels, and values for the library's known variables.

## The oracle

Three, in increasing strength:

1. the SDK's own compile output (`TEMP/`), to see what the toolchain made of the edit;
2. `BUILD/mods/streamable_resources/*.mod_bundle`, a compiled mod bundle to diff against;
3. the game itself - a material declaring the new name either binds or it does not.

## If the SDK route does not pan out

The Darktide equivalent of "add a variable" is a **`shader` graph** whose exports name it, because
that is the branch `material_records` reads. That is a larger piece of work than the VT2 experiment
and it should only be attempted once the block question is answered, since the answer decides whether
it is worth anything.

## Status

**Not run.** Nothing in the SDK tree has been modified, and this page is a plan, not a result.
