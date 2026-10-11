# File Type: Shader Source

A `.shader_source` is a Stingray source-side **shader library**: a named set of
HLSL chunks (a "shader source" file), plus an include list. A
[`.shader_node`](File%20Type%20-%20Shader%20Node.-.md) code block includes
chunks from one or more of these libraries, and `dtmt build` assembles and
compiles the result into a base [material's](File%20Type%20-%20Material.-.md)
shader section.

There is **no binary layout** for this file type — it is text SJSON read at
build time.

Status tags:

| Tag | Meaning |
|-----|---------|
| **Verified** | Confirmed against the reader and the libraries in the test mod. |
| **Verified in code** | Confirmed against the reader/tests only; no shipped data exercises it. |
| **Observed** | Seen in a file, but a wider sample could add cases. |
| **Guess** | Our inference; not verified. |
| **Unknown** | No reliable information. |

## Schema

Only two top-level keys are read, `includes` and `hlsl_shaders`. The file's
other tables (`render_states`, `sampler_states`, …) are drawing state, not code,
and are ignored. Status: **Verified in code**.

| Key | Type | Meaning | Status |
|-----|------|---------|--------|
| `includes` | string[] | full paths of other libraries this one includes | **Verified** |
| `hlsl_shaders` | map | chunk name → chunk | **Verified** |

A chunk has:

| Key | Type | Meaning | Status |
|-----|------|---------|--------|
| `includes` | string[] | chunks this one needs, resolved across the libraries | **Verified** |
| `code` | string or table | the body | **Verified** |

`code` is either a bare string (one body for every backend) or a table with
optional `shared`, `hlsl` and `glsl` parts. Darktide is D3D12 only, so the
selected body is `shared` followed by `hlsl`; `glsl` is never used. Status:
**Verified in code**.

## Example

```sjson
includes = [ "core/stingray_renderer/shader_libraries/common.shader_source" ]

hlsl_shaders = {
	common = {
		code = """ void common() {} """
	}
	gbuffer = {
		includes = [ "common" ]
		code = {
			shared = """ void shared() {} """
			glsl = """ void glsl_only() {} """
			hlsl = """ void hlsl_only() {} """
		}
	}
}
```

The chunk `gbuffer` compiles as `shared` + `hlsl`; `glsl_only` is not compiled.

## Mapping

* An include names a chunk as `path#chunk`; a bare name is a chunk on its own.
  Only the part after the last `#` is resolved.
* Chunks are looked up by name across all `.shader_source` files under the mod
  root, in path order. The first library that defines the name wins.
* A `.shader_node` code block assembles its body as: the preprocessor defines for
  the compile job, then its `include` chunks in order, then its own `code`. The
  result is compiled with DXC per stage (`vs_6_0` / `ps_6_0`, entry points
  `vs_main` / `ps_main`). The engine's macros `RENDERER_D3D12` and
  `STAGE_VERTEX` / `STAGE_FRAGMENT` are defined around it.

Status: **Verified in code**. The wider build flow is described in
[File Type - Material.-](File%20Type%20-%20Material.-.md) §2.2.

## Open questions

* The other top-level tables (`render_states`, `sampler_states`, …) are not read
  or documented. Status: **Unknown**.
* A chunk referenced by an include that no library defines is an error in the
  generator; how the real toolchain orders or disambiguates same-named chunks
  across libraries is not known beyond "first match in path order".
  Status: **Guess**.
