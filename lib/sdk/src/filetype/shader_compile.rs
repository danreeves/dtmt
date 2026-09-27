//! Compiling the dialect's programs with DXC.
//!
//! The loading and the FFI live in the `dxc` crate; this is the SDK's entry
//! point. The compiler library is loaded once per process, from the path
//! [`set_library`] pins (the caller's config), then `DTMT_DXC_DLL`, the tool's
//! directory and the platform's usual places - see `dxc::find_library`.
//!
//! The 6.0 profiles emit a `DXBC` container holding DXIL, which is what the
//! engine's programs are (see [`super::shader_node::profile_for`]).

use color_eyre::eyre::Result;

pub use dxc::{find_library, set_library};

/// Compiles one HLSL source for a profile and entry point, returning the
/// container DXC produced.
pub fn compile(source: &str, profile: &str, entry: &str) -> Result<Vec<u8>> {
    dxc::compile(source, profile, entry)
}
