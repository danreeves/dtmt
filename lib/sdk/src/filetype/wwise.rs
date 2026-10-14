//! The Wwise engine-layer resources.
//!
//! DTMT recognises five Wwise types (`wwise_bank`, `wwise_dep`, `wwise_event`,
//! `wwise_metadata`, `wwise_stream`) and decodes none of them, so all ship as
//! opaque blobs - which is already enough to *replace* a bank or a stream, but not
//! to read or edit one.
//!
//! The **dependency** is the smallest of them and is solved here. Its layout comes
//! from the two the boot bundle ships, whose byte counts close exactly:
//!
//! ```text
//! u32 version     29 in every shipped sample - a constant, not a size
//! u32 len         the bank path's length *including* its NUL
//! char[len]       the path, e.g. `wwise/Init`
//! ```
//!
//! so `8 + len` is the resource's size (`Init.wwise_dep` is `29, 11, "wwise/Init\0"`
//! and 19 bytes; `world_sound_fx.wwise_dep` is `29, 21, "wwise/world_sound_fx\0"`
//! and 29). The `wwise_event` resource is **not** modelled here: every one of 606
//! sampled embeds a Wwise bank (`BKHD` after the same `29` version word), so decoding
//! it means decoding the `.bnk` format.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The resource version every shipped dependency carries.
pub const VERSION: u32 = 29;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// One `wwise_dep`: the single bank it depends on.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WwiseDep {
    pub version: u32,
    /// The bank's path, e.g. `wwise/Init`.
    pub bank: String,
}

impl WwiseDep {
    pub fn from_binary(b: &[u8]) -> Result<Self> {
        if b.len() < 8 {
            bail!("a wwise_dep is shorter than its 8-byte header");
        }
        let version = u32_at(b, 0);
        let len = u32_at(b, 4) as usize;
        // The declared length is the whole name field, NUL included, so the resource
        // size follows from it - which is the invariant both shipped files satisfy.
        if 8 + len != b.len() {
            bail!(
                "a wwise_dep declares {len} name bytes but is {} bytes",
                b.len()
            );
        }
        let name = b
            .get(8..8 + len)
            .ok_or_else(|| color_eyre::eyre::eyre!("a wwise_dep's name runs past its body"))?;
        let name = name.strip_suffix(&[0]).unwrap_or(name);
        Ok(Self {
            version,
            bank: String::from_utf8_lossy(name).to_string(),
        })
    }

    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(8 + self.bank.len() + 1);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&((self.bank.len() + 1) as u32).to_le_bytes());
        out.extend_from_slice(self.bank.as_bytes());
        out.push(0);
        Ok(out)
    }

    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

/// Decompiles a `wwise_dep` variant to its SJSON.
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a wwise_dep resource needs a variant");
    };
    let dep = WwiseDep::from_binary(variant.data())?;
    Ok(vec![UserFile::new(dep.to_sjson()?.into_bytes())])
}

/// Compiles a `wwise_dep` SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let dep = WwiseDep::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(dep.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::WwiseDep);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// `world_sound_fx.wwise_dep` exactly as the boot bundle ships it.
    fn shipped() -> Vec<u8> {
        let mut bytes = vec![29, 0, 0, 0, 21, 0, 0, 0];
        bytes.extend_from_slice(b"wwise/world_sound_fx");
        bytes.push(0);
        bytes
    }

    #[test]
    fn a_shipped_dependency_round_trips() {
        let bytes = shipped();
        assert_eq!(bytes.len(), 29, "the shipped file's size");
        let dep = WwiseDep::from_binary(&bytes).expect("the shipped layout parses");
        assert_eq!(dep.version, 29);
        assert_eq!(dep.bank, "wwise/world_sound_fx");
        assert_eq!(dep.to_binary().unwrap(), bytes, "byte-for-byte");
        // And through its SJSON, which is the path a build takes.
        let sjson = dep.to_sjson().unwrap();
        assert_eq!(WwiseDep::from_sjson(&sjson).unwrap(), dep);
    }

    #[test]
    fn a_truncated_dependency_is_rejected() {
        let mut bytes = shipped();
        bytes.pop();
        assert!(WwiseDep::from_binary(&bytes).is_err());
    }
}
