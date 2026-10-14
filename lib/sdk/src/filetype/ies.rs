//! The `ies` resource: a baked photometric distribution.
//!
//! Despite the type name, a shipped `ies` is **not** an IESNA LM-63 text file. It is
//! a small fixed structure - four header words and then a float curve that falls
//! monotonically from its first value to its last, which is the shape of a
//! candela-versus-angle table:
//!
//! ```text
//! 0   u32   version     5 in all 1,297 shipped profiles
//! 4   f32   scale       the one header field that varies
//! 8   u32   unknown_8   16 in all 1,297, and the value count / 8
//! 12  u32   unknown_12  256 in all 1,297, and twice the value count
//! 16  [f32] the values, 128 of them, in a 528-byte profile
//! ```
//!
//! Every one of the 1,297 shipped profiles is **exactly 528 bytes** and holds 128
//! values, and `version`, `unknown_8` and `unknown_12` are identical in all of them
//! - 5, 16 and 256 - while `scale` is the header field that actually varies. The
//! count words are still **carried** rather than derived or recomputed: with a
//! single size in the corpus a derived value and a constant are indistinguishable,
//! and writing a constant would be the "read a field, drop it, write a default"
//! fault this code base spends its time removing.
//!
//! No community tool writes this type, so unlike `bones` and `animation` the
//! cooked-resource envelope is not accepted - a shipped variant is the bare body.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type id, for callers that need to recognise the payload.
pub const IES_TYPE: u64 = 0x8f7d_5a2c_0f96_7655;

/// The header's length; the values follow immediately.
const HEADER: usize = 16;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// A baked photometric distribution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ies {
    /// The version word; 5 in every shipped profile.
    pub version: u32,
    /// A float scale beside it.
    pub scale: f32,
    /// Unread count word at offset 8.
    pub unknown_8: u32,
    /// Unread count word at offset 12.
    pub unknown_12: u32,
    /// The distribution, falling from the first value to the last.
    pub values: Vec<f32>,
}

impl Ies {
    /// Reads a profile from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER {
            bail!("an ies is shorter than its {HEADER}-byte header");
        }
        if (bytes.len() - HEADER) % 4 != 0 {
            bail!(
                "an ies body is {} bytes after the header, which is not a whole number of floats",
                bytes.len() - HEADER
            );
        }
        let values = bytes[HEADER..]
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect();
        Ok(Self {
            version: u32_at(bytes, 0),
            scale: f32_at(bytes, 4),
            unknown_8: u32_at(bytes, 8),
            unknown_12: u32_at(bytes, 12),
            values,
        })
    }

    /// Writes the profile back.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(HEADER + self.values.len() * 4);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.scale.to_le_bytes());
        out.extend_from_slice(&self.unknown_8.to_le_bytes());
        out.extend_from_slice(&self.unknown_12.to_le_bytes());
        for value in &self.values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        Ok(out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the resource from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

/// Decompiles an `ies` variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("an ies resource needs a variant");
    };
    let ies = Ies::from_binary(variant.data())?;
    Ok(vec![UserFile::new(ies.to_sjson()?.into_bytes())])
}

/// Compiles an `ies` SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let ies = Ies::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(ies.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::Ies);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A 528-byte profile shaped like the shipped ones: `version 5`, a scale, 16 and
    /// 256, then 128 falling values.
    fn shipped_shaped() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(528);
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&1.5856f32.to_le_bytes());
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&256u32.to_le_bytes());
        for i in 0..128 {
            let value = 3200.0f32 / (1.0 + i as f32);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn a_shipped_shaped_profile_round_trips() {
        let bytes = shipped_shaped();
        assert_eq!(bytes.len(), 528, "the shipped size");
        let ies = Ies::from_binary(&bytes).expect("the sample parses");
        assert_eq!(ies.version, 5);
        assert_eq!(ies.scale, 1.5856);
        assert_eq!(ies.unknown_8, 16);
        assert_eq!(ies.unknown_12, 256);
        assert_eq!(ies.values.len(), 128);
        assert!(ies.values[0] > ies.values[127], "the curve falls");
        assert_eq!(ies.to_binary().unwrap(), bytes, "byte-for-byte");
        // And through the SJSON a build uses.
        let sjson = ies.to_sjson().unwrap();
        assert_eq!(Ies::from_sjson(&sjson).unwrap(), ies);
    }

    #[test]
    fn a_body_that_is_not_whole_floats_is_rejected() {
        let mut bytes = shipped_shaped();
        bytes.push(0);
        assert!(Ies::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_truncated_header_is_rejected() {
        assert!(Ies::from_binary(&[5, 0, 0, 0]).is_err());
    }
}
