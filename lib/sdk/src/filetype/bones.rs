//! The `bones` resource: a cooked skeleton - the joint names, their 32-bit ids
//! and the LOD joint counts - inside the 38-byte cooked-resource envelope
//! (`{type hash, name hash, 1, 12 zero bytes, body size, 1, stream size}`).
//!
//! The body is `{u32 joint count, u32 lod count, joint hashes, lod counts,
//! null-terminated names}`. A joint's id is [`Murmur32`] of its name - the upper
//! 32 bits of the seed-zero Murmur64 the engine uses for an `IdString32`.
//!
//! Layout from the Darktide-Asset-Compiler's `bones/bones_writer.cpp`, which
//! writes cooked bones the game loads.

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

/// A cooked skeleton.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bones {
    /// The envelope's resource-name hash, as `#<16 hex digits>`. Written by the
    /// decompiler; on compile it is replaced by the resource path's own hash, so
    /// a hand-authored SJSON may omit it.
    #[serde(default)]
    pub name: String,
    /// The joint names, in order.
    pub names: Vec<String>,
    /// The LOD joint counts; the first is the full joint count.
    pub lods: Vec<u32>,
}

impl Bones {
    /// Reads a bones resource from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < ENVELOPE {
            bail!("a bones resource is shorter than its 38-byte envelope");
        }
        let type_hash = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        if type_hash != BONES_TYPE {
            bail!("not a bones resource (type hash {type_hash:016X})");
        }
        let name_hash = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let body_size = u32::from_le_bytes(bytes[29..33].try_into().unwrap()) as usize;
        let stream_size = u32::from_le_bytes(bytes[34..38].try_into().unwrap()) as usize;
        if bytes.len() != ENVELOPE + body_size + stream_size {
            bail!("the bones envelope's sizes do not match its length");
        }

        let body = &bytes[ENVELOPE..ENVELOPE + body_size];
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

        Ok(Self {
            name: format!("#{name_hash:016x}"),
            names,
            lods,
        })
    }

    /// Writes the cooked bytes back, recomputing each joint's id from its name.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        // An empty name is written as 0; `compile` sets the resource's own hash.
        let name_hash = if self.name.is_empty() {
            0
        } else {
            u64::from_str_radix(self.name.trim_start_matches('#'), 16)
                .map_err(|_| color_eyre::eyre::eyre!("a bones name must be `#<hex>`"))?
        };

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

        let mut out = vec![0u8; ENVELOPE];
        out[0..8].copy_from_slice(&BONES_TYPE.to_le_bytes());
        out[8..16].copy_from_slice(&name_hash.to_le_bytes());
        out[16] = 1;
        out[29..33].copy_from_slice(&(body.len() as u32).to_le_bytes());
        out[33] = 1;
        out.extend_from_slice(&body);
        Ok(out)
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
    let mut bones = Bones::from_sjson(bones_sjson)?;
    // The envelope carries the resource's own name hash, so the SJSON need not
    // repeat it.
    let name_hash: u64 = name.to_murmur64().into();
    bones.name = format!("#{name_hash:016x}");
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
    fn sample() -> Vec<u8> {
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
    fn a_cooked_bones_resource_round_trips() {
        let bytes = sample();
        let bones = Bones::from_binary(&bytes).expect("the sample parses");
        assert_eq!(bones.names, ["j_root", "j_spine", "j_head"]);
        assert_eq!(bones.lods, [3, 2]);
        assert_eq!(bones.name, "#0123456789abcdef");
        assert_eq!(bones.to_binary().unwrap(), bytes);
    }

    #[test]
    fn a_bones_sjson_round_trips() {
        // The build path: SJSON -> cooked bytes -> a bundle file.
        let bones = Bones::from_binary(&sample()).unwrap();
        let sjson = bones.to_sjson().unwrap();
        assert_eq!(Bones::from_sjson(&sjson).unwrap(), bones);
        assert!(sjson.contains("j_spine"), "the names are in the SJSON");
    }

    #[test]
    fn a_bone_id_that_does_not_match_its_name_is_rejected() {
        let mut bytes = sample();
        // Corrupt the first joint's id.
        let at = ENVELOPE + 8;
        bytes[at] ^= 0xFF;
        assert!(Bones::from_binary(&bytes).is_err());
    }
}
