//! The `particles` resource: a compiled particle effect, version 102.
//!
//! This reads/writes the **effect header** - the lifetime, culling distance and
//! flags, the effect variables, and the system list - and keeps each system's
//! bytes opaque (the channel / initializer / simulator / visualizer component
//! model is not decoded yet). The reference is the Darktide-Asset-Compiler's
//! `stingray/particles/particles_resource.cpp`, which decodes every shipped
//! effect exactly.
//!
//! Effect header (offsets in the body):
//!
//! ```text
//! +0x00  u32  version            # 102
//! +0x04  f32  life_time
//! +0x08  f32  culling_distance
//! +0x0C  u32  use_random_seed    # 0 or 1
//! +0x10  u32  flag_10            # 0 or 1
//! +0x14  u32  field_14
//! +0x18  f32  field_18
//! +0x1C  u32  casts_shadows      # 0 or 1
//! +0x20  u32  disable_culling    # 0 or 1
//! +0x24  u32  variable count
//! +0x28  u32  system count
//! +0x2C  u32  variable names     # variable count entries
//!        f32 x3 variable values  # variable count entries
//!        systems                # system count entries, `0x240` bytes each
//! ```
//!
//! Each system carries its own byte length at `+0x23C`; the systems run to the
//! end of the body.

use std::io::Cursor;

use color_eyre::{Report, Result, eyre::bail};

use crate::binary::sync::ReadExt;
use crate::bundle::file::UserFile;
use crate::{BundleFile, BundleFileType, BundleFileVariant};
use crate::murmur::IdString64;

/// The version this reader models.
pub const PARTICLES_VERSION: u32 = 102;

/// The effect header's size before the variable names.
const EFFECT_HEADER: usize = 0x2c;

/// The smallest system header.
const SYSTEM_HEADER: usize = 0x240;

/// One effect variable: a name hash and a three-float value.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    /// The variable's name hash, as `#<8 hex digits>`.
    pub name: String,
    /// Its value.
    pub value: [f32; 3],
}

/// A compiled particle effect, with each system kept opaque.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Particles {
    pub life_time: f32,
    pub culling_distance: f32,
    pub use_random_seed: bool,
    pub flag_10: bool,
    pub field_14: u32,
    pub field_18: f32,
    pub casts_shadows: bool,
    pub disable_culling: bool,
    pub variables: Vec<Variable>,
    /// Each system's bytes as hex (the component model is not decoded yet).
    pub systems: Vec<String>,
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| color_eyre::eyre::eyre!("truncated at byte {at}"))?
            .try_into()
            .unwrap(),
    ))
}

fn f32_at(bytes: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| color_eyre::eyre::eyre!("truncated at byte {at}"))?
            .try_into()
            .unwrap(),
    ))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02X}");
    }
    text
}

fn from_hex(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("a hex string has an odd length");
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(pair).map_err(Report::new)?;
        bytes.push(u8::from_str_radix(pair, 16).map_err(Report::new)?);
    }
    Ok(bytes)
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

impl Particles {
    /// Reads an effect from its cooked body.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        if u32_at(bytes, 0)? != PARTICLES_VERSION {
            bail!("not a version {PARTICLES_VERSION} particles resource");
        }
        for at in [0x0c, 0x10, 0x1c, 0x20] {
            if u32_at(bytes, at)? > 1 {
                bail!("a particles effect header flag at {at:#x} is not 0 or 1");
            }
        }
        let variables = u32_at(bytes, 0x24)? as usize;
        let systems = u32_at(bytes, 0x28)? as usize;

        let mut list = Vec::with_capacity(variables);
        for index in 0..variables {
            let name = u32_at(bytes, EFFECT_HEADER + 4 * index)?;
            let value_at = EFFECT_HEADER + 4 * variables + 12 * index;
            list.push(Variable {
                name: format!("#{name:08x}"),
                value: [
                    f32_at(bytes, value_at)?,
                    f32_at(bytes, value_at + 4)?,
                    f32_at(bytes, value_at + 8)?,
                ],
            });
        }

        let mut system_bytes = Vec::with_capacity(systems);
        let mut at = EFFECT_HEADER + 16 * variables;
        for _ in 0..systems {
            let stride = u32_at(bytes, at + 0x23c)? as usize;
            if stride < SYSTEM_HEADER {
                bail!("a particles system is shorter than its {SYSTEM_HEADER:#x} header");
            }
            let system = bytes
                .get(at..at + stride)
                .ok_or_else(|| color_eyre::eyre::eyre!("a particles system runs past the body"))?;
            system_bytes.push(hex(system));
            at += stride;
        }
        if at != bytes.len() {
            bail!("{} bytes after the last particles system", bytes.len() - at);
        }

        Ok(Self {
            life_time: f32_at(bytes, 0x04)?,
            culling_distance: f32_at(bytes, 0x08)?,
            use_random_seed: u32_at(bytes, 0x0c)? != 0,
            flag_10: u32_at(bytes, 0x10)? != 0,
            field_14: u32_at(bytes, 0x14)?,
            field_18: f32_at(bytes, 0x18)?,
            casts_shadows: u32_at(bytes, 0x1c)? != 0,
            disable_culling: u32_at(bytes, 0x20)? != 0,
            variables: list,
            systems: system_bytes,
        })
    }

    /// Writes the cooked body back.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        push_u32(&mut out, PARTICLES_VERSION);
        out.extend_from_slice(&self.life_time.to_le_bytes());
        out.extend_from_slice(&self.culling_distance.to_le_bytes());
        push_u32(&mut out, self.use_random_seed as u32);
        push_u32(&mut out, self.flag_10 as u32);
        push_u32(&mut out, self.field_14);
        out.extend_from_slice(&self.field_18.to_le_bytes());
        push_u32(&mut out, self.casts_shadows as u32);
        push_u32(&mut out, self.disable_culling as u32);
        push_u32(&mut out, self.variables.len() as u32);
        push_u32(&mut out, self.systems.len() as u32);
        for variable in &self.variables {
            let name = u32::from_str_radix(variable.name.trim_start_matches('#'), 16)
                .map_err(|_| color_eyre::eyre::eyre!("a particles variable name must be `#<hex>`"))?;
            push_u32(&mut out, name);
        }
        for variable in &self.variables {
            for value in variable.value {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for system in &self.systems {
            out.extend_from_slice(&from_hex(system)?);
        }
        Ok(out)
    }

    /// The effect as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(Report::new)
    }

    /// Reads the effect from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        let effect: Self = serde_sjson::from_str(text).map_err(Report::new)?;
        // Round-trip so a malformed system length is caught on load.
        Self::from_binary(&effect.to_binary()?)
    }
}

/// Decompiles a particles variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &crate::Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a particles resource needs a variant");
    };
    let effect = Particles::from_binary(variant.data())?;
    Ok(vec![UserFile::new(effect.to_sjson()?.into_bytes())])
}

/// Compiles a particles SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let effect = Particles::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(effect.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::Particles);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// An effect with one variable and one system.
    fn sample() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&PARTICLES_VERSION.to_le_bytes());
        out.extend_from_slice(&1.5f32.to_le_bytes()); // life_time
        out.extend_from_slice(&20.0f32.to_le_bytes()); // culling_distance
        out.extend_from_slice(&1u32.to_le_bytes()); // use_random_seed
        out.extend_from_slice(&0u32.to_le_bytes()); // flag_10
        out.extend_from_slice(&7u32.to_le_bytes()); // field_14
        out.extend_from_slice(&0.25f32.to_le_bytes()); // field_18
        out.extend_from_slice(&1u32.to_le_bytes()); // casts_shadows
        out.extend_from_slice(&0u32.to_le_bytes()); // disable_culling
        out.extend_from_slice(&1u32.to_le_bytes()); // variable count
        out.extend_from_slice(&1u32.to_le_bytes()); // system count
        out.extend_from_slice(&0xdead_beefu32.to_le_bytes()); // variable name
        for value in [0.1f32, 0.2, 0.3] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        // One system: `0x240` bytes with its stride at +0x23c.
        let mut system = vec![0u8; SYSTEM_HEADER];
        system[0x23c..0x240].copy_from_slice(&(SYSTEM_HEADER as u32).to_le_bytes());
        out.extend_from_slice(&system);
        out
    }

    #[test]
    fn a_particles_effect_round_trips() {
        let bytes = sample();
        let effect = Particles::from_binary(&bytes).expect("the sample parses");
        assert_eq!(effect.life_time, 1.5);
        assert_eq!(effect.variables.len(), 1);
        assert_eq!(effect.variables[0].name, "#deadbeef");
        assert_eq!(effect.variables[0].value, [0.1, 0.2, 0.3]);
        assert_eq!(effect.systems.len(), 1);
        assert_eq!(effect.to_binary().unwrap(), bytes);
    }

    #[test]
    fn a_particles_sjson_round_trips() {
        let effect = Particles::from_binary(&sample()).unwrap();
        let sjson = effect.to_sjson().unwrap();
        assert_eq!(Particles::from_sjson(&sjson).unwrap(), effect);
    }

    #[test]
    fn a_wrong_version_or_trailing_bytes_is_rejected() {
        let mut wrong = sample();
        wrong[0] = 99;
        assert!(Particles::from_binary(&wrong).is_err());

        let mut trailing = sample();
        trailing.push(0);
        assert!(Particles::from_binary(&trailing).is_err());
    }
}
