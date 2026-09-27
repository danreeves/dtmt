//! A reader for the Stingray `.shader_source` library: the authoring-side HLSL
//! chunks a `.shader_node`'s code blocks include.
//!
//! The file is a named set of shader chunks:
//!
//! ```text
//! hlsl_shaders = {
//!     common = {
//!         code = """ ... """
//!         hlsl = """ ... """   // the HLSL variant, when it differs from code
//!         glsl = """ ... """   // portability scaffolding for other renderers
//!     }
//! }
//! ```
//!
//! Darktide is D3D12 only, so [`ShaderSource::hlsl`] prefers `hlsl` and falls
//! back to `code`; `glsl` is kept but never selected. The file's other tables
//! (`render_states`, `sampler_states`, ...) are not read yet: they are drawing
//! state, not code, and a pass's `render_state` reference is a separate decode.

use std::collections::BTreeMap;

use color_eyre::eyre::Result;
use serde::Deserialize;

/// A parsed `.shader_source` file.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShaderSource {
    /// The named HLSL chunks, keyed by the name a code block includes.
    #[serde(default)]
    pub hlsl_shaders: BTreeMap<String, ShaderChunk>,
}

/// One named chunk of a shader library.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShaderChunk {
    /// The shared body, when the variants do not differ.
    #[serde(default)]
    pub code: Option<String>,
    /// The HLSL body, when it differs from `code`.
    #[serde(default)]
    pub hlsl: Option<String>,
    /// The GLSL body. Kept so the file round-trips, never selected on D3D12.
    #[serde(default)]
    pub glsl: Option<String>,
}

impl ShaderChunk {
    /// The body to compile for Darktide: `hlsl` when present, else `code`.
    pub fn hlsl(&self) -> Option<&str> {
        self.hlsl.as_deref().or(self.code.as_deref())
    }
}

impl ShaderSource {
    /// Parses a `.shader_source` file.
    pub fn from_sjson(sjson: &str) -> Result<Self> {
        serde_sjson::from_str(sjson)
            .map_err(|err| color_eyre::eyre::eyre!("failed to parse the shader source: {err}"))
    }

    /// The HLSL for the chunk `name`, or `None` when the library lacks it.
    pub fn hlsl(&self, name: &str) -> Option<&str> {
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
            .map(str::len)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = r#"
        hlsl_shaders = {
            common = {
                code = """ void common() {} """
            }
            gbuffer = {
                code = """ void shared() {} """
                hlsl = """ void hlsl_only() {} """
                glsl = """ void glsl_only() {} """
            }
        }
    "#;

    #[test]
    fn reads_the_chunks() {
        let source = ShaderSource::from_sjson(LIBRARY).expect("parse");
        let names: Vec<&str> = source.names().collect();
        assert_eq!(names, vec!["common", "gbuffer"]);
        assert!(source.hlsl("common").expect("common").contains("common"));
    }

    #[test]
    fn hlsl_wins_over_code_and_glsl_is_never_selected() {
        let source = ShaderSource::from_sjson(LIBRARY).expect("parse");
        let gbuffer = source.hlsl("gbuffer").expect("gbuffer");
        assert!(gbuffer.contains("hlsl_only"), "{gbuffer}");
        assert!(!gbuffer.contains("glsl_only"));
        assert!(!gbuffer.contains("shared"));
    }

    #[test]
    fn a_missing_chunk_is_none() {
        let source = ShaderSource::from_sjson(LIBRARY).expect("parse");
        assert!(source.hlsl("nope").is_none());
    }
}
