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
  ```shell
  git clone --depth 1 https://github.com/KhronosGroup/SPIRV-Cross
  cmake -B SPIRV-Cross/build -G "Visual Studio 17 2022" -A x64 \
      -DSPIRV_CROSS_CLI=ON -DSPIRV_CROSS_ENABLE_TESTS=OFF
  cmake --build SPIRV-Cross/build --config Release --target spirv-cross
  ```
  Then apply `spirv-cross-minlod.patch` from this directory and rebuild. Without
  it, the HLSL backend refuses the min-LOD clamp operand
  (`MinLod texture operand not supported in HLSL`), which many shipped shaders
  use. The patch emits HLSL's `Sample(sampler, uv, offset, clamp)` form instead.

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

## Fix-ups before you can compile

The generated HLSL is a starting point, not the original source:

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
  (the patch above only handles the 2D implicit-LOD min-LOD case). If
  `dxil-spirv` or `spirv-cross` refuses a shader, fall back to the
  disassembly and reconstruct the part you need by hand.
