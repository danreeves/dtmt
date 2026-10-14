//! The `animation` resource: a cooked skeletal clip.
//!
//! A clip is a header, a **sync table** - one rest pose per joint of the rig the
//! clip animates - and then a flat stream of items, terminated by a footer that
//! must land on the very last byte.
//!
//! ```text
//! 0       u32   unknown_0
//! 4       u32   unknown_4
//! 8       u32   joint count      the sync table is exactly count * 40 bytes
//! 12      f32   duration
//! 16      u32   body size        the resource's own length
//! 20      u32   beat count N
//! 24      N x (f32 time_slice, f32 quantisation)
//! 24+8N   u16   sync marker
//! 26+8N   joints * 40 bytes:     f32 position[3], f32 rotation[4], f32 scale[3]
//!         then items until a `u16 3` footer that must end the body
//! ```
//!
//! The joint count is a real cross-resource link: for the first-person rig it is
//! `146`, which is the joint count of the sibling `base.bones`. It was first read
//! as a format version, and the correction came from decoding that sibling.
//!
//! # The item stream
//!
//! An item opens with a `u16`. Its top two bits choose the encoding:
//!
//! ```text
//! 0x0000 | kind 0..7   unpacked: u16 bone, f32 time, then 3 or 4 f32
//! 0xC000               packed:   see below
//! ```
//!
//! Unpacked kinds are `2` trigger, `3` footer, `4` translation, `5` rotation,
//! `6` scale and `7` the sync marker; the packed kinds are `0x8000` translation,
//! `0xC000` rotation and `0x4000` scale.
//!
//! A **packed** item fuses its type, joint and time into one `u32` -
//! `type (2 bits) | bone (10 bits) | milliseconds (20 bits)` - and then stores
//! quantised values: three `u16` for a translation or scale, spanning
//! `-10 .. 10` metres, and four `u16` for a rotation, spanning `-pi .. pi` before
//! normalisation.
//!
//! That fusion is what made the stream look broken. Reading `idle` as if every
//! item were unpacked works for 147 scale keys and then reads `0xC000`, which is
//! not a kind at all - it is the rotation item's type bits, and the joint and
//! time are packed into the same word. So the game's clips use the packed items
//! for the animated channels and the unpacked form for the static ones.
//!
//! # Grounding
//!
//! The header, the sync table and the unpacked item layout are also what the
//! community's cooked writer emits and validates
//! (`C:\dev\Darktide-Asset-Compiler`, `animation/animation_{writer,evaluator}.cpp`),
//! and the packed item types and quantisers come from
//! `bitsquid-blender-tools` (`bitsquid/stingray/animation.py`,
//! `bitsquid/deserializing.py`), which reads shipped Darktide clips. So the grammar
//! here is those two readers' grammar, not an inference from samples.
//!
//! The sync table is always the unpacked one in every shipped clip (marker `7`); a
//! packed sync (marker `1`) is rejected rather than mis-read, since a packed sync
//! cannot round-trip through decoded values.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type hash the cooked-resource envelope carries for an animation resource.
pub const ANIMATION_TYPE: u64 = 0x931e_336d_7646_cc26;

/// The cooked-resource envelope's fixed size, as `bones` uses it too.
const ENVELOPE: usize = 38;

/// The header up to and including the beat count word; the sync marker follows
/// `8 * N` bytes of beats.
const HEADER: usize = 26;

/// One sync pose: `position[3]`, `rotation[4]`, `scale[3]`.
const POSE: usize = 40;

/// The sync marker for a full-precision sync table.
const SYNC_UNPACKED: u16 = 7;

/// The sync marker for a quantised sync table, which is not implemented.
const SYNC_PACKED: u16 = 1;

/// The stream's terminator.
const FOOTER: u16 = 3;
/// A trigger event: a `u32` id then an `f32` time.
const TRIGGER: u16 = 2;
/// The unpacked key kinds.
const POSITION: u16 = 4;
const ROTATION: u16 = 5;
const SCALE: u16 = 6;
/// The packed item types, which are also the top two bits of the opening `u16`.
const PACKED_ROTATION: u16 = 0xC000;
const PACKED_POSITION: u16 = 0x8000;
const PACKED_SCALE: u16 = 0x4000;
/// The mask that selects the packed types.
const PACKED_MASK: u16 = 0xC000;

/// A packed translation or scale spans `-10 .. 10`.
const VEC3_SPAN: f64 = 20.0;
/// A packed rotation spans `-pi .. pi` before normalisation.
const QUAT_SPAN: f64 = 2.0 * std::f64::consts::PI;
/// One unit of a 16-bit field.
const FIELD: f64 = 65536.0;
/// A packed item's time is in milliseconds.
const MILLISECOND: f32 = 0.001;

/// One joint's rest pose: the state every channel starts from.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    /// The joint's local translation.
    pub position: [f32; 3],
    /// The joint's local rotation, `(x, y, z, w)`.
    pub rotation: [f32; 4],
    /// The joint's local scale, `(1,1,1)` on a rig that stores no scale.
    pub scale: [f32; 3],
}

impl Pose {
    /// Whether the pose is finite and its rotation is a quaternion. A mis-read of
    /// the sync table shows up here, which is how the "half-float" reading of the
    /// payload was caught: `1.0` read as two `u16`s is `0x0000, 0x3F80`, so the
    /// upper half of a float looks exactly like a half-float `1.0`.
    fn is_sane(&self) -> bool {
        let finite = self
            .position
            .iter()
            .chain(self.rotation.iter())
            .chain(self.scale.iter())
            .all(|v| v.is_finite());
        if !finite {
            return false;
        }
        let norm: f32 = self.rotation.iter().map(|v| v * v).sum();
        (norm.sqrt() - 1.0).abs() <= 1e-3
    }
}

/// A beat: a time slice and the quantisation interval the exporter chose for it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Beat {
    pub time_slice: f32,
    pub quantisation: f32,
}

/// One item in the stream: a key, or a trigger event.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    /// `"translation"`, `"rotation"`, `"scale"` or `"trigger"`.
    pub kind: String,
    /// Whether the key is stored in the packed, quantised form. The animated
    /// channels of a shipped clip usually are.
    #[serde(default, skip_serializing_if = "is_false")]
    pub packed: bool,
    /// The joint the key addresses, or the trigger's id.
    pub bone: u32,
    /// The time in seconds. A packed item stores whole milliseconds.
    pub time: f32,
    /// 3 floats for a translation or scale, 4 for a rotation, empty for a trigger.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value: Vec<f32>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// A cooked skeletal animation clip.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    /// Unread header word at offset 0.
    #[serde(default)]
    pub unknown_0: u32,
    /// Unread header word at offset 4.
    #[serde(default)]
    pub unknown_4: u32,
    /// The clip's length in seconds.
    pub duration: f32,
    /// The beat table; empty for a clip that carries none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beats: Vec<Beat>,
    /// One sync pose per joint of the animated rig.
    pub joints: Vec<Pose>,
    /// The keys and triggers, in the order they are stored.
    pub tracks: Vec<Track>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Dequantises a `u16` into `low .. low + span`.
fn dequantise(word: u16, span: f64, low: f64) -> f32 {
    (word as f64 * (span / FIELD) + low) as f32
}

/// Quantises a value back into its `u16`, saturating rather than wrapping.
fn quantise(value: f32, span: f64, low: f64) -> u16 {
    let steps = ((value as f64 - low) * (FIELD / span)).round();
    steps.clamp(0.0, FIELD - 1.0) as u16
}

fn packed_vec3(b: &[u8], at: usize) -> [f32; 3] {
    [0, 1, 2].map(|i| dequantise(u16_at(b, at + i * 2), VEC3_SPAN, -10.0))
}

fn packed_quat(b: &[u8], at: usize) -> [f32; 4] {
    let qword = u64_at(b, at);
    // The four components are stored directly as u16s, in `x, y, z, w` order.
    [0u32, 1, 2, 3].map(|i| dequantise(((qword >> (16 * i)) & 0xFFFF) as u16, QUAT_SPAN, -std::f64::consts::PI))
}

/// The item's kind and the width of its value, or `None` if `first` is not a kind
/// this module writes.
fn unpacked_kind(first: u16) -> Option<(&'static str, usize)> {
    Some(match first {
        POSITION => ("translation", 3),
        ROTATION => ("rotation", 4),
        SCALE => ("scale", 3),
        TRIGGER => ("trigger", 0),
        _ => return None,
    })
}

fn packed_kind(packed_type: u16) -> (&'static str, usize, usize) {
    match packed_type {
        PACKED_SCALE => ("scale", 3, 6),
        PACKED_POSITION => ("translation", 3, 6),
        _ => ("rotation", 4, 8),
    }
}

/// A bundle variant is the bare body; a cooked-resource blob wraps it in the
/// 38-byte envelope, as `bones` documents at length.
fn unwrap(bytes: &[u8]) -> Result<&[u8]> {
    if bytes.len() < ENVELOPE || u64_at(bytes, 0) != ANIMATION_TYPE {
        return Ok(bytes);
    }
    let body_size = u32_at(bytes, 29) as usize;
    let stream_size = u32_at(bytes, 34) as usize;
    if bytes.len() != ENVELOPE + body_size + stream_size {
        bail!("the animation envelope's sizes do not match its length");
    }
    Ok(&bytes[ENVELOPE..ENVELOPE + body_size])
}

impl Animation {
    /// Reads a clip from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        let body = unwrap(bytes)?;
        if body.len() < HEADER {
            bail!("an animation is shorter than its {HEADER}-byte header");
        }
        let unknown_0 = u32_at(body, 0);
        let unknown_4 = u32_at(body, 4);
        let joints = u32_at(body, 8) as usize;
        let duration = f32_at(body, 12);
        let size = u32_at(body, 16) as usize;
        let beat_count = u32_at(body, 20) as usize;

        if size != body.len() {
            bail!(
                "an animation declares {size} bytes but its body is {}",
                body.len()
            );
        }
        if !duration.is_finite() || duration < 0.0 {
            bail!("an animation's duration {duration} is not a finite, non-negative time");
        }

        let marker_at = HEADER - 2 + beat_count * 8;
        if marker_at + 2 > body.len() {
            bail!("an animation's {beat_count} beats run past its body");
        }
        let mut beats = Vec::with_capacity(beat_count);
        for i in 0..beat_count {
            let at = HEADER - 2 + i * 8;
            beats.push(Beat {
                time_slice: f32_at(body, at),
                quantisation: f32_at(body, at + 4),
            });
        }

        // The sync marker is what moves when the clip carries beats, and checking
        // it at its shifted home is what shows the beat count was read right.
        let sync = u16_at(body, marker_at);
        if sync == SYNC_PACKED {
            bail!("an animation's sync table is packed (item 1), which is not implemented");
        }
        if sync != SYNC_UNPACKED {
            bail!("an animation's sync marker is {sync}, not {SYNC_UNPACKED}");
        }

        let table_start = marker_at + 2;
        let table_end = table_start + joints * POSE;
        if table_end > body.len() {
            bail!("an animation's sync table of {joints} joints runs past its body");
        }
        let mut poses = Vec::with_capacity(joints);
        for bone in 0..joints {
            let base = table_start + bone * POSE;
            let f = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9].map(|i| f32_at(body, base + i * 4));
            let pose = Pose {
                position: [f[0], f[1], f[2]],
                rotation: [f[3], f[4], f[5], f[6]],
                scale: [f[7], f[8], f[9]],
            };
            if !pose.is_sane() {
                bail!("an animation's sync pose for joint {bone} is not a finite, unit-quaternion pose");
            }
            poses.push(pose);
        }

        // The item stream. Every field is checked, so a clip encoded differently is
        // rejected rather than silently mis-read.
        let mut at = table_end;
        let mut tracks = Vec::new();
        let mut terminated = false;
        while at + 2 <= body.len() {
            let first = u16_at(body, at);
            at += 2;

            if first & PACKED_MASK != 0 {
                // type (2) | bone (10) | time (20, whole milliseconds)
                if at + 2 > body.len() {
                    bail!("a packed animation key's header runs past the body");
                }
                let second = u16_at(body, at);
                at += 2;
                let fused = ((first as u32) << 16) | second as u32;
                let (kind, width, stride) = packed_kind(first & PACKED_MASK);
                let bone = (fused >> 20) & 0x3FF;
                let time = (fused & 0xFFFFF) as f32 * MILLISECOND;
                if at + stride > body.len() {
                    bail!("a packed animation key's value runs past the body");
                }
                let value = if width == 4 {
                    packed_quat(body, at).to_vec()
                } else {
                    packed_vec3(body, at).to_vec()
                };
                at += stride;
                // A packed rotation is quantised on a `-pi .. pi` grid, so its
                // length is a quaternion only to within a step or two.
                if width == 4 {
                    let norm: f32 = value.iter().map(|v| v * v).sum();
                    if (norm.sqrt() - 1.0).abs() > 2e-3 {
                        bail!("a packed rotation key is not a quaternion");
                    }
                }
                if bone as usize >= joints {
                    bail!("a packed key targets joint {bone} of a {joints}-joint rig");
                }
                check_time(time, duration, MILLISECOND)?;
                tracks.push(Track {
                    kind: kind.to_string(),
                    packed: true,
                    bone,
                    time,
                    value,
                });
                continue;
            }

            if first == FOOTER {
                terminated = true;
                break;
            }
            let Some((kind, width)) = unpacked_kind(first) else {
                bail!("an animation item's type {first} is not one this profile defines");
            };
            let need = if first == TRIGGER { 8 } else { 6 + width * 4 };
            if at + need > body.len() {
                bail!("an animation item runs past the body");
            }
            let bone = if first == TRIGGER {
                let bone = u32_at(body, at);
                at += 4;
                bone
            } else {
                let bone = u16_at(body, at) as u32;
                at += 2;
                if bone as usize >= joints {
                    bail!("an animation key targets joint {bone} of a {joints}-joint rig");
                }
                bone
            };
            let time = f32_at(body, at);
            at += 4;
            check_time(time, duration, 0.0)?;
            let mut value = Vec::with_capacity(width);
            for i in 0..width {
                let v = f32_at(body, at + i * 4);
                if !v.is_finite() {
                    bail!("an animation key's value is not finite");
                }
                value.push(v);
            }
            at += width * 4;
            tracks.push(Track {
                kind: kind.to_string(),
                packed: false,
                bone,
                time,
                value,
            });
        }
        if !terminated {
            bail!("an animation's item stream has no footer");
        }
        // A footer anywhere but the last two bytes means the stream was read out of
        // step, so the trailing bytes are a symptom rather than padding.
        if at != body.len() {
            bail!(
                "an animation's footer leaves {} trailing bytes",
                body.len() - at
            );
        }

        Ok(Self {
            unknown_0,
            unknown_4,
            duration,
            beats,
            joints: poses,
            tracks,
        })
    }

    /// Writes the clip back, deriving the body size and the beat count.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let joints = self.joints.len();
        if joints > u32::MAX as usize {
            bail!("an animation has more joints than its count word can hold");
        }
        if !self.duration.is_finite() || self.duration < 0.0 {
            bail!(
                "an animation's duration {} is not a finite, non-negative time",
                self.duration
            );
        }

        let mut out = Vec::with_capacity(HEADER + self.beats.len() * 8 + joints * POSE + self.tracks.len() * 24);
        out.extend_from_slice(&self.unknown_0.to_le_bytes());
        out.extend_from_slice(&self.unknown_4.to_le_bytes());
        out.extend_from_slice(&(joints as u32).to_le_bytes());
        out.extend_from_slice(&self.duration.to_le_bytes());
        // The body size is only known once the body exists; patched below.
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(self.beats.len() as u32).to_le_bytes());
        for beat in &self.beats {
            out.extend_from_slice(&beat.time_slice.to_le_bytes());
            out.extend_from_slice(&beat.quantisation.to_le_bytes());
        }
        out.extend_from_slice(&SYNC_UNPACKED.to_le_bytes());

        for (bone, pose) in self.joints.iter().enumerate() {
            if !pose.is_sane() {
                bail!("the sync pose for joint {bone} is not a finite, unit-quaternion pose");
            }
            for v in pose
                .position
                .iter()
                .chain(pose.rotation.iter())
                .chain(pose.scale.iter())
            {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }

        for track in &self.tracks {
            let (kind, width) = match (track.kind.as_str(), track.packed) {
                ("translation", false) => (POSITION, 3),
                ("rotation", false) => (ROTATION, 4),
                ("scale", false) => (SCALE, 3),
                ("trigger", _) => (TRIGGER, 0),
                ("translation", true) => (PACKED_POSITION, 3),
                ("rotation", true) => (PACKED_ROTATION, 4),
                ("scale", true) => (PACKED_SCALE, 3),
                (other, _) => bail!("an animation item's channel is '{other}', not translation, rotation, scale or trigger"),
            };
            if kind != TRIGGER {
                if track.bone as usize >= joints {
                    bail!("an item targets joint {} of a {joints}-joint rig", track.bone);
                }
            }
            if track.value.len() != width {
                bail!(
                    "a {} item carries {} floats, not {width}",
                    track.kind,
                    track.value.len()
                );
            }
            if !track.value.iter().all(|v| v.is_finite()) {
                bail!("a key's value is not finite");
            }

            if track.packed {
                if kind == TRIGGER {
                    bail!("a trigger cannot be packed");
                }
                if track.bone > 0x3FF {
                    bail!("a packed key's joint {} exceeds the 10 bits it is stored in", track.bone);
                }
                let millis = (track.time / MILLISECOND).round();
                if !(0.0..=0xFFFFF as f32).contains(&millis) {
                    bail!("a packed key's time {} is outside the 20 bits it is stored in", track.time);
                }
                check_time(track.time, self.duration, MILLISECOND)?;
                let fused = ((kind as u32) << 16)
                    | (track.bone << 20)
                    | (millis as u32);
                out.extend_from_slice(&((fused >> 16) as u16).to_le_bytes());
                out.extend_from_slice(&(fused as u16).to_le_bytes());
                if width == 4 {
                    if (track.value.iter().map(|v| v * v).sum::<f32>().sqrt() - 1.0).abs() > 2e-3 {
                        bail!("a rotation key is not a quaternion");
                    }
                    for v in &track.value {
                        out.extend_from_slice(&quantise(*v, QUAT_SPAN, -std::f64::consts::PI).to_le_bytes());
                    }
                } else {
                    for v in &track.value {
                        out.extend_from_slice(&quantise(*v, VEC3_SPAN, -10.0).to_le_bytes());
                    }
                }
                continue;
            }

            check_time(track.time, self.duration, 0.0)?;
            out.extend_from_slice(&kind.to_le_bytes());
            if kind == TRIGGER {
                out.extend_from_slice(&track.bone.to_le_bytes());
            } else {
                if track.bone > u16::MAX as u32 {
                    bail!("a key's joint {} exceeds the u16 it is stored in", track.bone);
                }
                out.extend_from_slice(&(track.bone as u16).to_le_bytes());
            }
            out.extend_from_slice(&track.time.to_le_bytes());
            for v in &track.value {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&FOOTER.to_le_bytes());

        let size = out.len() as u32;
        out[16..20].copy_from_slice(&size.to_le_bytes());
        Ok(out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the resource from its SJSON, round-tripping the body so a bad edit is
    /// caught on load rather than at build time.
    pub fn from_sjson(text: &str) -> Result<Self> {
        let animation: Self = serde_sjson::from_str(text).map_err(color_eyre::Report::new)?;
        Self::from_binary(&animation.to_binary()?)
    }
}

/// A key's time must lie inside the clip, with the tolerance its own encoding
/// imposes: an unpacked key is an exact `f32`, a packed one holds whole
/// milliseconds and so can land up to half a step past the end.
fn check_time(time: f32, duration: f32, tolerance: f32) -> Result<()> {
    if !time.is_finite() || time < 0.0 || time > duration + tolerance + 1e-5 {
        bail!("an animation key's time {time} is outside 0..={duration}");
    }
    Ok(())
}

/// Decompiles an animation variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("an animation resource needs a variant");
    };
    let animation = Animation::from_binary(variant.data())?;
    Ok(vec![UserFile::new(animation.to_sjson()?.into_bytes())])
}

/// Compiles an animation SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let animation = Animation::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(animation.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::Animation);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    fn push_f32(out: &mut Vec<u8>, v: f32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    /// `idle`'s first packed rotation: the fused header plus the four quantised
    /// components the game ships, whose `0x7fff`s dequantise to zero and whose
    /// `0xa8bd` dequantises to one.
    const SHIPPED_PACKED_ROTATION: [u8; 12] = [
        0x00, 0xc0, 0x10, 0x00, 0xff, 0x7f, 0xff, 0x7f, 0xff, 0x7f, 0xbd, 0xa8,
    ];

    /// A two-joint clip whose stream holds one key of each kind.
    fn sample() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        push_f32(&mut out, 1.5);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes());
        for v in [0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        for v in [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        // An unpacked translation.
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        push_f32(&mut out, 0.5);
        for v in [1.0, 2.0, 3.0] {
            push_f32(&mut out, v);
        }
        // An unpacked rotation.
        out.extend_from_slice(&5u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        push_f32(&mut out, 0.25);
        for v in [0.0, 0.0, 0.0, 1.0] {
            push_f32(&mut out, v);
        }
        // An unpacked scale.
        out.extend_from_slice(&6u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        push_f32(&mut out, 1.0);
        for v in [1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        out.extend_from_slice(&3u16.to_le_bytes());
        let size = out.len() as u32;
        out[16..20].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// The same clip with a beat, which moves the sync marker and the table by
    /// eight bytes, plus the packed rotation the game ships.
    fn sample_with_beat_and_packed_key() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        push_f32(&mut out, 1.5);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        push_f32(&mut out, 0.0333333);
        push_f32(&mut out, 0.0166667);
        out.extend_from_slice(&7u16.to_le_bytes());
        for v in [0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        for v in [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        out.extend_from_slice(&SHIPPED_PACKED_ROTATION);
        out.extend_from_slice(&3u16.to_le_bytes());
        let size = out.len() as u32;
        out[16..20].copy_from_slice(&size.to_le_bytes());
        out
    }

    #[test]
    fn a_sample_clip_round_trips() {
        let bytes = sample();
        let animation = Animation::from_binary(&bytes).expect("the sample parses");
        assert_eq!(animation.joints.len(), 2);
        assert_eq!(animation.duration, 1.5);
        assert!(animation.beats.is_empty());
        assert_eq!(animation.joints[0].position, [0.0, 0.0, 1.5]);
        assert_eq!(animation.joints[1].rotation, [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(animation.tracks.len(), 3);
        assert_eq!(animation.tracks[0].kind, "translation");
        assert_eq!(animation.tracks[0].bone, 1);
        assert_eq!(animation.tracks[0].value, vec![1.0, 2.0, 3.0]);
        assert_eq!(animation.tracks[1].value.len(), 4, "a rotation is four floats");
        assert!(!animation.tracks.iter().any(|t| t.packed));
        assert_eq!(animation.to_binary().unwrap(), bytes, "byte-for-byte");
    }

    #[test]
    fn a_beat_shifts_the_sync_marker_and_the_table() {
        let bytes = sample_with_beat_and_packed_key();
        let animation = Animation::from_binary(&bytes).expect("a clip with a beat parses");
        assert_eq!(animation.beats.len(), 1);
        assert_eq!(animation.beats[0].time_slice, 0.0333333);
        assert_eq!(animation.beats[0].quantisation, 0.0166667);
        assert_eq!(animation.joints.len(), 2, "the table is 8 bytes past the marker");
        assert_eq!(animation.joints[0].position, [0.0, 0.0, 1.5]);
        assert_eq!(animation.to_binary().unwrap(), bytes, "byte-for-byte");
    }

    #[test]
    fn the_shipped_packed_rotation_decodes_to_the_identity() {
        let animation =
            Animation::from_binary(&sample_with_beat_and_packed_key()).expect("the sample parses");
        let key = animation.tracks.last().expect("a packed key");
        assert_eq!(key.kind, "rotation");
        assert!(key.packed);
        // type | bone | milliseconds: `0xc000`, joint 0, 16 ms.
        assert_eq!(key.bone, 0);
        assert_eq!(key.time, 0.016);
        for (got, want) in key.value.iter().zip([0.0, 0.0, 0.0, 1.0]) {
            assert!((got - want).abs() < 1e-3, "a quantised component: {got} vs {want}");
        }
        // And the quantisation is exact, so the bytes come back unchanged - which
        // the round-trip above already asserts key by key.
        assert_eq!(animation.to_binary().unwrap(), sample_with_beat_and_packed_key());
    }

    #[test]
    fn a_joint_count_and_size_are_derived_not_stored() {
        let animation = Animation::from_binary(&sample()).unwrap();
        let sjson = animation.to_sjson().unwrap();
        assert!(!sjson.contains("size"), "the size word is not authored: {sjson}");
        assert_eq!(Animation::from_sjson(&sjson).unwrap(), animation);
    }

    #[test]
    fn a_sync_marker_that_is_not_seven_is_rejected() {
        let mut bytes = sample();
        bytes[24] = 6;
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_packed_sync_table_is_rejected_by_name() {
        let mut bytes = sample();
        bytes[24] = 1;
        let error = Animation::from_binary(&bytes).unwrap_err().to_string();
        assert!(error.contains("packed"), "the error names the encoding: {error}");
    }

    #[test]
    fn a_size_word_that_lies_is_rejected() {
        let mut bytes = sample();
        bytes[16] ^= 0xFF;
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_non_unit_sync_quaternion_is_rejected() {
        let mut bytes = sample();
        let at = HEADER + 3 * 4;
        let broken = (f32_at(&bytes, at) + 0.5).to_le_bytes();
        bytes[at..at + 4].copy_from_slice(&broken);
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn an_out_of_range_joint_is_rejected() {
        let mut bytes = sample();
        let at = HEADER + 2 * POSE + 2;
        bytes[at..at + 2].copy_from_slice(&9u16.to_le_bytes());
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn trailing_bytes_after_the_footer_are_rejected() {
        let mut bytes = sample();
        bytes.push(0);
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_time_past_the_duration_is_rejected() {
        let mut bytes = sample();
        let at = HEADER + 2 * POSE + 4;
        bytes[at..at + 4].copy_from_slice(&9.0f32.to_le_bytes());
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_trigger_round_trips() {
        let mut bytes = sample();
        // Replace the footer with a trigger then the footer.
        let footer = bytes.len() - 2;
        bytes.truncate(footer);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        push_f32(&mut bytes, 0.75);
        bytes.extend_from_slice(&3u16.to_le_bytes());
        let size = bytes.len() as u32;
        bytes[16..20].copy_from_slice(&size.to_le_bytes());

        let animation = Animation::from_binary(&bytes).expect("a trigger parses");
        let trigger = animation.tracks.last().unwrap();
        assert_eq!(trigger.kind, "trigger");
        assert_eq!(trigger.bone, 0xDEAD_BEEF, "a trigger id is a full u32");
        assert!(trigger.value.is_empty());
        assert_eq!(animation.to_binary().unwrap(), bytes);
    }
}
