//! Darktide `MaterialResource` compilation and decompilation.
//!
//! Darktide stores materials in the "stream" layout that is also parsed by the
//! Bitsquid Blender tools (`stingray/material.py`, `MaterialResourceDT`). The
//! bytes referenced by a material's bundle entry are the material stream
//! itself:
//!
//! ```text
//! u32 version                 // 60, 61 or 62
//! u32 material_offset         // always 28
//! u32 material_size
//! u32 shader_offset           // u32::MAX if no embedded shader
//! u32 shader_size
//! u32 unk2_offset             // u32::MAX if absent
//! u32 unk2_size
//! // at material_offset:
//! u32 name                    // IdString32, usually 0
//! MaterialTemplate...
//! // at shader_offset (if present): raw shader/DXBC blob
//! // at unk2_offset (if present): raw blob
//! ```
//!
//! `MaterialTemplate` (with little-endian, Stingray-style serialization):
//!
//! ```text
//! u64 material1               // primary parent material
//! u64 material2               // secondary parent material
//! IdString32[] unk1           // shader texture channels (base materials only)
//! TextureChannel[] textures   // (IdString32 channel, u64 texture)
//! MaterialContext[] contexts  // (IdString32 context, IdString32 material)
//! ShaderVariableReflection[]  // (u32 class, u32 elements, IdString32 name,
//!                             //  u32 offset, u32 stride)
//! u8[] variable_data
//! (IdString32, bool)[] unk2
//! (u32, u32)[] unk3
//! ```
//!
//! Materials that carry their own shader (`shader_size > 0`) are "base"
//! materials. Everything else is an instance that inherits its shader through
//! `material1`/`material2`.
//!
//! Base materials round trip through `shader_data`, which preserves their
//! embedded shader byte for byte. Custom shader programs can be spliced in with
//! [`ShaderOverrides`], which decodes and re-compresses the affected frames and
//! updates the shader section header.
//!
//! The SJSON representation follows the Stingray source format also used by
//! Vermintide 2, i.e. string values and map-style `textures`,
//! `material_contexts` and `variables`. A decompiled material looks like:
//!
//! ```sjson
//! parent_material = "shaders/fx/example"
//! material_contexts = {
//!   surface_material = "cloth"
//! }
//! textures = {
//!   texture_map = "textures/example/foo"
//! }
//! variables = {
//!   dirt = {
//!     type = "scalar"
//!     value = [0.5]
//!     offset = 0
//!   }
//! }
//! ```
//!
//! Note that a material's `variable_data` may contain floats that are not
//! described by any reflection entry. Those are preserved in the `extra_data`
//! field so that decompile -> compile round trips stay byte-for-byte identical.
//!
//! # References
//!
//! The Darktide material layout was reverse engineered with the help of:
//! - the Bitsquid Blender tools' `stingray/material.py` and its Vermintide 2
//!   dictionary (GPL-3.0, by qasikfwn, adapted from `vt2_bundle_unpacker`)
//! - `limn` by manshanko, which confirmed that material payloads live in an
//!   external `data/` file like streamed textures
//! - `filediver` by xypwn (BSD-3-Clause), whose Helldivers 2 material parser is
//!   a reference for a related engine version

use std::io::{Cursor, Seek, SeekFrom};
use std::marker::PhantomData;
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result, bail};
use color_eyre::{Help, SectionExt};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::fs;

use crate::binary::sync::{ReadExt, WriteExt};
use crate::bundle::file::UserFile;
use crate::murmur::{HashGroup, IdString32, IdString64};
use crate::{BundleFile, BundleFileType, BundleFileVariant, Properties};

const EXPECTED_VERSIONS: [u32; 3] = [60, 61, 62];
const DEFAULT_VERSION: u32 = 61;
const MATERIAL_OFFSET: usize = 28;
const NO_OFFSET: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Serde-friendly hash wrappers
// ---------------------------------------------------------------------------
//
// `IdString64`/`IdString32` always serialize to their numeric hash, which is
// unreadable in SJSON. These wrappers emit known names as strings and unknown
// hashes as fixed-width uppercase hex strings, and parse them back the same
// way.
//
// String values are always written quoted (`"..."`), because the SJSON number
// grammar would otherwise read a value like `4C567810` as the integer `4`.

/// Writes `s` as a quoted SJSON string. `serialize_bytes` is used because the
/// `serde_sjson` serializer only quotes strings that contain certain
/// characters, which would leave e.g. hex hashes ambiguous.
fn serialize_quoted<S: Serializer>(serializer: S, s: &str) -> Result<S::Ok, S::Error> {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    serializer.serialize_bytes(out.as_bytes())
}

/// Whether `s` can be written as a bare SJSON identifier. Used for map keys,
/// which (like in the Stingray source format) are not quoted when they are
/// plain names.
fn is_bare_identifier(s: &str) -> bool {
    if s.is_empty() || matches!(s, "true" | "false" | "null") {
        return false;
    }

    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    s.chars()
        .all(|c| !matches!(c, '"' | '\'' | '\\' | '=' | ':' | ' ' | '\t' | '\n' | '\r'))
}

fn parse_idstring64<E: serde::de::Error>(value: &str) -> Result<IdString64, E> {
    if let Some(hex) = value.strip_prefix('#') {
        Ok(IdString64::Hash(
            u64::from_str_radix(hex, 16).map_err(E::custom)?.into(),
        ))
    } else if value.is_empty() {
        Ok(IdString64::Hash(0.into()))
    } else {
        Ok(IdString64::String(value.to_string()))
    }
}

fn parse_idstring32<E: serde::de::Error>(value: &str) -> Result<IdString32, E> {
    if let Some(hex) = value.strip_prefix('#') {
        Ok(IdString32::Hash(
            u32::from_str_radix(hex, 16).map_err(E::custom)?.into(),
        ))
    } else if value.is_empty() {
        Ok(IdString32::Hash(0.into()))
    } else {
        Ok(IdString32::String(value.to_string()))
    }
}

/// Converts a stored `IdString64` into its display form. Values that already
/// carry a string are kept as-is; raw hashes are looked up in the dictionary
/// and fall back to an explicit `#` prefixed hex hash.
fn display_idstring64(ctx: &crate::Context, value: &IdString64) -> IdString64 {
    match value {
        IdString64::String(_) => value.clone(),
        IdString64::Hash(hash) => ctx.lookup_hash(*hash, HashGroup::Other),
    }
}

/// See [`display_idstring64`].
fn display_idstring32(ctx: &crate::Context, value: &IdString32) -> IdString32 {
    match value {
        IdString32::String(_) => value.clone(),
        IdString32::Hash(hash) => ctx.lookup_hash_short(*hash, HashGroup::Other),
    }
}

fn repr(idstring: &IdString64) -> String {
    match idstring {
        IdString64::String(s) => s.clone(),
        IdString64::Hash(h) => format!("#{h:016X}"),
    }
}

fn repr32(idstring: &IdString32) -> String {
    match idstring {
        IdString32::String(s) => s.clone(),
        IdString32::Hash(h) => format!("#{h:08X}"),
    }
}

/// A 64-bit resource name. Always serialized as a quoted string.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hash64(IdString64);

impl Hash64 {
    fn to_idstring(&self) -> IdString64 {
        self.0.clone()
    }
}

impl Serialize for Hash64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_quoted(serializer, &repr(&self.0))
    }
}

impl<'de> Deserialize<'de> for Hash64 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Hash64;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a resource name or a 16 digit hex hash")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Hash64(parse_idstring64::<E>(v)?))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Hash64(IdString64::Hash(v.into())))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Hash64(IdString64::Hash((v as u64).into())))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// A 32-bit name. Always serialized as a quoted string.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hash32(IdString32);

impl Hash32 {
    fn to_idstring(&self) -> IdString32 {
        self.0.clone()
    }
}

impl Serialize for Hash32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_quoted(serializer, &repr32(&self.0))
    }
}

impl<'de> Deserialize<'de> for Hash32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Hash32;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a name or an 8 digit hex hash")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Hash32(parse_idstring32::<E>(v)?))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Hash32(IdString32::Hash((v as u32).into())))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Hash32(IdString32::Hash((v as u32).into())))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// A 32-bit name used as a map key. Plain names are written as bare
/// identifiers (like the Stingray source format); hashes are quoted.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hash32Key(Hash32);

impl Hash32Key {
    fn to_idstring(&self) -> IdString32 {
        self.0.to_idstring()
    }
}

impl Serialize for Hash32Key {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = repr32(&self.0.0);
        if is_bare_identifier(&value) {
            serializer.serialize_str(&value)
        } else {
            serialize_quoted(serializer, &value)
        }
    }
}

impl<'de> Deserialize<'de> for Hash32Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Hash32::deserialize(deserializer).map(Hash32Key)
    }
}

/// A string that is always quoted when serialized. Used for `extra_data` hex
/// blobs, which would otherwise be misread as numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QuotedString(String);

impl Serialize for QuotedString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_quoted(serializer, &self.0)
    }
}

impl<'de> Deserialize<'de> for QuotedString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = QuotedString;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(QuotedString(v.to_string()))
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

/// A map that remembers insertion order, so decompile -> compile keeps the
/// original texture/channel/variable order.
#[derive(Clone, Debug, PartialEq)]
struct OrderedMap<K, V>(Vec<(K, V)>);

impl<K, V> Default for OrderedMap<K, V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<K, V> OrderedMap<K, V> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: Serialize, V: Serialize> Serialize for OrderedMap<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<K, V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::MapAccess;

        struct Visitor<K, V>(PhantomData<(K, V)>);
        impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<K, V> {
            type Value = OrderedMap<K, V>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(4));
                while let Some((key, value)) = map.next_entry()? {
                    entries.push((key, value));
                }
                Ok(OrderedMap(entries))
            }
        }

        deserializer.deserialize_map(Visitor(PhantomData))
    }
}

// ---------------------------------------------------------------------------
// SJSON model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
enum VarType {
    #[serde(rename = "scalar")]
    Scalar,
    #[serde(rename = "vector2")]
    Vector2,
    #[serde(rename = "vector3")]
    Vector3,
    #[serde(rename = "vector4")]
    Vector4,
    #[serde(rename = "other")]
    Other,
}

impl VarType {
    fn class(&self) -> u32 {
        match self {
            Self::Scalar => 0,
            Self::Vector2 => 1,
            Self::Vector3 => 2,
            Self::Vector4 => 3,
            Self::Other => 12,
        }
    }

    fn from_class(class: u32) -> Self {
        match class {
            0 => Self::Scalar,
            1 => Self::Vector2,
            2 => Self::Vector3,
            3 => Self::Vector4,
            _ => Self::Other,
        }
    }

    /// Number of floats a single element occupies.
    fn element_len(&self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Vector2 => 2,
            Self::Vector3 => 3,
            Self::Vector4 => 4,
            Self::Other => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
struct VariableDef {
    #[serde(rename = "type")]
    kind: VarType,
    /// Always an array, even for a scalar. The SJSON number grammar parses a
    /// float like `0.5` as the integer `0` followed by `.5`, so numbers that
    /// are not inside an array cannot be round-tripped reliably.
    value: Vec<f32>,
    /// Byte offset into `variable_data`. Optional on input: omitted offsets are
    /// packed after the previous variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<u32>,
    /// Number of elements (arrays). `0` means a single element.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    elements: u32,
    /// Element stride in bytes, only meaningful for `type = "other"`.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    stride: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
struct Unk3Def {
    a: u32,
    b: u32,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_empty_map<K, V>(map: &OrderedMap<K, V>) -> bool {
    map.is_empty()
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
struct MaterialDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<Hash32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_material: Option<Hash64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_material_2: Option<Hash64>,
    /// Size of the embedded shader blob, informational on decompile.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    shader_size: u32,
    /// Hex encoded shader blob. Base materials (materials with a shader) carry
    /// a compiled DXBC blob; preserving it lets a mod ship its own copy of a
    /// base material instead of depending on a game resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shader_data: Option<QuotedString>,
    /// Shader texture channels (`unk1`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channels: Vec<Hash32>,
    #[serde(default, skip_serializing_if = "is_empty_map")]
    textures: OrderedMap<Hash32Key, Hash64>,
    #[serde(default, skip_serializing_if = "is_empty_map")]
    material_contexts: OrderedMap<Hash32Key, Hash32>,
    #[serde(default, skip_serializing_if = "is_empty_map")]
    variables: OrderedMap<Hash32Key, VariableDef>,
    /// Bytes of `variable_data` that are not described by any variable, hex
    /// encoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra_data: Option<QuotedString>,
    #[serde(default, skip_serializing_if = "is_empty_map")]
    unk2: OrderedMap<Hash32Key, bool>,
    /// Hex encoded `unk2_data` blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unk2_data: Option<QuotedString>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unk3: Vec<Unk3Def>,
}

// ---------------------------------------------------------------------------
// Binary model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Reflection {
    class: u32,
    elements: u32,
    name: IdString32,
    offset: u32,
    stride: u32,
}

impl Reflection {
    /// Number of bytes this variable occupies inside `variable_data`.
    fn size(&self) -> usize {
        let elements = if self.elements == 0 {
            1
        } else {
            self.elements as usize
        };

        match self.class {
            0 => 4 * elements,
            1 => 8 * elements,
            2 => 12 * elements,
            3 => 16 * elements,
            _ => (self.elements as usize) * (self.stride as usize),
        }
    }
}

#[derive(Clone, Debug)]
struct MaterialTemplate {
    name: IdString32,
    material1: IdString64,
    material2: IdString64,
    unk1: Vec<IdString32>,
    textures: Vec<(IdString32, IdString64)>,
    contexts: Vec<(IdString32, IdString32)>,
    reflection: Vec<Reflection>,
    variable_data: Vec<u8>,
    unk2: Vec<(IdString32, bool)>,
    unk3: Vec<(u32, u32)>,
}

impl Default for MaterialTemplate {
    fn default() -> Self {
        Self {
            name: IdString32::Hash(0.into()),
            material1: IdString64::Hash(0.into()),
            material2: IdString64::Hash(0.into()),
            unk1: Vec::new(),
            textures: Vec::new(),
            contexts: Vec::new(),
            reflection: Vec::new(),
            variable_data: Vec::new(),
            unk2: Vec::new(),
            unk3: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Material {
    version: u32,
    template: MaterialTemplate,
    shader: Vec<u8>,
    unk2_data: Vec<u8>,
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02X}");
    }
    s
}

fn from_hex(s: &str) -> Result<Vec<u8>> {
    let s: String = s
        .trim()
        .strip_prefix('#')
        .unwrap_or(s.trim())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if s.len() % 2 != 0 {
        bail!("hex string has an odd length");
    }

    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
        .collect()
}

fn read_f32(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("slice of 4 bytes"),
    )
}

impl Material {
    fn from_binary(data: &[u8]) -> Result<Self> {
        let mut r = Cursor::new(data);

        let version = r.read_u32()?;
        if !EXPECTED_VERSIONS.contains(&version) {
            bail!("Unsupported material version {version} (expected one of {EXPECTED_VERSIONS:?})");
        }

        let material_offset = r.read_u32()? as usize;
        let material_size = r.read_u32()? as usize;
        let shader_offset = r.read_u32()?;
        let shader_size = r.read_u32()? as usize;
        let unk2_offset = r.read_u32()?;
        let unk2_size = r.read_u32()? as usize;

        if material_offset != MATERIAL_OFFSET {
            bail!("Unexpected material offset {material_offset}, expected {MATERIAL_OFFSET}");
        }

        // Material template.
        r.seek(SeekFrom::Start(material_offset as u64))?;
        let name = r.read_u32()?.into();

        let material1 = IdString64::from(r.read_u64()?);
        let material2 = IdString64::from(r.read_u64()?);

        let mut unk1 = Vec::new();
        for _ in 0..r.read_u32()? {
            unk1.push(r.read_u32()?.into());
        }

        let mut textures = Vec::new();
        for _ in 0..r.read_u32()? {
            let channel = r.read_u32()?.into();
            let texture = IdString64::from(r.read_u64()?);
            textures.push((channel, texture));
        }

        let mut contexts = Vec::new();
        for _ in 0..r.read_u32()? {
            let context = r.read_u32()?.into();
            let material = r.read_u32()?.into();
            contexts.push((context, material));
        }

        let mut reflection = Vec::new();
        for _ in 0..r.read_u32()? {
            reflection.push(Reflection {
                class: r.read_u32()?,
                elements: r.read_u32()?,
                name: r.read_u32()?.into(),
                offset: r.read_u32()?,
                stride: r.read_u32()?,
            });
        }

        let variable_data = r.byte_array()?;

        let mut unk2 = Vec::new();
        for _ in 0..r.read_u32()? {
            let name = r.read_u32()?.into();
            let value = r.read_bool()?;
            unk2.push((name, value));
        }

        let mut unk3 = Vec::new();
        for _ in 0..r.read_u32()? {
            let a = r.read_u32()?;
            let b = r.read_u32()?;
            unk3.push((a, b));
        }

        let consumed = r.stream_position()? as usize;
        if consumed != material_offset + material_size {
            bail!(
                "Material template ended at {consumed}, expected {}",
                material_offset + material_size
            );
        }

        let shader = if shader_size > 0 && shader_offset != NO_OFFSET {
            r.seek(SeekFrom::Start(shader_offset as u64))?;
            r.byte_array_fixed(shader_size)?
        } else {
            Vec::new()
        };

        let unk2_data = if unk2_size > 0 && unk2_offset != NO_OFFSET {
            r.seek(SeekFrom::Start(unk2_offset as u64))?;
            r.byte_array_fixed(unk2_size)?
        } else {
            Vec::new()
        };

        Ok(Self {
            version,
            template: MaterialTemplate {
                name,
                material1,
                material2,
                unk1,
                textures,
                contexts,
                reflection,
                variable_data,
                unk2,
                unk3,
            },
            shader,
            unk2_data,
        })
    }

    fn template_to_binary(&self) -> Result<Vec<u8>> {
        let mut w = Cursor::new(Vec::new());

        w.write_u32(self.template.name.to_murmur32().into())?;
        w.write_u64(self.template.material1.to_murmur64().into())?;
        w.write_u64(self.template.material2.to_murmur64().into())?;

        w.write_u32(self.template.unk1.len() as u32)?;
        for v in &self.template.unk1 {
            w.write_u32(v.to_murmur32().into())?;
        }

        w.write_u32(self.template.textures.len() as u32)?;
        for (channel, texture) in &self.template.textures {
            w.write_u32(channel.to_murmur32().into())?;
            w.write_u64(texture.to_murmur64().into())?;
        }

        w.write_u32(self.template.contexts.len() as u32)?;
        for (context, material) in &self.template.contexts {
            w.write_u32(context.to_murmur32().into())?;
            w.write_u32(material.to_murmur32().into())?;
        }

        w.write_u32(self.template.reflection.len() as u32)?;
        for r in &self.template.reflection {
            w.write_u32(r.class)?;
            w.write_u32(r.elements)?;
            w.write_u32(r.name.to_murmur32().into())?;
            w.write_u32(r.offset)?;
            w.write_u32(r.stride)?;
        }

        write_byte_array(&mut w, &self.template.variable_data)?;

        w.write_u32(self.template.unk2.len() as u32)?;
        for (name, value) in &self.template.unk2 {
            w.write_u32(name.to_murmur32().into())?;
            w.write_bool(*value)?;
        }

        w.write_u32(self.template.unk3.len() as u32)?;
        for (a, b) in &self.template.unk3 {
            w.write_u32(*a)?;
            w.write_u32(*b)?;
        }

        Ok(w.into_inner())
    }

    fn to_binary(&self) -> Result<Vec<u8>> {
        let template = self.template_to_binary()?;
        let material_size = template.len();

        let shader_offset = if self.shader.is_empty() {
            NO_OFFSET
        } else {
            (MATERIAL_OFFSET + material_size) as u32
        };

        let after_shader = MATERIAL_OFFSET
            + material_size
            + if self.shader.is_empty() {
                0
            } else {
                self.shader.len()
            };

        let unk2_offset = if self.unk2_data.is_empty() {
            NO_OFFSET
        } else {
            after_shader as u32
        };

        let mut w = Cursor::new(Vec::new());
        w.write_u32(self.version)?;
        w.write_u32(MATERIAL_OFFSET as u32)?;
        w.write_u32(material_size as u32)?;
        w.write_u32(shader_offset)?;
        w.write_u32(self.shader.len() as u32)?;
        w.write_u32(unk2_offset)?;
        w.write_u32(self.unk2_data.len() as u32)?;

        debug_assert_eq!(w.stream_position()? as usize, MATERIAL_OFFSET);

        w.write_all_bytes(&template)?;

        if !self.shader.is_empty() {
            w.write_all_bytes(&self.shader)?;
        }

        if !self.unk2_data.is_empty() {
            w.write_all_bytes(&self.unk2_data)?;
        }

        Ok(w.into_inner())
    }

    fn to_sjson(&self, ctx: &crate::Context) -> Result<String> {
        let t = &self.template;

        let mut variables: OrderedMap<Hash32Key, VariableDef> = OrderedMap::default();
        // Track which bytes of `variable_data` are described by a variable so
        // that any extra bytes can be preserved verbatim.
        let mut covered = vec![false; t.variable_data.len()];

        for r in &t.reflection {
            let size = r.size();
            let offset = r.offset as usize;
            if offset + size > t.variable_data.len() {
                bail!(
                    "Variable at offset {offset} with size {size} exceeds variable_data length {}",
                    t.variable_data.len()
                );
            }

            for byte in &mut covered[offset..offset + size] {
                *byte = true;
            }

            let kind = VarType::from_class(r.class);
            let values = if kind == VarType::Other {
                // `elements` is the element count, `stride` the element size.
                let count = size / 4;
                (0..count)
                    .map(|i| read_f32(&t.variable_data, offset + i * 4))
                    .collect::<Vec<_>>()
            } else {
                let count = if r.elements == 0 {
                    1
                } else {
                    r.elements as usize
                };
                let stride = size / count.max(1);
                let mut values = Vec::with_capacity(count * kind.element_len());
                for i in 0..count {
                    let start = offset + i * stride;
                    for j in 0..kind.element_len() {
                        values.push(read_f32(&t.variable_data, start + j * 4));
                    }
                }
                values
            };

            variables.0.push((
                Hash32Key(Hash32(display_idstring32(ctx, &r.name))),
                VariableDef {
                    kind,
                    value: values,
                    offset: Some(r.offset),
                    elements: r.elements,
                    stride: r.stride,
                },
            ));
        }

        let extra_data = {
            let mut extra = Vec::new();
            for (i, is_covered) in covered.iter().enumerate() {
                if !is_covered && t.variable_data[i] != 0 {
                    extra.push((i, t.variable_data[i]));
                }
            }
            if extra.is_empty() {
                None
            } else {
                // Emit the full gap including intervening zero bytes, so the
                // bytes keep their positions.
                let last = extra.last().unwrap().0;
                Some(QuotedString(to_hex(&t.variable_data[..=last])))
            }
        };

        let mut textures: OrderedMap<Hash32Key, Hash64> = OrderedMap::default();
        for (channel, texture) in &t.textures {
            textures.0.push((
                Hash32Key(Hash32(display_idstring32(ctx, channel))),
                Hash64(display_idstring64(ctx, texture)),
            ));
        }

        let mut material_contexts: OrderedMap<Hash32Key, Hash32> = OrderedMap::default();
        for (context, material) in &t.contexts {
            material_contexts.0.push((
                Hash32Key(Hash32(display_idstring32(ctx, context))),
                Hash32(display_idstring32(ctx, material)),
            ));
        }

        let mut unk2: OrderedMap<Hash32Key, bool> = OrderedMap::default();
        for (name, value) in &t.unk2 {
            unk2.0
                .push((Hash32Key(Hash32(display_idstring32(ctx, name))), *value));
        }

        let def = MaterialDefinition {
            name: if u32::from(t.name.to_murmur32()) == 0 {
                None
            } else {
                Some(Hash32(display_idstring32(ctx, &t.name)))
            },
            parent_material: if u64::from(t.material1.to_murmur64()) == 0 {
                None
            } else {
                Some(Hash64(display_idstring64(ctx, &t.material1)))
            },
            parent_material_2: if u64::from(t.material2.to_murmur64()) == 0 {
                None
            } else {
                Some(Hash64(display_idstring64(ctx, &t.material2)))
            },
            shader_size: self.shader.len() as u32,
            shader_data: if self.shader.is_empty() {
                None
            } else {
                Some(QuotedString(to_hex(&self.shader)))
            },
            channels: t
                .unk1
                .iter()
                .map(|c| Hash32(display_idstring32(ctx, c)))
                .collect(),
            textures,
            material_contexts,
            variables,
            extra_data,
            unk2,
            unk2_data: if self.unk2_data.is_empty() {
                None
            } else {
                Some(QuotedString(to_hex(&self.unk2_data)))
            },
            unk3: t
                .unk3
                .iter()
                .map(|(a, b)| Unk3Def { a: *a, b: *b })
                .collect(),
        };

        serde_sjson::to_string(&def).wrap_err("Failed to serialize material to SJSON")
    }

    fn from_definition(def: MaterialDefinition) -> Result<Self> {
        Self::from_definition_with_shader(def, None)
    }

    /// Like [`Material::from_definition`], but with the shader section to embed:
    /// the definition's own `shader_data`/`shader_size` are ignored when a
    /// section is given, so a caller can hand over generated bytes without
    /// hex-encoding them into the source first.
    fn from_definition_with_shader(
        def: MaterialDefinition,
        shader_override: Option<&[u8]>,
    ) -> Result<Self> {
        let shader = match shader_override {
            Some(shader) => shader.to_vec(),
            None => {
                let shader = match &def.shader_data {
                    Some(data) => from_hex(&data.0).wrap_err("Invalid 'shader_data'")?,
                    None => Vec::new(),
                };

                if shader.len() != def.shader_size as usize {
                    bail!(
                        "Material has a {} byte shader blob but 'shader_size' is {}",
                        shader.len(),
                        def.shader_size
                    );
                }

                shader
            }
        };

        let unk2_data = match &def.unk2_data {
            Some(data) => from_hex(&data.0).wrap_err("Invalid 'unk2_data'")?,
            None => Vec::new(),
        };

        let mut variable_data: Vec<u8> = match &def.extra_data {
            Some(hex) => from_hex(&hex.0).wrap_err("Invalid 'extra_data'")?,
            None => Vec::new(),
        };

        let mut reflection = Vec::with_capacity(def.variables.0.len());

        for (name, variable) in &def.variables.0 {
            let class = variable.kind.class();
            let values = &variable.value;

            let expected = if class == 12 {
                variable.elements as usize * variable.stride as usize / 4
            } else {
                let count = if variable.elements == 0 {
                    1
                } else {
                    variable.elements as usize
                };
                count * variable.kind.element_len()
            };

            if class != 12 && values.len() != expected {
                bail!(
                    "Variable '{}' is a {:?} but has {} values (expected {})",
                    name.to_idstring().display(),
                    variable.kind,
                    values.len(),
                    expected
                );
            }

            let size = values.len() * 4;
            let offset = match variable.offset {
                Some(offset) => offset,
                None => (variable_data.len().div_ceil(4) * 4) as u32,
            };

            let end = offset as usize + size;
            if variable_data.len() < end {
                variable_data.resize(end, 0);
            }

            for (i, value) in values.iter().enumerate() {
                let start = offset as usize + i * 4;
                variable_data[start..start + 4].copy_from_slice(&value.to_le_bytes());
            }

            reflection.push(Reflection {
                class,
                elements: variable.elements,
                name: name.to_idstring(),
                offset,
                stride: variable.stride,
            });
        }

        let template = MaterialTemplate {
            name: def
                .name
                .map(|n| n.to_idstring())
                .unwrap_or(IdString32::Hash(0.into())),
            material1: def
                .parent_material
                .map(|m| m.to_idstring())
                .unwrap_or(IdString64::Hash(0.into())),
            material2: def
                .parent_material_2
                .map(|m| m.to_idstring())
                .unwrap_or(IdString64::Hash(0.into())),
            unk1: def.channels.into_iter().map(|c| c.to_idstring()).collect(),
            textures: def
                .textures
                .0
                .into_iter()
                .map(|(k, v)| (k.to_idstring(), v.to_idstring()))
                .collect(),
            contexts: def
                .material_contexts
                .0
                .into_iter()
                .map(|(k, v)| (k.to_idstring(), v.to_idstring()))
                .collect(),
            reflection,
            variable_data,
            unk2: def
                .unk2
                .0
                .into_iter()
                .map(|(k, v)| (k.to_idstring(), v))
                .collect(),
            unk3: def.unk3.into_iter().map(|u| (u.a, u.b)).collect(),
        };

        Ok(Self {
            version: DEFAULT_VERSION,
            template,
            shader,
            unk2_data,
        })
    }
}

// Small helpers that are not (yet) part of `ReadExt`/`WriteExt`.

trait MaterialReadExt: ReadExt {
    fn byte_array(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        self.byte_array_fixed(len)
    }

    fn byte_array_fixed(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0; len];
        std::io::Read::read_exact(self, &mut buf)?;
        Ok(buf)
    }
}
impl<T: ReadExt> MaterialReadExt for T {}

trait MaterialWriteExt: WriteExt {
    fn write_all_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        std::io::Write::write_all(self, bytes)?;
        Ok(())
    }
}
impl<T: WriteExt> MaterialWriteExt for T {}

fn write_byte_array(w: &mut impl WriteExt, bytes: &[u8]) -> Result<()> {
    w.write_u32(bytes.len() as u32)?;
    std::io::Write::write_all(w, bytes)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Bundle integration
// ---------------------------------------------------------------------------

/// Builds a material bundle file whose payload lives in an external data file,
/// mirroring how the game stores materials.
fn external_material_file(name: IdString64, data: Vec<u8>) -> BundleFile {
    let hash = format!("{:016x}", u64::from(name.to_murmur64()));
    let data_file_name = format!("data/{}/{}", &hash[..2], hash);

    let mut variant = BundleFileVariant::new();
    variant.set_external(true);
    variant.set_external_data_file(data_file_name, data);

    let mut file = BundleFile::new(name, BundleFileType::Material);
    file.set_props(Properties::DATA);
    file.add_variant(variant);
    file
}

#[tracing::instrument(skip_all, fields(name = %name.display()))]
pub fn compile(name: IdString64, sjson: impl AsRef<str>) -> Result<BundleFile> {
    compile_with_shader(name, sjson, None)
}

/// Like [`compile`], but with the shader section to embed: the SJSON's own
/// `shader_data` field is ignored when `shader` is given, so a build can hand
/// over a generated section without stringifying it into the material source.
#[tracing::instrument(skip_all, fields(name = %name.display()))]
pub fn compile_with_shader(
    name: IdString64,
    sjson: impl AsRef<str>,
    shader: Option<&[u8]>,
) -> Result<BundleFile> {
    let sjson = sjson.as_ref();
    let def: MaterialDefinition = serde_sjson::from_str(sjson)
        .wrap_err("Failed to deserialize SJSON")
        .with_section(|| sjson.to_string().header("SJSON:"))?;

    let material = Material::from_definition_with_shader(def, shader)
        .wrap_err("Invalid material definition")?;
    let data = material.to_binary()?;

    Ok(external_material_file(name, data))
}

/// DXBC containers used to replace a base material's shader programs.
///
/// Programs are matched by shader stage; programs whose stage has no
/// replacement are preserved byte for byte. The frame is re-compressed with
/// Oodle and the frame key, device data size and default data offset are
/// updated automatically.
#[derive(Default, Clone)]
pub struct ShaderOverrides {
    pub vertex: Option<Vec<u8>>,
    pub pixel: Option<Vec<u8>>,
}

impl ShaderOverrides {
    pub fn is_empty(&self) -> bool {
        self.vertex.is_none() && self.pixel.is_none()
    }
}

/// Replaces the shader programs of a material stream with the given DXBC
/// containers.
pub fn replace_shader_programs(data: &[u8], overrides: &ShaderOverrides) -> Result<Vec<u8>> {
    if data.len() < 28 {
        bail!("Material stream is too small");
    }

    let shader_offset = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    let shader_size = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
    let unk2_offset = u32::from_le_bytes(data[20..24].try_into().unwrap()) as usize;
    let unk2_size = u32::from_le_bytes(data[24..28].try_into().unwrap()) as usize;

    if shader_size == 0 {
        bail!("Material has no embedded shader to replace");
    }

    let shader = data
        .get(shader_offset..shader_offset + shader_size)
        .ok_or_else(|| color_eyre::eyre::eyre!("Shader section is out of range"))?;
    let unk2 = data
        .get(unk2_offset..unk2_offset + unk2_size)
        .ok_or_else(|| color_eyre::eyre::eyre!("Trailing section is out of range"))?;

    let new_shader = crate::filetype::shader::rebuild(shader, |program| match program.stage {
        crate::filetype::shader::Stage::Vertex => overrides.vertex.clone(),
        crate::filetype::shader::Stage::Pixel => overrides.pixel.clone(),
        _ => None,
    })?;

    let mut out = Vec::with_capacity(shader_offset + new_shader.len() + unk2.len());
    out.extend_from_slice(&data[..shader_offset]);
    out.extend_from_slice(&new_shader);
    let new_unk2_offset = out.len();
    out.extend_from_slice(unk2);

    out[16..20].copy_from_slice(&(new_shader.len() as u32).to_le_bytes());
    out[20..24].copy_from_slice(&(new_unk2_offset as u32).to_le_bytes());

    Ok(out)
}

/// Applies shader overrides to every variant of a compiled base material.
pub fn apply_shader_overrides(file: &mut BundleFile, overrides: &ShaderOverrides) -> Result<()> {
    if overrides.is_empty() {
        return Ok(());
    }

    for variant in file.variants_mut() {
        let Some(data_file_name) = variant.data_file_name().cloned() else {
            continue;
        };
        let Some(data) = variant.external_data().cloned() else {
            continue;
        };

        let new_data = replace_shader_programs(&data, overrides)
            .wrap_err_with(|| format!("Failed to replace shader programs in '{data_file_name}'"))?;

        variant.set_external_data_file(data_file_name, new_data);
    }

    Ok(())
}

#[tracing::instrument(skip(ctx, variant))]
pub async fn decompile(
    ctx: &crate::Context,
    name: String,
    variant: &BundleFileVariant,
) -> Result<Vec<UserFile>> {
    let data = match variant.data_file_name() {
        Some(data_file_name) => {
            let path = data_file_name_path(ctx, data_file_name);
            fs::read(&path)
                .await
                .wrap_err_with(|| format!("Failed to read material data file '{}'", path.display()))
                .with_suggestion(|| {
                    "Provide a game directory in the config file or make sure the `data` \
                     directory is next to the provided bundle."
                })?
        }
        None => variant.data().to_vec(),
    };

    let sjson = decompile_data(ctx, &data)?;
    Ok(vec![UserFile::with_name(sjson.into_bytes(), name)])
}

/// Decompiles raw material data (a material data file's bytes) to SJSON.
///
/// This is the raw-data form of [`decompile`]: the caller has the bytes rather
/// than a bundle file and its data file.
pub fn decompile_data(ctx: &crate::Context, data: &[u8]) -> Result<String> {
    let material = Material::from_binary(data).wrap_err("Failed to parse material")?;
    if !material.shader.is_empty() {
        tracing::debug!(
            "Material embeds a {}-byte shader; it is preserved through 'shader_data'.",
            material.shader.len()
        );
    }

    material.to_sjson(ctx)
}

fn data_file_name_path(ctx: &crate::Context, data_file_name: &str) -> PathBuf {
    match &ctx.game_dir {
        Some(dir) => dir.join("bundle").join(data_file_name),
        None => PathBuf::from("bundle").join(data_file_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_definition() -> MaterialDefinition {
        let textures: OrderedMap<Hash32Key, Hash64> = OrderedMap(vec![
            (
                Hash32Key(Hash32(IdString32::String("nar".to_string()))),
                Hash64(IdString64::String("textures/foo/bar_nar".to_string())),
            ),
            (
                Hash32Key(Hash32(IdString32::String("albedo".to_string()))),
                Hash64(IdString64::String("textures/foo/bar_bc".to_string())),
            ),
        ]);

        let material_contexts: OrderedMap<Hash32Key, Hash32> = OrderedMap(vec![(
            Hash32Key(Hash32(IdString32::String("surface_material".to_string()))),
            Hash32(IdString32::String("cloth".to_string())),
        )]);

        let variables: OrderedMap<Hash32Key, VariableDef> = OrderedMap(vec![
            (
                Hash32Key(Hash32(IdString32::String("dirt".to_string()))),
                VariableDef {
                    kind: VarType::Scalar,
                    value: vec![0.5],
                    offset: Some(0),
                    elements: 0,
                    stride: 0,
                },
            ),
            (
                Hash32Key(Hash32(IdString32::String("tint".to_string()))),
                VariableDef {
                    kind: VarType::Vector3,
                    value: vec![0.1, 0.2, 0.3],
                    offset: Some(16),
                    elements: 0,
                    stride: 0,
                },
            ),
        ]);

        MaterialDefinition {
            name: None,
            parent_material: Some(Hash64(IdString64::String("materials/base/foo".to_string()))),
            parent_material_2: None,
            shader_size: 0,
            shader_data: None,
            channels: vec![],
            textures,
            material_contexts,
            variables,
            extra_data: None,
            unk2: OrderedMap::default(),
            unk2_data: None,
            unk3: vec![Unk3Def { a: 1, b: 0 }],
        }
    }

    #[test]
    fn round_trip_binary() {
        let definition = sample_definition();
        let material = Material::from_definition(definition.clone()).unwrap();
        let bin = material.to_binary().unwrap();

        let parsed = Material::from_binary(&bin).unwrap();
        let bin2 = parsed.to_binary().unwrap();

        assert_eq!(bin, bin2, "re-serialized material differs from original");
    }

    #[test]
    fn round_trip_definition() {
        let definition = sample_definition();
        let material = Material::from_definition(definition.clone()).unwrap();

        // Serialize to SJSON and parse it back.
        let ctx = crate::Context::new();
        let sjson = material.to_sjson(&ctx).unwrap();
        let parsed: MaterialDefinition = serde_sjson::from_str(&sjson).unwrap();

        assert_eq!(parsed.parent_material, definition.parent_material);
        assert_eq!(parsed.textures, definition.textures);
        assert_eq!(parsed.material_contexts, definition.material_contexts);
        assert_eq!(parsed.variables, definition.variables);
        assert_eq!(parsed.unk3, definition.unk3);
    }

    #[test]
    fn extra_data_is_preserved() {
        let mut material = Material::from_definition(sample_definition()).unwrap();
        // A float between the two variables that no reflection entry refers to.
        material.template.variable_data[8..12].copy_from_slice(&1.25f32.to_le_bytes());

        let bin = material.to_binary().unwrap();
        let parsed = Material::from_binary(&bin).unwrap();
        let ctx = crate::Context::new();
        let sjson = parsed.to_sjson(&ctx).unwrap();
        let def: MaterialDefinition = serde_sjson::from_str(&sjson).unwrap();
        assert!(def.extra_data.is_some());

        let recompiled = Material::from_definition(def).unwrap().to_binary().unwrap();
        assert_eq!(bin, recompiled);
    }

    #[test]
    fn embedded_shader_is_preserved() {
        let mut definition = sample_definition();
        let shader: Vec<u8> = (0..=255u8).collect();
        definition.shader_size = shader.len() as u32;
        definition.shader_data = Some(QuotedString(to_hex(&shader)));

        let material = Material::from_definition(definition).unwrap();
        let bin = material.to_binary().unwrap();
        let parsed = Material::from_binary(&bin).unwrap();

        assert_eq!(parsed.shader, shader);
        assert_eq!(parsed.to_binary().unwrap(), bin);
    }

    #[test]
    fn hex_hashes_are_quoted() {
        let mut definition = sample_definition();
        definition.textures.0[0].1 = Hash64(IdString64::Hash(0x4c56_7810_0000_0001u64.into()));

        let material = Material::from_definition(definition).unwrap();
        let ctx = crate::Context::new();
        let sjson = material.to_sjson(&ctx).unwrap();
        assert!(sjson.contains("\"#4C56781000000001\""), "{sjson}");

        // And it must parse back to the same hash.
        let parsed: MaterialDefinition = serde_sjson::from_str(&sjson).unwrap();
        assert_eq!(
            parsed.textures.0[0].1,
            Hash64(IdString64::Hash(0x4c56_7810_0000_0001u64.into()))
        );
    }
}
