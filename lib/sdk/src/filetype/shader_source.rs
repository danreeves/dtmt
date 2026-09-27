//! A reader for the Stingray `.shader_source` library: the authoring-side HLSL
//! chunks a `.shader_node`'s code blocks include.
//!
//! The file is a named set of shader chunks, plus a file-level include list:
//!
//! ```text
//! includes = [ "core/stingray_renderer/shader_libraries/common.shader_source" ]
//!
//! hlsl_shaders = {
//!     common = {
//!         code = """ ... """          // the body, shared by the backends
//!     }
//!     skinning = {
//!         includes = [ "common" ]     // chunks this one needs, by name
//!         code = {
//!             glsl = """ ... """      // portability scaffolding
//!             hlsl = """ ... """      // the HLSL body
//!         }
//!     }
//! }
//! ```
//!
//! Darktide is D3D12 only, so [`ShaderChunk::hlsl`] takes the `shared` part and
//! the `hlsl` part (in that order) and ignores `glsl`. The file's other tables
//! (`render_states`, `sampler_states`, ...) are not read yet: they are drawing
//! state, not code, and a pass's `render_state` reference is a separate decode.

use std::collections::BTreeMap;

use color_eyre::eyre::Result;
use serde::Deserialize;

/// A parsed `.shader_source` file.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShaderSource {
    /// The files this library includes, as full paths, when it names them.
    #[serde(default)]
    pub includes: Vec<String>,
    /// The named HLSL chunks, keyed by the name a code block includes.
    #[serde(default)]
    pub hlsl_shaders: BTreeMap<String, ShaderChunk>,
}

/// One named chunk of a shader library.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShaderChunk {
    /// The chunks this chunk includes, by name, resolved across the pool.
    #[serde(default)]
    pub includes: Vec<String>,
    /// The body: a bare string, or the `shared`/`hlsl`/`glsl` table.
    #[serde(default)]
    pub code: Option<CodeParts>,
}

/// A `code` value: a bare string, or the table of per-backend parts.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum CodeParts {
    /// `code = """ ... """`: one body for every backend.
    Text(String),
    /// `code = { shared = ... hlsl = ... glsl = ... }`.
    Parts(CodeTable),
}

/// The parts of a `code` table. Every part is optional; the file writes the
/// ones its backends need.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CodeTable {
    /// The body shared by every backend.
    #[serde(default)]
    pub shared: Option<String>,
    /// The HLSL body.
    #[serde(default)]
    pub hlsl: Option<String>,
    /// The GLSL body. Kept so the file round-trips, never selected on D3D12.
    #[serde(default)]
    pub glsl: Option<String>,
}

impl CodeParts {
    /// The body to compile for Darktide: `shared` then `hlsl`; `glsl` is
    /// ignored.
    pub fn hlsl(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(table) => {
                let mut out = String::new();
                if let Some(shared) = &table.shared {
                    out.push_str(shared);
                    out.push('\n');
                }
                if let Some(hlsl) = &table.hlsl {
                    out.push_str(hlsl);
                }
                out
            }
        }
    }
}

impl ShaderChunk {
    /// The body to compile for Darktide, when the chunk has one.
    pub fn hlsl(&self) -> Option<String> {
        self.code.as_ref().map(CodeParts::hlsl)
    }
}

impl ShaderSource {
    /// Parses a `.shader_source` file.
    pub fn from_sjson(sjson: &str) -> Result<Self> {
        serde_sjson::from_str(sjson)
            .map_err(|err| color_eyre::eyre::eyre!("failed to parse the shader source: {err}"))
    }

    /// The HLSL for the chunk `name`, or `None` when the library lacks it.
    pub fn hlsl(&self, name: &str) -> Option<String> {
        self.hlsl_shaders.get(name)?.hlsl()
    }

    /// The chunk names the library defines.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.hlsl_shaders.keys().map(String::as_str)
    }

    /// The bytes of the HLSL the library defines, for a caller that wants to
    /// hand them to the compiler.
    pub fn hlsl_len(&self) -> usize {
        self.hlsl_shaders
            .values()
            .filter_map(ShaderChunk::hlsl)
            .map(|text| text.len())
            .sum()
    }
}

/// The chunk name an `include` names: the part after the last `#`. An include
/// without a `#` is a chunk name on its own.
pub fn include_chunk(include: &str) -> &str {
    include.rsplit('#').next().unwrap_or(include)
}

/// The library and chunk an include names, searched across the given libraries
/// in order.
pub fn find_chunk<'a>(
    libraries: &'a [ShaderSource],
    include: &str,
) -> Option<(&'a ShaderSource, &'a ShaderChunk)> {
    let name = include_chunk(include);
    libraries
        .iter()
        .find_map(|library| library.hlsl_shaders.get(name).map(|chunk| (library, chunk)))
}

/// The HLSL an include names, searched across the given libraries in order.
/// `None` when no library defines the chunk; the caller decides whether that is
/// an error or a chunk from another source.
pub fn resolve_include(libraries: &[ShaderSource], include: &str) -> Option<String> {
    find_chunk(libraries, include).and_then(|(_, chunk)| chunk.hlsl())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = r#"
        includes = [ "core/stingray_renderer/shader_libraries/other.shader_source" ]
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
    "#;

    #[test]
    fn reads_the_chunks() {
        let source = ShaderSource::from_sjson(LIBRARY).expect("parse");
        let names: Vec<&str> = source.names().collect();
        assert_eq!(names, vec!["common", "gbuffer"]);
        assert!(source.hlsl("common").expect("common").contains("common"));
        assert_eq!(
            source.includes,
            vec!["core/stingray_renderer/shader_libraries/other.shader_source"]
        );
    }

    #[test]
    fn shared_and_hlsl_are_taken_and_glsl_is_never_selected() {
        let source = ShaderSource::from_sjson(LIBRARY).expect("parse");
        let gbuffer = source.hlsl("gbuffer").expect("gbuffer");
        assert!(gbuffer.contains("shared"), "{gbuffer}");
        assert!(gbuffer.contains("hlsl_only"), "{gbuffer}");
        assert!(!gbuffer.contains("glsl_only"), "{gbuffer}");
        let chunk = source.hlsl_shaders.get("gbuffer").expect("chunk");
        assert_eq!(chunk.includes, vec!["common"]);
    }

    #[test]
    fn a_missing_chunk_is_none() {
        let source = ShaderSource::from_sjson(LIBRARY).expect("parse");
        assert!(source.hlsl("nope").is_none());
    }

    #[test]
    fn includes_resolve_across_libraries() {
        let first = ShaderSource::from_sjson(LIBRARY).expect("first");
        let second =
            ShaderSource::from_sjson(r#"hlsl_shaders = { extra = { code = """ void extra() {} """ } }"#)
                .expect("second");
        let libraries = [first, second];
        let resolved =
            resolve_include(&libraries, "core/stingray_renderer/shader_libraries/common#gbuffer")
                .expect("gbuffer");
        assert!(resolved.contains("hlsl_only"), "{resolved}");
        assert!(
            resolve_include(&libraries, "somewhere#extra")
                .expect("extra")
                .contains("extra")
        );
        assert!(resolve_include(&libraries, "somewhere#missing").is_none());
        assert_eq!(include_chunk("path#chunk"), "chunk");
        assert_eq!(include_chunk("plain"), "plain");
    }
}
