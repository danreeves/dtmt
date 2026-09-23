# Shader decompilation

Shipped base materials carry compiled DXBC/DXIL programs (see
[the material format](File%20Type%20-%20Material.-.md)). When you want a
*targeted* change to an existing shader instead of replacing it with a
hand-written one, the compiled program can be translated back to editable HLSL:

```text
material data file
  -> shader43 --dump <dir>                    frames decoded to DXBC containers
  -> dxil-spirv <container> -o shader.spv     DXIL (SM 6.x) to SPIR-V
  -> spirv-cross --hlsl --shader-model 60     SPIR-V to HLSL
  -> fix up the entry point and semantics
  -> edit
  -> dxc -T ps_6_0 -E ps_main shader.hlsl     HLSL back to a DXBC/DXIL container
  -> shader43 --rebuild <dir> --replace-ps    re-framed and spliced automatically
```

There is no HLSL decompiler for DXIL: the disassembly produced by
`dxc -dumpbin` is LLVM IR, and `dxc` cannot compile IR back (it only accepts
HLSL). The route above goes through another toolchain instead.

## Building the tools

Both tools are open source and must be built locally:

- [dxil-spirv](https://github.com/HansKristian-Work/dxil-spirv)
  ```shell
  git clone --depth 1 --recursive https://github.com/HansKristian-Work/dxil-spirv
  cmake -B dxil-spirv/build -G "Visual Studio 17 2022" -A x64 \
      -DPython3_EXECUTABLE=<python.exe>
  cmake --build dxil-spirv/build --config Release --target dxil-spirv
  ```
  Its SPIRV-Tools submodule needs a Python 3 interpreter for code generation.
  A normal install works; the embeddable Python zip from python.org is enough
  when Python is not installed.

- [SPIRV-Cross](https://github.com/KhronosGroup/SPIRV-Cross)
  Use the patched `hlsl-minlod` branch of the
  [danreeves/SPIRV-Cross fork](https://github.com/danreeves/SPIRV-Cross/tree/hlsl-minlod):
  ```shell
  git clone --depth 1 -b hlsl-minlod https://github.com/danreeves/SPIRV-Cross
  cmake -B SPIRV-Cross/build -G "Visual Studio 17 2022" -A x64 \
      -DSPIRV_CROSS_CLI=ON -DSPIRV_CROSS_ENABLE_TESTS=OFF
  cmake --build SPIRV-Cross/build --config Release --target spirv-cross
  ```
  Upstream refuses the min-LOD clamp operand (`MinLod texture operand not
  supported in HLSL`), which many shipped shaders use. The branch emits HLSL's
  `Sample(sampler, uv, offset, clamp)` form instead. Plain upstream works for
  every shader that does not use the clamp; fall back to the fork only when the
  decompiler rejects one.

## What the output looks like

For the UI shader's pixel program, the patched pipeline emits (abridged):

```hlsl
SamplerState _29[] : register(s0, space2);
SamplerState _31 : register(s0, space31);
Texture2D<float4> _9[] : register(t0, space2);

float4 _100 = _9[asuint(_25_m0[1u]).x + 0u].Sample(_31, float2(CUSTOM_1.x, CUSTOM_1.y));
float4 _106 = _9[_74].Sample(_29[_67], float2(CUSTOM_1.x, CUSTOM_1.y), int2(0, 0), _100.x * 32.0f);
```

The bindless resource arrays, the static sampler, both samples and the min-LOD
clamp all survive. Vertex and pixel shaders from a sample of 20 base materials
across several families all decompiled (20/20 VS, 20/20 PS).

## One-command decompilation

`shader43 --decompile` runs both tools for every program in a material and
applies the mechanical fix-ups:

```shell
cargo run -p sdk --example shader43 -- --decompile out \
    --dxil-spirv <dxil-spirv> --spirv-cross <spirv-cross> <material data file>...
```

It writes, per program, `<name>_pNN.original.dxbc` (the shipped container),
`.spv` and `.hlsl`. `--program <index>` limits it to one program. The tools are
looked up from `--dxil-spirv`/`--spirv-cross`, then `DXIL_SPIRV`/`SPIRV_CROSS`,
then `PATH`.

The HLSL is ready to compile:

- the entry point is renamed to `vs_main` or `ps_main`,
- the signature structs are rebuilt in the shipped register order with the
  original names and indices, including elements the shader does not read
  (`spirv-cross` drops them, which shifts the registers and fails the interface
  check),
- bindings, cbuffers and the shader body are left as generated.

```shell
dxc -T ps_6_0 -E ps_main <name>_pNN.hlsl -Fo <name>_pNN.dxbc
cargo run -p sdk --example shader43 -- --rebuild out --replace-ps <name>_pNN.dxbc <material data file>
```

Or drop the `.hlsl` next to the material and let `dtmt build` compile it (see
`docs/File Type - Material.-.md`). Every program of four sample materials
(including a 96-program HUD library) compiled with `dxc` and passed the
interface check, and the UI material rendered correctly in game.

### Using the output as a base

The decompiled HLSL is ordinary HLSL, so custom effects are added to it the same
way as to a hand-written shader. As a worked example, the tint and the wave from
`shaders/gui_tint.hlsl` and `shaders/gui_wave.hlsl` were rebuilt on top of the
decompiled `gui` programs by adding four lines to `ps_main`:

```hlsl
float wave_time = _20_m0[90u].x;                                     // global_viewport.time
CUSTOM_1.x += sin(CUSTOM_1.y * 30.0 + wave_time * 2.0) * 0.02;       // the wave
CUSTOM_1.y += sin(CUSTOM_1.x * 30.0 + wave_time * 1.7) * 0.02;
SV_Target.rgb *= (0.75 + 0.25 * cos(wave_time * 4.0)) * _25_m0[14u].rgb;  // mod_tint
```

The slots come from the decompiled cbuffer arrays: `_20_m0[90].x` is
`global_viewport.time` (b0, offset 1440) and `_25_m0[14]` is a material
variable (b1, offset 224). The decompiled cbuffers are flat `float4` arrays, so
the offsets are `byte_offset / 16`, and the shader's variable table says which
of those offsets a material may set (`shader43 --variables <dictionary.csv>
<material>`, or add your own record with `shader43 --add-variable`, see
`shaders/README.md`). The wave lives in the pixel stage because the UI
background is a single quad: a clip-space displacement in the vertex stage only
moves its four corners. Lua drives the variable through
`widget.style.texture.material_values`.

## Fix-ups before you can compile

The generated HLSL is a starting point, not the original source. `--decompile`
does the first two for you; the rest is on you:

- **Entry point**: `spirv-cross` calls it `main`; rename it to `ps_main` or
  `vs_main` (or compile with `-E main`).
- **Semantics**: the original semantic names are lost and replaced with
  `TEXCOORD0`, `TEXCOORD1`, `CUSTOM_1`, ... Restore them from the shipped
  container's `ISG1`/`OSG1` (`dxc -dumpbin` prints them, and the `shader43`
  example lists the chunks). The engine's input layouts and the other stages are
  unchanged, so the signatures have to match exactly; the `shader43` interface
  check and `dtmt build` will refuse anything else.
- **Uninitialized outputs**: the compiler may have optimized away writes the
  original HLSL had (e.g. an interpolant that the pixel shader does not read).
  Declaring it is enough for the signature, but be aware it is not computed.
- **Names and structure**: everything is renamed and control flow is flattened;
  `#define`s and the original shader source organisation are gone.
- **No root signature**: the engine supplies it, so keep the registers and
  spaces that the decompiled code uses.
- **Not everything translates**: some constructs have no HLSL backend support
  (the fork's branch only adds the 2D implicit-LOD min-LOD case). If
  `dxil-spirv` or `spirv-cross` refuses a shader, fall back to the
  disassembly and reconstruct the part you need by hand.
