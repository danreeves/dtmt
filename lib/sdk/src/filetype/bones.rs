//! The `bones` resource: a cooked skeleton - the joint names, their 32-bit ids
//! and the LOD joint counts.
//!
//! The body is `{u32 joint count, u32 lod count, joint hashes, lod counts,
//! null-terminated names}`. A joint's id is [`Murmur32`] of its name - the upper
//! 32 bits of the seed-zero Murmur64 the engine uses for an `IdString32`.
//!
//! A Darktide bundle variant stores the **body alone**: none of 1,142 shipped
//! `bones` variants carries an envelope, and the resource's type and name live in
//! the bundle file entry. The community writer's `wrap_cooked_resource` instead
//! emits a standalone blob with a 38-byte envelope (`{type hash, name hash, 1, 12
//! zero bytes, body size, 1, stream size}`); [`Bones::from_binary`] accepts that
//! form too, so such a blob still loads.

use std::io::Cursor;

use color_eyre::{Report, Result, eyre::bail};

use crate::binary::sync::ReadExt;
use crate::bundle::file::UserFile;
use crate::murmur::{IdString64, Murmur32};
use crate::{BundleFile, BundleFileType, BundleFileVariant};

/// The type hash the cooked-resource envelope carries for a bones resource.
pub const BONES_TYPE: u64 = 0x18de_ad01_056b_72e9;

/// The cooked-resource envelope's fixed size.
const ENVELOPE: usize = 38;

/// Whether `bytes` opens with a well-formed `wrap_cooked_resource` envelope.
fn envelope_present(bytes: &[u8]) -> bool {
    let type_hash = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
    type_hash == BONES_TYPE
        && bytes[16] == 1
        && bytes[33] == 1
        && bytes[17..29].iter().all(|b| *b == 0)
}

/// A cooked skeleton.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bones {
    /// The joint names, in order.
    pub names: Vec<String>,
    /// The LOD joint counts; the first is the full joint count.
    pub lods: Vec<u32>,
}

impl Bones {
    /// Reads a bones resource from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        // A bundle variant is the bare body; a `wrap_cooked_resource` blob wraps
        // it in the 38-byte envelope described in the module docs.
        let body = if bytes.len() >= ENVELOPE && envelope_present(bytes) {
            let body_size = u32::from_le_bytes(bytes[29..33].try_into().unwrap()) as usize;
            let stream_size = u32::from_le_bytes(bytes[34..38].try_into().unwrap()) as usize;
            if bytes.len() != ENVELOPE + body_size + stream_size {
                bail!("the bones envelope's sizes do not match its length");
            }
            &bytes[ENVELOPE..ENVELOPE + body_size]
        } else {
            bytes
        };
        let mut r = Cursor::new(body);
        let joints = r.read_u32()? as usize;
        let lod_count = r.read_u32()? as usize;
        let mut hashes = Vec::with_capacity(joints);
        for _ in 0..joints {
            hashes.push(r.read_u32()?);
        }
        let mut lods = Vec::with_capacity(lod_count);
        for _ in 0..lod_count {
            lods.push(r.read_u32()?);
        }
        let mut names = Vec::with_capacity(joints);
        for _ in 0..joints {
            let mut name = Vec::new();
            loop {
                let byte = body.get(r.position() as usize).copied();
                let Some(byte) = byte else {
                    bail!("a bone name is not null-terminated");
                };
                r.set_position(r.position() + 1);
                if byte == 0 {
                    break;
                }
                name.push(byte);
            }
            names.push(String::from_utf8(name).map_err(Report::new)?);
        }

        for (name, hash) in names.iter().zip(&hashes) {
            if u32::from(Murmur32::hash(name.as_bytes())) != *hash {
                bail!("the bone name '{name}' does not hash to its stored id");
            }
        }

        Ok(Self { names, lods })
    }

    /// Writes the cooked body back, recomputing each joint's id from its name.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        body.extend_from_slice(&(self.names.len() as u32).to_le_bytes());
        body.extend_from_slice(&(self.lods.len() as u32).to_le_bytes());
        for name in &self.names {
            body.extend_from_slice(&u32::from(Murmur32::hash(name.as_bytes())).to_le_bytes());
        }
        for lod in &self.lods {
            body.extend_from_slice(&lod.to_le_bytes());
        }
        for name in &self.names {
            body.extend_from_slice(name.as_bytes());
            body.push(0);
        }

        Ok(body)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(Report::new)
    }

    /// Reads the resource from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        let bones: Self = serde_sjson::from_str(text).map_err(Report::new)?;
        // Round-trip the body so a name/id mismatch is caught on load.
        Self::from_binary(&bones.to_binary()?)
    }
}

/// Decompiles a bones variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &crate::Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a bones resource needs a variant");
    };
    let bones = Bones::from_binary(variant.data())?;
    Ok(vec![UserFile::new(bones.to_sjson()?.into_bytes())])
}

/// Compiles a bones SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, bones_sjson: &str) -> Result<BundleFile> {
    let bones = Bones::from_sjson(bones_sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(bones.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::Bones);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// The community's `bones_writer` example body: three joints, two LODs.
    fn sample_body() -> Vec<u8> {
        let names = ["j_root", "j_spine", "j_head"];
        let mut body = Vec::new();
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&2u32.to_le_bytes());
        for name in names {
            body.extend_from_slice(&u32::from(Murmur32::hash(name.as_bytes())).to_le_bytes());
        }
        for lod in [3u32, 2] {
            body.extend_from_slice(&lod.to_le_bytes());
        }
        for name in names {
            body.extend_from_slice(name.as_bytes());
            body.push(0);
        }
        body
    }

    /// The same body as `wrap_cooked_resource` wraps it.
    fn sample_enveloped() -> Vec<u8> {
        let body = sample_body();
        let mut out = vec![0u8; ENVELOPE];
        out[0..8].copy_from_slice(&BONES_TYPE.to_le_bytes());
        out[8..16].copy_from_slice(&0x0123_4567_89AB_CDEFu64.to_le_bytes());
        out[16] = 1;
        out[29..33].copy_from_slice(&(body.len() as u32).to_le_bytes());
        out[33] = 1;
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn a_bundle_variant_body_round_trips() {
        // Shipped Darktide variants are the bare body.
        let bytes = sample_body();
        let bones = Bones::from_binary(&bytes).expect("the sample parses");
        assert_eq!(bones.names, ["j_root", "j_spine", "j_head"]);
        assert_eq!(bones.lods, [3, 2]);
        assert_eq!(bones.to_binary().unwrap(), bytes);
    }

    #[test]
    fn an_enveloped_blob_is_accepted_and_normalises_to_the_body() {
        let bones = Bones::from_binary(&sample_enveloped()).expect("an enveloped blob parses");
        assert_eq!(bones.names, ["j_root", "j_spine", "j_head"]);
        assert_eq!(bones.to_binary().unwrap(), sample_body());
    }

    #[test]
    fn a_bones_sjson_round_trips() {
        // The build path: SJSON -> cooked bytes -> a bundle file.
        let bones = Bones::from_binary(&sample_body()).unwrap();
        let sjson = bones.to_sjson().unwrap();
        assert_eq!(Bones::from_sjson(&sjson).unwrap(), bones);
        assert!(sjson.contains("j_spine"), "the names are in the SJSON");
    }

    #[test]
    fn a_bone_id_that_does_not_match_its_name_is_rejected() {
        let mut bytes = sample_body();
        // Corrupt the first joint's id (after the two count words).
        bytes[8] ^= 0xFF;
        assert!(Bones::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_truncated_body_is_rejected() {
        // The joint count alone, without any ids.
        assert!(Bones::from_binary(&3u32.to_le_bytes()).is_err());
    }
}
