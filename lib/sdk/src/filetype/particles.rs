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
    /// The simulator list.
    pub simulators: Vec<Simulator>,
    /// The visualizer list.
    pub visualizers: Vec<Visualizer>,
    /// The unknown words at `0x228`/`0x234`/`0x238`, hex.
    pub unknown: String,
    /// The simulator data block, hex.
    pub simulator_data: String,
}

/// One initializer: its op name, its operand bytes and a readable view of them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initializer {
    #[serde(rename = "type")]
    pub kind: String,
    /// The operand bytes, hex. Authoritative: the compiler writes these back
    /// verbatim, so `decoded` can never disturb the round-trip.
    pub data: String,
    /// The operand's fields by the reference's names (`min`, `max`, `radius_min`,
    /// ...), typed so an authored value can be written back. `data` stays
    /// authoritative - the compiler writes it verbatim - so this is descriptive for
    /// now, and absent for the three variable-length ops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<indexmap::IndexMap<String, FieldValue>>,
}

/// One decoded initializer field. Untagged, so it writes as a bare SJSON value: a
/// number, or a list for a vector.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    U32(u32),
    I32(i32),
    F32(f32),
    Vec3([f32; 3]),
}

/// One simulator: its op name and the offset of its data in the system's data
/// block (the data itself is kept opaque).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Simulator {
    #[serde(rename = "type")]
    pub kind: String,
    pub offset: u32,
}

/// One visualizer: its type name and its record bytes (kept opaque; the
/// per-type field layouts are not modelled yet).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visualizer {
    #[serde(rename = "type")]
    pub kind: String,
    /// The record bytes after the type word, hex.
    pub data: String,
}

/// The visualizer type names, in the community's `kVisualizers` order plus gpu.
const VISUALIZERS: [&str; 5] = ["billboard", "light", "mesh", "ribbon", "gpu"];

/// The payload offset of each visualizer type's byte-size word.
const VISUALIZER_SIZE_AT: [usize; 5] = [0xf4, 0xf8, 0xf8, 0xf0, 0xa0];

/// The simulator op codes, in registry order (the community's
/// `simulator_specs`).
const SIMULATORS: [&str; 50] = [
    "age_age",
    "position_integrate",
    "position_integrate_scaled",
    "velocity_accelerate",
    "advance_frame",
    "integrate_float_scaled",
    "plane_collision",
    "rate_spawn",
    "trail_spawn",
    "collision_spawn",
    "wind",
    "warp_box",
    "rate_emitter",
    "burst_emitter",
    "grid_collision",
    "fast_forward_static",
    "debug_draw",
    "local_space",
    "ray",
    "copy_variable_to_float",
    "scale_float",
    "spiral",
    "query_collision",
    "collision_resolution",
    "integrate_float",
    "query_vector_field",
    "air_resistance",
    "vector_language_program",
    "spline_control_points",
    "copy_vector3",
    "write_float",
    "write_int",
    "size",
    "color",
    "copy_float",
    "rotate_by_velocity",
    "copy_float_and_floor",
    "distance_fade",
    "opacity_kill",
    "stretch_by_velocity",
    "rotation_face_velocity",
    "rotation_align_to_velocity",
    "uv_tile",
    "hdr_color",
    "rotation_3d_from_2d",
    "tangent_to_next",
    "snap_to_translation",
    "local_age",
    "rotation_3d",
    "scale_3d",
];

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
/// The operand fields of one initializer, from the community compiler's
/// declarative spec (`particles_resource.cpp`'s `initializer_specs`): a channel
/// reference, a scalar, a three-float vector, or a fixed byte block. The names
/// are the reference's, and the *order and widths* are checked against
/// `INITIALIZER_SIZES` - the table the reader already uses and the shipped
/// round-trips already validate - by the test below.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InitializerField {
    /// A channel reference, `CH` in the reference.
    Channel,
    /// An unsigned `u32`.
    U32,
    /// A signed `i32`.
    I32,
    /// A float.
    F32,
    /// Three floats.
    Vec3,
    /// A fixed block of bytes.
    Bytes(usize),
}

impl InitializerField {
    fn size(self) -> usize {
        match self {
            Self::Channel | Self::U32 | Self::I32 | Self::F32 => 4,
            Self::Vec3 => 12,
            Self::Bytes(size) => size,
        }
    }
}

/// The initializer operand layouts in opcode order. The three variable-length
/// ops (`position_mesh`, `position_mesh_skinned`, `vector_language_program`) are
/// empty, since the reader sizes those from their own contents.
pub const INITIALIZER_LAYOUTS: &[(&str, &[InitializerField])] = &[
    ("zero", &[InitializerField::Channel, InitializerField::U32]),
    (
        "random_float",
        &[InitializerField::Channel, InitializerField::F32, InitializerField::F32],
    ),
    (
        "velocity_cone",
        &[
            InitializerField::Channel,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::Vec3,
        ],
    ),
    (
        "position_sphere",
        &[InitializerField::Channel, InitializerField::F32, InitializerField::F32],
    ),
    (
        "position_box",
        &[InitializerField::Channel, InitializerField::Vec3, InitializerField::Vec3],
    ),
    (
        "random_int",
        &[InitializerField::Channel, InitializerField::I32, InitializerField::I32],
    ),
    ("zero_velocity", &[InitializerField::Channel]),
    (
        "copy",
        &[InitializerField::Channel, InitializerField::Channel, InitializerField::U32],
    ),
    (
        "velocity_box",
        &[InitializerField::Channel, InitializerField::Vec3, InitializerField::Vec3],
    ),
    (
        "normal_box",
        &[
            InitializerField::Channel,
            InitializerField::Channel,
            InitializerField::Vec3,
            InitializerField::Vec3,
        ],
    ),
    (
        "tangent_box",
        &[InitializerField::Channel, InitializerField::Vec3, InitializerField::Vec3],
    ),
    (
        "position_cylinder",
        &[
            InitializerField::Channel,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::I32,
            InitializerField::I32,
            InitializerField::I32,
        ],
    ),
    (
        "velocity_cylinder",
        &[
            InitializerField::Channel,
            InitializerField::Channel,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
        ],
    ),
    ("position_mesh", &[]),
    ("position_mesh_skinned", &[]),
    ("float", &[InitializerField::Channel, InitializerField::F32]),
    ("vector", &[InitializerField::Channel, InitializerField::Vec3]),
    ("vector_language_program", &[]),
    ("leap_number", &[InitializerField::Channel]),
    ("initializer_13", &[InitializerField::Channel]),
    ("spline_control_points", &[InitializerField::Bytes(0x40)]),
    (
        "multiply_by_variable",
        &[
            InitializerField::Channel,
            InitializerField::U32,
            InitializerField::U32,
        ],
    ),
    (
        "rotation_align_to_local",
        &[InitializerField::Channel, InitializerField::Channel],
    ),
    (
        "runtime_position_mesh",
        &[
            InitializerField::Channel,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
            InitializerField::F32,
        ],
    ),
    ("initializer_18", &[InitializerField::Channel]),
];

/// The reference's field *names* for each initializer, in the same op order and
/// the same field order as `INITIALIZER_LAYOUTS`. Kept separate so the layouts stay
/// the width authority; the test below pins the two together, so a name list
/// cannot drift out of step with the fields it names.
pub const INITIALIZER_FIELD_NAMES: &[&[&str]] = &[
    &["channel", "size"],
    &["channel", "min", "max"],
    &[
        "velocity",
        "speed_min",
        "speed_max",
        "theta_min",
        "theta_max",
        "direction",
    ],
    &["position", "radius_min", "radius_max"],
    &["position", "min", "max"],
    &["channel", "min", "max"],
    &["velocity"],
    &["source", "destination", "size"],
    &["velocity", "min", "max"],
    &["tangent", "binormal", "min", "max"],
    &["tangent", "min", "max"],
    &[
        "position",
        "radius_min",
        "radius_max",
        "height_min",
        "height_max",
        "angle_min",
        "angle_max",
        "radius_variable",
        "height_variable",
        "angle_variable",
    ],
    &[
        "position",
        "velocity",
        "radial_min",
        "radial_max",
        "z_min",
        "z_max",
        "angular_min",
        "angular_max",
    ],
    &[],
    &[],
    &["channel", "value"],
    &["channel", "value"],
    &[],
    &["leap_number"],
    &["channel"],
    &["data"],
    &["channel", "channel_size", "variable"],
    &["tangent", "binormal"],
    &["position", "radius_min", "radius_max", "height_min", "height_max"],
    &["channel"],
];

/// A readable view of one initializer's operands, from the layout the reference
/// gives that op: one `"kind = value"` per field. `data` stays authoritative, so
/// this is descriptive only, and the three variable-length ops have no layout to
/// read.
fn describe_initializer(
    op: usize,
    operand: &[u8],
) -> Option<indexmap::IndexMap<String, FieldValue>> {
    let (_, layout) = INITIALIZER_LAYOUTS.get(op)?;
    let mut out = indexmap::IndexMap::with_capacity(layout.len());
    let mut at = 0usize;
    for (index, field) in layout.iter().enumerate() {
        let width = field.size();
        let Some(bytes) = operand.get(at..at + width) else {
            break;
        };
        let (kind_label, value) = match field {
            InitializerField::Channel => (
                "channel",
                FieldValue::U32(u32::from_le_bytes(bytes.try_into().unwrap())),
            ),
            InitializerField::U32 => (
                "u32",
                FieldValue::U32(u32::from_le_bytes(bytes.try_into().unwrap())),
            ),
            InitializerField::I32 => (
                "i32",
                FieldValue::I32(i32::from_le_bytes(bytes.try_into().unwrap())),
            ),
            InitializerField::F32 => (
                "f32",
                FieldValue::F32(f32::from_le_bytes(bytes.try_into().unwrap())),
            ),
            InitializerField::Vec3 => (
                "vec3",
                FieldValue::Vec3([
                    f32::from_le_bytes(bytes[0..4].try_into().unwrap()),
                    f32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                    f32::from_le_bytes(bytes[8..12].try_into().unwrap()),
                ]),
            ),
            InitializerField::Bytes(_) => ("bytes", FieldValue::U32(width as u32)),
        };
        // The reference's field name when it has one, the kind otherwise.
        let label = INITIALIZER_FIELD_NAMES
            .get(op)
            .and_then(|names| names.get(index))
            .copied()
            .unwrap_or(kind_label);
        out.insert(label.to_string(), value);
        at += width;
    }
    Some(out)
}

/// Rebuild an initializer's operands from its typed fields, using the op's layout
/// for the field order and widths. An integer-valued `F32` field is accepted,
/// because the untagged form reads `2` back as the integer variant; a value that
/// does not fit its field is an error rather than a silent truncation. `None` means
/// the op cannot be rebuilt this way - it has no layout, or holds an opaque byte
/// block whose contents the view does not carry - and the caller keeps the authored
/// `data` instead.
fn encode_initializer(
    op: usize,
    fields: &indexmap::IndexMap<String, FieldValue>,
) -> Result<Option<Vec<u8>>> {
    let Some((_, layout)) = INITIALIZER_LAYOUTS.get(op) else {
        return Ok(None);
    };
    if layout
        .iter()
        .any(|field| matches!(field, InitializerField::Bytes(_)))
    {
        return Ok(None);
    }
    let names = INITIALIZER_FIELD_NAMES.get(op);
    let mut out = Vec::with_capacity(layout.iter().map(|field| field.size()).sum());
    for (index, field) in layout.iter().enumerate() {
        let name = names
            .and_then(|names| names.get(index))
            .copied()
            .unwrap_or("field");
        let value = fields.get(name).ok_or_else(|| {
            color_eyre::eyre::eyre!("initializer op {op} is missing its '{name}' field")
        })?;
        match field {
            InitializerField::Channel | InitializerField::U32 => {
                out.extend_from_slice(&as_u32(*value, op, name)?.to_le_bytes())
            }
            InitializerField::I32 => {
                out.extend_from_slice(&as_i32(*value, op, name)?.to_le_bytes())
            }
            InitializerField::F32 => {
                out.extend_from_slice(&as_f32(*value, op, name)?.to_le_bytes())
            }
            InitializerField::Vec3 => {
                let FieldValue::Vec3(values) = value else {
                    bail!("initializer op {op} field '{name}' is not a vector");
                };
                for component in values {
                    out.extend_from_slice(&component.to_le_bytes());
                }
            }
            InitializerField::Bytes(_) => unreachable!("handled above"),
        }
    }
    Ok(Some(out))
}

fn as_u32(value: FieldValue, op: usize, name: &str) -> Result<u32> {
    match value {
        FieldValue::U32(value) => Ok(value),
        FieldValue::F32(value) if value >= 0.0 && value.fract() == 0.0 => Ok(value as u32),
        other => bail!("initializer op {op} field '{name}' is not a u32: {other:?}"),
    }
}

fn as_i32(value: FieldValue, op: usize, name: &str) -> Result<i32> {
    match value {
        FieldValue::I32(value) => Ok(value),
        FieldValue::U32(value) if value <= i32::MAX as u32 => Ok(value as i32),
        other => bail!("initializer op {op} field '{name}' is not an i32: {other:?}"),
    }
}

fn as_f32(value: FieldValue, op: usize, name: &str) -> Result<f32> {
    match value {
        // An integer-valued float writes as an integer, so accept both.
        FieldValue::F32(value) => Ok(value),
        FieldValue::U32(value) => Ok(value as f32),
        FieldValue::I32(value) => Ok(value as f32),
        other => bail!("initializer op {op} field '{name}' is not a float: {other:?}"),
    }
}

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
        let simulators_count = u32_at(bytes, 0x21c)?;
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
                fields: describe_initializer(op, operand),
            });
            at += 4 + size;
        }
        if at > records {
            bail!("a particles system's initializers run past its simulator records");
        }
        let initializer_padding = hex(&bytes[at..records]);

        if records + 5 * simulators_count as usize != data {
            bail!("a particles system's simulator records do not end at its data block");
        }
        let mut simulator_list = Vec::with_capacity(simulators_count as usize);
        for index in 0..simulators_count as usize {
            let op = *bytes
                .get(records + 5 * index)
                .ok_or_else(|| color_eyre::eyre::eyre!("a simulator record is short"))?
                as usize;
            let offset = u32::from_le_bytes(
                bytes
                    .get(records + 5 * index + 1..records + 5 * index + 5)
                    .ok_or_else(|| color_eyre::eyre::eyre!("a simulator record is short"))?
                    .try_into()
                    .unwrap(),
            );
            simulator_list.push(Simulator {
                kind: SIMULATORS
                    .get(op)
                    .ok_or_else(|| color_eyre::eyre::eyre!("unknown simulator op {op}"))?
                    .to_string(),
                offset,
            });
        }

        let mut visualizer_list = Vec::with_capacity(visualizers as usize);
        let mut vis = visualizer_at;
        for _ in 0..visualizers {
            let kind = u32_at(bytes, vis)? as usize;
            if kind > 4 {
                bail!("a particles system has an unknown visualizer type {kind}");
            }
            let size = u32_at(bytes, vis + 4 + VISUALIZER_SIZE_AT[kind])? as usize;
            let body = bytes
                .get(vis + 4..vis + 4 + size)
                .ok_or_else(|| color_eyre::eyre::eyre!("a visualizer runs past the system"))?;
            visualizer_list.push(Visualizer {
                kind: VISUALIZERS[kind].to_string(),
                data: hex(body),
            });
            vis += 4 + size;
        }
        if vis != end {
            bail!("a particles system's visualizers do not end at its end");
        }

        Ok(Self {
            name: format!("#{:08x}", u32_at(bytes, 0)?),
            capacity: u32_at(bytes, 4)?,
            min_capacity_scaling: f32_at(bytes, 8)?,
            channels: sizes,
            channel_stride: u32_at(bytes, 0x10)?,
            header: slice(0x58, 0x214),
            initializers,
            initializer_padding,
            simulators: simulator_list,
            visualizers: visualizer_list,
            unknown: hex(&unknown),
            simulator_data: slice(data, visualizer_at),
        })
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        let simulator_data = from_hex(&self.simulator_data)?;
        let mut visualizer_bytes = Vec::new();
        for visualizer in &self.visualizers {
            let kind = VISUALIZERS
                .iter()
                .position(|name| *name == visualizer.kind)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("unknown visualizer type '{}'", visualizer.kind)
                })?;
            push_u32(&mut visualizer_bytes, kind as u32);
            visualizer_bytes.extend_from_slice(&from_hex(&visualizer.data)?);
        }
        let header = from_hex(&self.header)?;
        if header.len() != 0x214 - 0x58 {
            bail!("a particles system's header is not {} bytes", 0x214 - 0x58);
        }
        if self.channels.len() > 16 {
            bail!("a particles system has too many channels");
        }
        let mut simulator_records = Vec::with_capacity(self.simulators.len() * 5);
        for simulator in &self.simulators {
            let op = SIMULATORS
                .iter()
                .position(|name| *name == simulator.kind)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("unknown simulator type '{}'", simulator.kind)
                })?;
            simulator_records.push(op as u8);
            simulator_records.extend_from_slice(&simulator.offset.to_le_bytes());
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
            // A typed field view rebuilds the operands; for an opaque byte block,
            // or an op with no layout, the authored `data` stays authoritative.
            let encoded = match &initializer.fields {
                Some(fields) => encode_initializer(op, fields)?,
                None => None,
            };
            initializer_bytes.extend_from_slice(&match encoded {
                Some(bytes) => bytes,
                None => from_hex(&initializer.data)?,
            });
        }

        let records = SYSTEM_HEADER + initializer_bytes.len() + from_hex(&self.initializer_padding)?.len();
        let data = records + simulator_records.len();
        let visualizer_at = data + simulator_data.len();
        let end = visualizer_at + visualizer_bytes.len();

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
        out[0x21c..0x220].copy_from_slice(&(self.simulators.len() as u32).to_le_bytes());
        out[0x220..0x224].copy_from_slice(&(records as u32).to_le_bytes());
        out[0x224..0x228].copy_from_slice(&(data as u32).to_le_bytes());
        out[0x22c..0x230].copy_from_slice(&(self.visualizers.len() as u32).to_le_bytes());
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
        out.extend_from_slice(&visualizer_bytes);
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

    /// Every fixed layout rebuilds byte for byte from its typed view, which is what
    /// lets the compiler trust `fields` rather than only `data`.
    #[test]
    fn initializer_fields_rebuild_their_operands() {
        for op in 0..INITIALIZER_SIZES.len() {
            let (name, layout) = INITIALIZER_LAYOUTS[op];
            if layout.is_empty()
                || layout
                    .iter()
                    .any(|field| matches!(field, InitializerField::Bytes(_)))
            {
                continue;
            }
            // Distinctive bytes, so a field written in the wrong place shows up.
            let size: usize = layout.iter().map(|field| field.size()).sum();
            let operand: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
            let fields = describe_initializer(op, &operand)
                .unwrap_or_else(|| panic!("op {op} ({name}) has a layout, so it has a view"));
            let rebuilt = encode_initializer(op, &fields)
                .expect("the view encodes")
                .unwrap_or_else(|| panic!("op {op} ({name}) has a fixed layout"));
            assert_eq!(rebuilt, operand, "op {op} ({name})");
        }
    }

    /// The ported initializer layouts must agree with `INITIALIZER_SIZES` - the
    /// table the reader walks and the byte-identical shipped round-trips already
    /// validate - so this pins the field *order and widths* the reference gives,
    /// independently of the field *names*.
    #[test]
    fn initializer_layouts_match_their_sizes() {
        assert_eq!(INITIALIZER_LAYOUTS.len(), INITIALIZER_SIZES.len());
        for (op, (name, fields)) in INITIALIZER_LAYOUTS.iter().enumerate() {
            let sum: usize = fields.iter().map(|field| field.size()).sum();
            assert_eq!(
                sum, INITIALIZER_SIZES[op],
                "op {op} ({name}) layout sums to {sum}"
            );
            // Only the three variable-length ops have an empty layout, and they
            // are exactly the ones the size table marks with a zero.
            assert_eq!(
                fields.is_empty(),
                INITIALIZER_SIZES[op] == 0,
                "op {op} ({name})"
            );
            // The names are the reference's, one per field.
            assert_eq!(
                INITIALIZER_FIELD_NAMES[op].len(),
                fields.len(),
                "op {op} ({name}) field name count"
            );
        }
    }

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
        // The typed view is derived from the op's layout, under the reference's names.
        let fields = effect.systems[0].initializers[0]
            .fields
            .as_ref()
            .expect("the layout gives the fields");
        assert_eq!(fields["channel"], FieldValue::U32(0));
        assert_eq!(fields["min"], FieldValue::F32(0.0));
        assert_eq!(fields["max"], FieldValue::F32(0.0));
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
