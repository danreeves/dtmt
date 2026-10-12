//! The `particles` resource: a compiled particle effect, version 102.
//!
//! This reads/writes the **effect header** - the lifetime, culling distance and
//! flags, the effect variables - and each **system header** (its name, capacity,
//! channel layout and the initializer / simulator / visualizer counts), keeping
//! the component *bodies* opaque (the channel / initializer / simulator /
//! visualizer model is not decoded yet). The reference is the
//! Darktide-Asset-Compiler's `stingray/particles/particles_resource.cpp`, which
//! decodes every shipped effect exactly.
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
//!        systems                # system count entries
//! ```
//!
//! System header (offsets within a system; the whole system is `end` bytes):
//!
//! ```text
//! +0x00  u32  name hash
//! +0x04  u32  capacity
//! +0x08  f32  min_capacity_scaling
//! +0x0C  u32  channel count      # at most 16
//! +0x10  u32  channel stride
//! +0x14  u32  channel size x16   # the unused slots are 0
//! +0x54  u32  0
//! +0x58        header tail       # to 0x214, opaque
//! +0x214 u32  initializer count
//! +0x218 u32  initializers at    # 0x240
//! +0x21C u32  simulator count
//! +0x220 u32  simulator records at
//! +0x224 u32  simulator data at
//! +0x228 u32  unknown
//! +0x22C u32  visualizer count
//! +0x230 u32  visualizers at
//! +0x234 u32  unknown
//! +0x238 u32  unknown
//! +0x23C u32  end (the system's length)
//! ```

use color_eyre::{Report, Result, eyre::bail};

use crate::bundle::file::UserFile;
use crate::murmur::IdString64;
use crate::{BundleFile, BundleFileType, BundleFileVariant};

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

/// One particle system: its header read, its component bodies partly kept as hex.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct System {
    /// The system's name hash, as `#<8 hex digits>`.
    pub name: String,
    pub capacity: u32,
    pub min_capacity_scaling: f32,
    /// The channel sizes, one per used channel.
    pub channels: Vec<u32>,
    pub channel_stride: u32,
    /// The header bytes `0x58..0x214`, hex.
    pub header: String,
    /// The initializer list.
    pub initializers: Vec<Initializer>,
    /// Any bytes between the initializers' end and the simulator records, hex.
    pub initializer_padding: String,
    pub simulators: u32,
    pub visualizers: u32,
    /// The unknown words at `0x228`/`0x234`/`0x238`, hex.
    pub unknown: String,
    /// The simulator records, hex.
    pub simulator_records: String,
    /// The simulator data, hex.
    pub simulator_data: String,
    /// The visualizer block, hex.
    pub visualizer_data: String,
}

/// One initializer: its op name and its operand bytes (kept opaque; the field
/// layouts are not modelled yet).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initializer {
    #[serde(rename = "type")]
    pub kind: String,
    /// The operand bytes, hex.
    pub data: String,
}

/// The initializer op codes, in registry order (the community's
/// `initializer_specs`). Op `0x0d`/`0x0e`/`0x11` are variable length.
const INITIALIZERS: [&str; 25] = [
    "zero",
    "random_float",
    "velocity_cone",
    "position_sphere",
    "position_box",
    "random_int",
    "zero_velocity",
    "copy",
    "velocity_box",
    "normal_box",
    "tangent_box",
    "position_cylinder",
    "velocity_cylinder",
    "position_mesh",
    "position_mesh_skinned",
    "float",
    "vector",
    "vector_language_program",
    "leap_number",
    "initializer_13",
    "spline_control_points",
    "multiply_by_variable",
    "rotation_align_to_local",
    "runtime_position_mesh",
    "initializer_18",
];

/// The fixed operand size of each initializer op (the variable ones are
/// `0x0d`/`0x0e`/`0x11`, handled in [`initializer_size`]).
const INITIALIZER_SIZES: [usize; 25] = [
    8, 12, 32, 12, 28, 12, 4, 12, 28, 32, 28, 40, 32, 0, 0, 8, 16, 0, 4, 4, 64, 12, 8, 20, 4,
];

/// A compiled particle effect.
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
    pub systems: Vec<System>,
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

/// Reads a name hash written as `#<8 hex digits>`.
fn name_hash(text: &str) -> Result<u32> {
    u32::from_str_radix(text.trim_start_matches('#'), 16)
        .map_err(|_| color_eyre::eyre::eyre!("a particles name must be `#<8 hex>`"))
}

/// The operand size of an initializer, from its op (the variable ops read a
/// count out of the operand).
fn initializer_size(op: usize, bytes: &[u8], at: usize) -> Result<usize> {
    match op {
        0x0d => Ok(0x18 + 24 * u32_at(bytes, at + 0x14)? as usize),
        0x0e => Ok(u32_at(bytes, at + 0x2c)? as usize),
        0x11 => Ok(0x48 + u32_at(bytes, at + 0x44)? as usize),
        _ => INITIALIZER_SIZES
            .get(op)
            .copied()
            .filter(|size| *size > 0)
            .ok_or_else(|| color_eyre::eyre::eyre!("unknown initializer op {op}")),
    }
}

impl System {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < SYSTEM_HEADER {
            bail!("a particles system is shorter than its {SYSTEM_HEADER:#x} header");
        }
        let channels = u32_at(bytes, 0x0c)? as usize;
        if channels > 16 {
            bail!("a particles system has {channels} channels");
        }
        let mut sizes = Vec::with_capacity(channels);
        for index in 0..16 {
            let size = u32_at(bytes, 0x14 + 4 * index)?;
            if index < channels {
                sizes.push(size);
            } else if size != 0 {
                bail!("a particles system's unused channel slot {index} is not zero");
            }
        }
        if u32_at(bytes, 0x54)? != 0 {
            bail!("a particles system's header word 0x54 is set");
        }

        let initializers_count = u32_at(bytes, 0x214)?;
        let initializer_at = u32_at(bytes, 0x218)? as usize;
        let simulators = u32_at(bytes, 0x21c)?;
        let records = u32_at(bytes, 0x220)? as usize;
        let data = u32_at(bytes, 0x224)? as usize;
        let visualizers = u32_at(bytes, 0x22c)?;
        let visualizer_at = u32_at(bytes, 0x230)? as usize;
        let end = u32_at(bytes, 0x23c)? as usize;
        if initializer_at != SYSTEM_HEADER {
            bail!("a particles system's initializers do not follow its header");
        }
        if end != bytes.len() {
            bail!("a particles system's end is not its length");
        }
        if !(SYSTEM_HEADER <= records && records <= data && data <= visualizer_at && visualizer_at <= end)
        {
            bail!("a particles system's component offsets are out of order");
        }
        let slice = |from: usize, to: usize| hex(&bytes[from..to]);
        let mut unknown = Vec::with_capacity(12);
        for at in [0x228, 0x234, 0x238] {
            unknown.extend_from_slice(&bytes[at..at + 4]);
        }

        let mut initializers = Vec::with_capacity(initializers_count as usize);
        let mut at = initializer_at;
        for _ in 0..initializers_count {
            let op = u32_at(bytes, at)? as usize;
            let size = initializer_size(op, bytes, at + 4)?;
            let operand = bytes
                .get(at + 4..at + 4 + size)
                .ok_or_else(|| color_eyre::eyre::eyre!("an initializer's operands run past the body"))?;
            initializers.push(Initializer {
                kind: INITIALIZERS
                    .get(op)
                    .ok_or_else(|| color_eyre::eyre::eyre!("unknown initializer op {op}"))?
                    .to_string(),
                data: hex(operand),
            });
            at += 4 + size;
        }
        if at > records {
            bail!("a particles system's initializers run past its simulator records");
        }
        let initializer_padding = hex(&bytes[at..records]);

        Ok(Self {
            name: format!("#{:08x}", u32_at(bytes, 0)?),
            capacity: u32_at(bytes, 4)?,
            min_capacity_scaling: f32_at(bytes, 8)?,
            channels: sizes,
            channel_stride: u32_at(bytes, 0x10)?,
            header: slice(0x58, 0x214),
            initializers,
            initializer_padding,
            simulators,
            visualizers,
            unknown: hex(&unknown),
            simulator_records: slice(records, data),
            simulator_data: slice(data, visualizer_at),
            visualizer_data: slice(visualizer_at, end),
        })
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        let simulator_records = from_hex(&self.simulator_records)?;
        let simulator_data = from_hex(&self.simulator_data)?;
        let visualizer_data = from_hex(&self.visualizer_data)?;
        let header = from_hex(&self.header)?;
        if header.len() != 0x214 - 0x58 {
            bail!("a particles system's header is not {} bytes", 0x214 - 0x58);
        }
        if self.channels.len() > 16 {
            bail!("a particles system has too many channels");
        }
        if simulator_records.len() != 5 * self.simulators as usize {
            bail!("a particles system's simulator records are not five bytes each");
        }
        let mut initializer_bytes = Vec::new();
        for initializer in &self.initializers {
            let op = INITIALIZERS
                .iter()
                .position(|name| *name == initializer.kind)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("unknown initializer type '{}'", initializer.kind)
                })?;
            push_u32(&mut initializer_bytes, op as u32);
            initializer_bytes.extend_from_slice(&from_hex(&initializer.data)?);
        }

        let records = SYSTEM_HEADER + initializer_bytes.len() + from_hex(&self.initializer_padding)?.len();
        let data = records + simulator_records.len();
        let visualizer_at = data + simulator_data.len();
        let end = visualizer_at + visualizer_data.len();

        let mut out = vec![0u8; SYSTEM_HEADER];
        out[0..4].copy_from_slice(&name_hash(&self.name)?.to_le_bytes());
        out[4..8].copy_from_slice(&self.capacity.to_le_bytes());
        out[8..12].copy_from_slice(&self.min_capacity_scaling.to_le_bytes());
        out[0x0c..0x10].copy_from_slice(&(self.channels.len() as u32).to_le_bytes());
        out[0x10..0x14].copy_from_slice(&self.channel_stride.to_le_bytes());
        for (index, size) in self.channels.iter().enumerate() {
            out[0x14 + 4 * index..0x18 + 4 * index].copy_from_slice(&size.to_le_bytes());
        }
        out[0x58..0x214].copy_from_slice(&header);
        out[0x214..0x218].copy_from_slice(&(self.initializers.len() as u32).to_le_bytes());
        out[0x218..0x21c].copy_from_slice(&(SYSTEM_HEADER as u32).to_le_bytes());
        out[0x21c..0x220].copy_from_slice(&self.simulators.to_le_bytes());
        out[0x220..0x224].copy_from_slice(&(records as u32).to_le_bytes());
        out[0x224..0x228].copy_from_slice(&(data as u32).to_le_bytes());
        out[0x22c..0x230].copy_from_slice(&self.visualizers.to_le_bytes());
        out[0x230..0x234].copy_from_slice(&(visualizer_at as u32).to_le_bytes());
        out[0x23c..0x240].copy_from_slice(&(end as u32).to_le_bytes());
        let unknown = from_hex(&self.unknown)?;
        if unknown.len() != 12 {
            bail!("a particles system's unknown words are not twelve bytes");
        }
        for (index, at) in [0x228usize, 0x234, 0x238].into_iter().enumerate() {
            out[at..at + 4].copy_from_slice(&unknown[index * 4..index * 4 + 4]);
        }

        out.extend_from_slice(&initializer_bytes);
        out.extend_from_slice(&from_hex(&self.initializer_padding)?);
        out.extend_from_slice(&simulator_records);
        out.extend_from_slice(&simulator_data);
        out.extend_from_slice(&visualizer_data);
        Ok(out)
    }
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

        let mut system_list = Vec::with_capacity(systems);
        let mut at = EFFECT_HEADER + 16 * variables;
        for _ in 0..systems {
            let stride = u32_at(bytes, at + 0x23c)? as usize;
            let system = bytes
                .get(at..at + stride)
                .ok_or_else(|| color_eyre::eyre::eyre!("a particles system runs past the body"))?;
            system_list.push(System::from_bytes(system)?);
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
            systems: system_list,
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
            push_u32(&mut out, name_hash(&variable.name)?);
        }
        for variable in &self.variables {
            for value in variable.value {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        for system in &self.systems {
            out.extend_from_slice(&system.to_bytes()?);
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
        // Round-trip so a malformed region is caught on load.
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

    /// One system: two channels, one `random_float` initializer.
    fn system(name: u32) -> Vec<u8> {
        let mut out = vec![0u8; SYSTEM_HEADER];
        out[0..4].copy_from_slice(&name.to_le_bytes());
        out[4..8].copy_from_slice(&64u32.to_le_bytes()); // capacity
        out[8..12].copy_from_slice(&0.5f32.to_le_bytes()); // min_capacity_scaling
        out[0x0c..0x10].copy_from_slice(&2u32.to_le_bytes()); // channel count
        out[0x10..0x14].copy_from_slice(&16u32.to_le_bytes()); // channel stride
        out[0x14..0x18].copy_from_slice(&4u32.to_le_bytes()); // channel 0 size
        out[0x18..0x1c].copy_from_slice(&12u32.to_le_bytes()); // channel 1 size

        // One `random_float` initializer (op 1, 12 operand bytes).
        let mut initializer = 1u32.to_le_bytes().to_vec();
        initializer.extend_from_slice(&[0u8; 12]);
        let records = SYSTEM_HEADER + initializer.len();
        out[0x214..0x218].copy_from_slice(&1u32.to_le_bytes()); // initializer count
        out[0x218..0x21c].copy_from_slice(&(SYSTEM_HEADER as u32).to_le_bytes());
        out[0x220..0x224].copy_from_slice(&(records as u32).to_le_bytes());
        out[0x224..0x228].copy_from_slice(&(records as u32).to_le_bytes());
        out[0x230..0x234].copy_from_slice(&(records as u32).to_le_bytes());
        out[0x23c..0x240].copy_from_slice(&(records as u32).to_le_bytes());
        out.extend_from_slice(&initializer);
        out
    }

    /// An effect with one variable and one system.
    fn sample() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&PARTICLES_VERSION.to_le_bytes());
        out.extend_from_slice(&1.5f32.to_le_bytes());
        out.extend_from_slice(&20.0f32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&7u32.to_le_bytes());
        out.extend_from_slice(&0.25f32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // variable count
        out.extend_from_slice(&1u32.to_le_bytes()); // system count
        out.extend_from_slice(&0xdead_beefu32.to_le_bytes());
        for value in [0.1f32, 0.2, 0.3] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&system(0x1234_5678));
        out
    }

    #[test]
    fn a_particles_effect_round_trips() {
        let bytes = sample();
        let effect = Particles::from_binary(&bytes).expect("the sample parses");
        assert_eq!(effect.life_time, 1.5);
        assert_eq!(effect.variables[0].name, "#deadbeef");
        assert_eq!(effect.systems[0].name, "#12345678");
        assert_eq!(effect.systems[0].capacity, 64);
        assert_eq!(effect.systems[0].channels, [4, 12]);
        assert_eq!(effect.systems[0].channel_stride, 16);
        assert_eq!(effect.systems[0].initializers.len(), 1);
        assert_eq!(effect.systems[0].initializers[0].kind, "random_float");
        assert_eq!(effect.systems[0].initializers[0].data, "000000000000000000000000");
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
