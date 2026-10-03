//! The engine's permutation keys and their ids.
//!
//! There are **two** key spaces, and they differ in more than the tail:
//!
//! - the **query key**: `<shader>:<context>:<defines, sorted>:
//!   PLATFORM_<platform>:RENDERER_<renderer>` - the VT2 SDK compiler logs these,
//!   e.g. `gui:default:DIFFUSE_MAP:ONE_BIT_ALPHA:PLATFORM_WIN32:RENDERER_D3D12`,
//!   and the id a Darktide section carries for a query is the high 32 bits of
//!   MurmurHash64A over it (see [`key`], [`id`]);
//! - the **library name**: `<shader>:<tokens in declaration order>`, lowercased,
//!   with **no context token** - this is what `debug_file_index.sjson` lists and
//!   what a `.editor` registry entry names, e.g. `gui:diffuse_map:one_bit_alpha`.
//!   The file is that name plus `.shader_library` (see [`library_file`]).
//!
//! The two spaces were separated by re-reading the VT2 SDK compiler's own output
//! (220/220 against `debug_file_index.sjson`; the tool is
//! `examples/vt2_editor.rs`). VT2 is an older engine than Darktide, so the
//! concrete tokens (shader names, contexts, defines) differ; the shapes do not.
//! Treat the tokens as inputs.

use crate::murmur::{IdString64, Murmur64};

/// The id the engine assigns to a permutation: the high 32 bits of its 64-bit
/// hash.
pub fn id(key: &str) -> u32 {
    (u64::from(IdString64::from(Murmur64::hash(key.as_bytes()))) >> 32) as u32
}

/// The file name of a permutation's compiled shader library.
pub fn library_file(key: &str) -> String {
    format!("{}.shader_library", key.to_lowercase())
}

/// The 64-bit hash a bundle database knows a compiled library file by.
pub fn library_hash(key: &str) -> u64 {
    u64::from(IdString64::from(Murmur64::hash(library_file(key).as_bytes())))
}

/// Builds the full key: the defines sorted (as the compiler emits them), then
/// the platform and renderer tail.
pub fn key(
    shader: &str,
    context: &str,
    defines: &[&str],
    platform: &str,
    renderer: &str,
) -> String {
    format!(
        "{}:PLATFORM_{platform}:RENDERER_{renderer}",
        library_key(shader, context, defines)
    )
}

/// Builds the key without the platform/renderer tail: the form the compiled
/// library file is named after. Empty parts are skipped, so a shader with no
/// context (`linearize_depth`) or no defines still produces a canonical key.
pub fn library_key(shader: &str, context: &str, defines: &[&str]) -> String {
    let mut defs: Vec<&str> = defines.to_vec();
    defs.sort_unstable();

    let mut parts: Vec<&str> = Vec::with_capacity(defs.len() + 2);
    for part in [shader, context] {
        if !part.is_empty() {
            parts.push(part);
        }
    }
    parts.extend(defs);
    parts.join(":")
}

#[cfg(test)]
mod test {
    use super::*;

    fn hash64(s: &str) -> u64 {
        u64::from(IdString64::from(Murmur64::hash(s.as_bytes())))
    }

    /// The VT2 SDK compiler writes `debug_file_index.sjson`, mapping every
    /// compiled file to its name. These are pairs from a local compile run.
    #[test]
    fn library_names_hash_to_their_file_names() {
        assert_eq!(
            hash64("gui:diffuse_map:one_bit_alpha.shader_library"),
            0x1AE3_0551_22B2_7B54
        );
        assert_eq!(
            hash64("linearize_depth.shader_library"),
            0x0205_D79D_3BE9_1999
        );
        assert_eq!(
            hash64("gui_gradient:diffuse_map:gradient.shader_library"),
            0x01B5_B82D_032E_C091
        );
    }

    /// The compiled libraries carry their keys' ids: the high half of
    /// `murmur64a` over the full key. Both pairs were read back out of the
    /// libraries the compiler produced.
    #[test]
    fn keys_hash_to_the_ids_inside_their_libraries() {
        assert_eq!(
            id("gui:default:DIFFUSE_MAP:ONE_BIT_ALPHA:PLATFORM_WIN32:RENDERER_D3D12"),
            0xE39F_328F
        );
        assert_eq!(
            id("gui_gradient:default:DIFFUSE_MAP:CIRCULAR_MASK:UV_SCALE:PLATFORM_WIN32:RENDERER_D3D12"),
            0x2B31_4E79
        );
    }

    #[test]
    fn keys_sort_their_defines() {
        assert_eq!(
            key(
                "gui",
                "default",
                &["ONE_BIT_ALPHA", "DIFFUSE_MAP"],
                "WIN32",
                "D3D12"
            ),
            "gui:default:DIFFUSE_MAP:ONE_BIT_ALPHA:PLATFORM_WIN32:RENDERER_D3D12"
        );
        assert_eq!(library_key("linearize_depth", "", &[]), "linearize_depth");
        assert_eq!(
            library_file("Linearize_Depth"),
            "linearize_depth.shader_library"
        );
    }

    /// The compiled library name is *not* the query key: it drops the context
    /// and keeps the declaration's token order. Both examples are entries the
    /// VT2 compiler wrote into its `.editor` registries and into
    /// `debug_file_index.sjson` (`examples/vt2_editor.rs` verifies all 220).
    #[test]
    fn library_names_drop_the_context_and_keep_declaration_order() {
        // `{ shader="decal" defines="DIFFUSE_MAP NORMAL_MAP" }` ->
        // `decal:diffuse_map:normal_map.shader_library`.
        assert_eq!(
            library_file("decal:DIFFUSE_MAP:NORMAL_MAP"),
            "decal:diffuse_map:normal_map.shader_library"
        );
        // `{ shader="apply_fog" defines="DEBUG_FOG CALCULATE_LIGHTING" }` ->
        // `apply_fog:debug_fog:calculate_lighting.shader_library`; the order is
        // the declaration's (sorted would be calculate_lighting:debug_fog) and
        // there is no `default` context token.
        assert_eq!(
            library_file("apply_fog:DEBUG_FOG:CALCULATE_LIGHTING"),
            "apply_fog:debug_fog:calculate_lighting.shader_library"
        );
    }
}
