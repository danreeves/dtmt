//! The `animation` resource: a cooked skeletal clip.
//!
//! A clip is a header, a **base-state table** - one pose per joint of the rig the
//! clip animates - and then a flat stream of control points, one per joint per
//! channel, terminated by a sentinel that must land on the very last byte.
//!
//! ```text
//! 0       u32   unknown_0
//! 4       u32   unknown_4
//! 8       u32   joint count      the base-state table is exactly count * 40 bytes
//! 12      f32   duration
//! 16      u32   body size        the resource's own length
//! 20      u32   entry count N
//! 24      N * 8-byte entries
//! 24+8N   u16   marker = 7
//! 26+8N   joints * 40 bytes:     f32 position[3], f32 rotation[4], f32 scale[3]
//!         then, until a `u16 3` terminator that must end the body:
//!           u16 kind   4 = translation, 5 = rotation, 6 = scale
//!           u16 bone
//!           f32 time
//!           f32 value[4] for a rotation, f32 value[3] otherwise
//! ```
//!
//! The entry count matters and is easy to miss: it moves *both* the marker and the
//! base-state table by `8 * N` bytes, so a clip with one entry reads as garbage if
//! the table is assumed to start at 26. That is how the field was found - a clip
//! whose first control word read as `65529`, and whose first pose then decoded
//! perfectly eight bytes later with the marker `7` eight bytes before *that*.
//! The entries' own meaning is not derived, so they are carried verbatim.
//!
//! The joint count is a real cross-resource link: for the first-person rig it is
//! `146`, which is the joint count of the sibling `base.bones`. It was first read
//! as a format version, and the correction came from decoding that sibling.
//!
//! The base-state rotation is a quaternion and the scale is usually `(1,1,1)`;
//! both are *validated* rather than assumed - a mis-read of the table shows up as
//! a quaternion that is not unit length, which is how the "half-float" reading of
//! the payload was caught. `1.0` read as two `u16`s is `0x0000, 0x3F80`, so the
//! upper half of a float looks exactly like a half-float `1.0`.
//!
//! The layout is the same one the community's cooked writer emits and validates,
//! so the grammar here is the compiler's reader, not an inference from samples.
//!
//! The base-state table and the control stream are read here; the *curves* those
//! controls drive are the engine's to evaluate.

use color_eyre::{Report, Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type hash the cooked-resource envelope carries for an animation resource.
pub const ANIMATION_TYPE: u64 = 0x931e_336d_7646_cc26;

/// The cooked-resource envelope's fixed size, as `bones` uses it too.
const ENVELOPE: usize = 38;

/// The header's fixed length; the base-state table begins `8N` bytes after this,
/// where `N` is the entry count at offset 20.
const HEADER: usize = 26;

/// The base-state table's stride, one pose per joint.
const POSE: usize = 40;

/// The value every shipped animation carries at offset 24.
pub const MARKER: u16 = 7;

/// The control stream's terminator.
const TERMINATOR: u16 = 3;

/// The channel a control point addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    Translation = 4,
    Rotation = 5,
    Scale = 6,
}

impl Path {
    /// How many floats a control point of this channel carries.
    fn width(self) -> usize {
        match self {
            Path::Rotation => 4,
            Path::Translation | Path::Scale => 3,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Path::Translation => "translation",
            Path::Rotation => "rotation",
            Path::Scale => "scale",
        }
    }

    fn from_name(name: &str) -> Result<Self> {
        Ok(match name {
            "translation" => Path::Translation,
            "rotation" => Path::Rotation,
            "scale" => Path::Scale,
            other => bail!("an animation track's channel is '{other}', not translation, rotation or scale"),
        })
    }
}

/// One joint's rest pose: the base state every track starts from.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    /// The joint's local translation.
    pub position: [f32; 3],
    /// The joint's local rotation, a unit quaternion `(x, y, z, w)`.
    pub rotation: [f32; 4],
    /// The joint's local scale, `(1,1,1)` on a rig that stores no scale.
    pub scale: [f32; 3],
}

impl Pose {
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

/// One control point in the stream.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    /// The channel: `"translation"`, `"rotation"` or `"scale"`.
    pub path: String,
    /// The joint this control addresses, an index into the base-state table.
    pub bone: u16,
    /// The time, in seconds from the clip's start.
    pub time: f32,
    /// 3 floats, or 4 for a rotation.
    pub value: Vec<f32>,
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
    /// The pre-marker entries at offset 24, hex-encoded, `8 * N` bytes; the
    /// entry count word is derived from this length. Omitted when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<String>,
    /// The profile marker; every shipped clip carries 7.
    pub marker: u16,
    /// One base pose per joint of the animated rig.
    pub joints: Vec<Pose>,
    /// The control points, in the order they are stored.
    pub tracks: Vec<Track>,
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("an animation entry blob has an odd number of hex digits");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| color_eyre::eyre::eyre!("an animation entry blob is not hex"))
        })
        .collect()
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// A bundle variant is the bare body; a cooked-resource blob wraps it in an
/// envelope, as `bones` documents at length.
fn unwrap<'a>(bytes: &'a [u8]) -> Result<&'a [u8]> {
    if bytes.len() < ENVELOPE || u64::from_le_bytes(bytes[0..8].try_into().unwrap()) != ANIMATION_TYPE
    {
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
        let entry_count = u32_at(body, 20) as usize;
        let marker_at = HEADER - 2 + entry_count * 8;

        // The entry region lies between the count word and the marker; the marker
        // is what moves, and validating it at its shifted home is the check that
        // the count was read correctly.
        if marker_at + 2 > body.len() {
            bail!("an animation's {entry_count} header entries run past its body");
        }
        let entries = body[HEADER - 2..marker_at].to_vec();
        let marker = u16_at(body, marker_at);

        // The invariable header invariants. A clip whose own size word disagrees
        // with its length is not this profile at all.
        if marker != MARKER {
            bail!("an animation's marker is {marker}, not {MARKER}");
        }
        if size != body.len() {
            bail!(
                "an animation declares {size} bytes but its body is {}",
                body.len()
            );
        }
        if !duration.is_finite() || duration < 0.0 {
            bail!("an animation's duration {duration} is not a finite, non-negative time");
        }

        let table_start = marker_at + 2;
        let table_end = table_start + joints * POSE;
        if table_end > body.len() {
            bail!("an animation's base-state table of {joints} joints runs past its body");
        }
        let mut poses = Vec::with_capacity(joints);
        for bone in 0..joints {
            let base = table_start + bone * POSE;
            let mut f = [0.0f32; 10];
            for (i, slot) in f.iter_mut().enumerate() {
                *slot = f32_at(body, base + i * 4);
            }
            let pose = Pose {
                position: [f[0], f[1], f[2]],
                rotation: [f[3], f[4], f[5], f[6]],
                scale: [f[7], f[8], f[9]],
            };
            if !pose.is_sane() {
                bail!("an animation's base state for joint {bone} is not a finite, unit-quaternion pose");
            }
            poses.push(pose);
        }

        // The control stream. Every field is checked, so a clip encoded in a
        // different profile is rejected rather than silently mis-read.
        let mut at = table_end;
        let mut tracks = Vec::new();
        let mut terminated = false;
        while at + 2 <= body.len() {
            let kind = u16_at(body, at);
            at += 2;
            if kind == TERMINATOR {
                terminated = true;
                break;
            }
            let path = match kind {
                4 => Path::Translation,
                5 => Path::Rotation,
                6 => Path::Scale,
                other => bail!("an animation track's kind {other} is not 4, 5 or 6"),
            };
            if at + 6 > body.len() {
                bail!("an animation track header runs past the body");
            }
            let bone = u16_at(body, at);
            let time = f32_at(body, at + 2);
            at += 6;
            if bone as usize >= joints {
                bail!("an animation track targets joint {bone} of a {joints}-joint rig");
            }
            if !time.is_finite() || time < 0.0 || time > duration + 1e-5 {
                bail!("an animation track's time {time} is outside 0..={duration}");
            }
            let width = path.width();
            if at + width * 4 > body.len() {
                bail!("an animation track's value runs past the body");
            }
            let mut value = Vec::with_capacity(width);
            for i in 0..width {
                let v = f32_at(body, at + i * 4);
                if !v.is_finite() {
                    bail!("an animation track's value is not finite");
                }
                value.push(v);
            }
            at += width * 4;
            tracks.push(Track {
                path: path.name().to_string(),
                bone,
                time,
                value,
            });
        }
        if !terminated {
            bail!("an animation's control stream has no terminator");
        }
        // A terminator anywhere but the last two bytes means the stream was read
        // out of step, so the trailing bytes are a symptom rather than padding.
        if at != body.len() {
            bail!(
                "an animation's terminator leaves {} trailing bytes",
                body.len() - at
            );
        }

        Ok(Self {
            unknown_0,
            unknown_4,
            duration,
            entries: (!entries.is_empty()).then(|| hex_encode(&entries)),
            marker,
            joints: poses,
            tracks,
        })
    }

    /// Writes the clip back, deriving the body size and the entry count.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let joints = self.joints.len();
        if joints > u32::MAX as usize {
            bail!("an animation has more joints than its count word can hold");
        }
        if !self.duration.is_finite() || self.duration < 0.0 {
            bail!("an animation's duration {} is not a finite, non-negative time", self.duration);
        }
        let entries = match &self.entries {
            Some(hex) => hex_decode(hex)?,
            None => Vec::new(),
        };
        if entries.len() % 8 != 0 {
            bail!("an animation's entry blob is {} bytes, not a multiple of 8", entries.len());
        }
        let mut out = Vec::with_capacity(HEADER + entries.len() + joints * POSE + self.tracks.len() * 24);
        out.extend_from_slice(&self.unknown_0.to_le_bytes());
        out.extend_from_slice(&self.unknown_4.to_le_bytes());
        out.extend_from_slice(&(joints as u32).to_le_bytes());
        out.extend_from_slice(&self.duration.to_le_bytes());
        // The body size is only known once the body exists; patched below.
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&((entries.len() / 8) as u32).to_le_bytes());
        out.extend_from_slice(&entries);
        out.extend_from_slice(&self.marker.to_le_bytes());

        for (bone, pose) in self.joints.iter().enumerate() {
            if !pose.is_sane() {
                bail!("the base state for joint {bone} is not a finite, unit-quaternion pose");
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
            let path = Path::from_name(&track.path)?;
            if track.bone as usize >= joints {
                bail!("a track targets joint {} of a {joints}-joint rig", track.bone);
            }
            if !track.time.is_finite() || track.time < 0.0 || track.time > self.duration + 1e-5 {
                bail!("a track's time {} is outside 0..={}", track.time, self.duration);
            }
            if track.value.len() != path.width() {
                bail!(
                    "a {} track carries {} floats, not {}",
                    track.path,
                    track.value.len(),
                    path.width()
                );
            }
            if !track.value.iter().all(|v| v.is_finite()) {
                bail!("a track's value is not finite");
            }
            out.extend_from_slice(&(path as u16).to_le_bytes());
            out.extend_from_slice(&track.bone.to_le_bytes());
            out.extend_from_slice(&track.time.to_le_bytes());
            for v in &track.value {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&TERMINATOR.to_le_bytes());

        let size = out.len() as u32;
        out[16..20].copy_from_slice(&size.to_le_bytes());
        Ok(out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(Report::new)
    }

    /// Reads the resource from its SJSON, round-tripping the body so a bad edit
    /// is caught on load rather than at build time.
    pub fn from_sjson(text: &str) -> Result<Self> {
        let animation: Self = serde_sjson::from_str(text).map_err(Report::new)?;
        Self::from_binary(&animation.to_binary()?)
    }
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

    /// A two-joint clip with one control on each of the three channels, written
    /// out field by field the way the layout says.
    fn sample() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        push_f32(&mut out, 1.5);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes());
        // Joint 0: moved along z, no rotation, unit scale.
        for v in [0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        // Joint 1: at the origin, rotated 180 degrees about y, unit scale.
        for v in [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        push_f32(&mut out, 0.5);
        for v in [1.0, 2.0, 3.0] {
            push_f32(&mut out, v);
        }
        out.extend_from_slice(&5u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        push_f32(&mut out, 0.25);
        for v in [0.0, 0.0, 0.0, 1.0] {
            push_f32(&mut out, v);
        }
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

    /// The same clip as `equip` ships it: one header entry, so the marker and the
    /// base-state table both sit eight bytes later than in `sample`.
    fn sample_with_entry() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        push_f32(&mut out, 1.5);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&[0x1c, 0x22, 0x22, 0x3f, 0x59, 0x52, 0x07, 0x4f]);
        out.extend_from_slice(&7u16.to_le_bytes());
        for v in [0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        for v in [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            push_f32(&mut out, v);
        }
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        push_f32(&mut out, 0.5);
        for v in [1.0, 2.0, 3.0] {
            push_f32(&mut out, v);
        }
        out.extend_from_slice(&3u16.to_le_bytes());
        let size = out.len() as u32;
        out[16..20].copy_from_slice(&size.to_le_bytes());
        out
    }

    #[test]
    fn a_header_entry_shifts_the_marker_and_the_table() {
        let bytes = sample_with_entry();
        let animation = Animation::from_binary(&bytes).expect("a clip with one entry parses");
        assert_eq!(animation.marker, 7, "the marker is at 24 + 8N, not 24");
        assert_eq!(animation.joints.len(), 2);
        assert_eq!(
            animation.entries.as_deref(),
            Some("1c22223f5952074f"),
            "the entries are carried verbatim"
        );
        assert_eq!(animation.joints[0].position, [0.0, 0.0, 1.5]);
        assert_eq!(animation.to_binary().unwrap(), bytes, "byte-for-byte");
        // The count word follows from the blob, so a round trip through SJSON
        // cannot drift it.
        let sjson = animation.to_sjson().unwrap();
        assert_eq!(Animation::from_sjson(&sjson).unwrap(), animation);
    }

    #[test]
    fn an_entry_blob_that_is_not_a_multiple_of_eight_is_rejected() {
        let mut animation = Animation::from_binary(&sample()).unwrap();
        animation.entries = Some("1c2222".to_string());
        assert!(animation.to_binary().is_err());
    }

    #[test]
    fn a_sample_clip_round_trips() {
        let bytes = sample();
        let animation = Animation::from_binary(&bytes).expect("the sample parses");
        assert_eq!(animation.joints.len(), 2);
        assert_eq!(animation.duration, 1.5);
        assert_eq!(animation.marker, 7);
        assert_eq!(animation.joints[0].position, [0.0, 0.0, 1.5]);
        assert_eq!(animation.joints[1].rotation, [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(animation.tracks.len(), 3);
        assert_eq!(animation.tracks[0].path, "translation");
        assert_eq!(animation.tracks[0].bone, 1);
        assert_eq!(animation.tracks[0].value, vec![1.0, 2.0, 3.0]);
        assert_eq!(animation.tracks[1].value.len(), 4, "a rotation is four floats");
        assert_eq!(animation.to_binary().unwrap(), bytes, "byte-for-byte");
    }

    #[test]
    fn the_joint_count_and_size_are_derived_not_stored() {
        let animation = Animation::from_binary(&sample()).unwrap();
        let sjson = animation.to_sjson().unwrap();
        assert!(!sjson.contains("size"), "the size word is not authored: {sjson}");
        assert_eq!(Animation::from_sjson(&sjson).unwrap(), animation);
    }

    #[test]
    fn a_marker_that_is_not_seven_is_rejected() {
        let mut bytes = sample();
        bytes[24] = 6;
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_size_word_that_lies_is_rejected() {
        let mut bytes = sample();
        bytes[16] ^= 0xFF;
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_non_unit_base_quaternion_is_rejected() {
        let mut bytes = sample();
        // Joint 0's rotation w, the fourth float after its position.
        let at = HEADER + 3 * 4;
        let broken = (f32_at(&bytes, at) + 0.5).to_le_bytes();
        bytes[at..at + 4].copy_from_slice(&broken);
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_step_out_of_range_bone_is_rejected() {
        let mut bytes = sample();
        // The first track's bone word.
        let at = HEADER + 2 * POSE + 2;
        bytes[at..at + 2].copy_from_slice(&9u16.to_le_bytes());
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn trailing_bytes_after_the_terminator_are_rejected() {
        let mut bytes = sample();
        bytes.push(0);
        assert!(Animation::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_time_past_the_duration_is_rejected() {
        let mut bytes = sample();
        let at = HEADER + 2 * POSE + 4;
        let late = 9.0f32.to_le_bytes();
        bytes[at..at + 4].copy_from_slice(&late);
        assert!(Animation::from_binary(&bytes).is_err());
    }
}
