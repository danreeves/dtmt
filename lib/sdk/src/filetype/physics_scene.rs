//! The physics scene a unit carries.
//!
//! A unit's `physics` byte array is the native physics scene, and the layout comes
//! from the community compiler's DT reference
//! (`C:\dev\Darktide-Asset-Compiler`, `stingray/physics/physics_collection.h` and
//! `physics_scene.cpp`, whose `serialize_physics_scene` builds exactly this):
//!
//! ```text
//! dependencies            the first serialised PhysX collection
//! ... padded to 128       the second collection is 128-byte aligned
//! objects                 the second serialised collection
//! bodies                  72 bytes each
//! shape bindings          16 bytes each
//! template bindings        8 bytes each
//! material bindings        8 bytes each
//! header                  128 bytes, at the very end
//! ```
//!
//! The header's every offset and count is a function of those sizes - the dependency
//! block starts at 0, the second collection at the 128-aligned end of the first, the
//! bodies after it, and so on to `+ 128` for the header itself - so the whole layout
//! is **derived** and re-validated on read. That is what makes a wrong reading
//! impossible to slip through quietly: the offsets must add up to the blob's length
//! exactly.
//!
//! What is *not* derived, and so is carried: the two PhysX collections (opaque SDK
//! serialisation - the reference loads them *through PhysX* to describe them), the
//! header's first two words, its sixteen transform floats, and the bodies' velocities
//! and lookup words. A body's first lookup word points back at the body table, and
//! that one is checked against the derived offset.

use color_eyre::{Result, eyre::bail};

/// The type id of a `unit`, whose `physics` field this describes.
/// Not a bundle type of its own - the scene lives inside a unit.
pub const PHYSICS_HEADER: usize = 128;

/// A body's record: four words, six velocity floats and eight lookup words.
pub const BODY: usize = 72;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn align128(value: usize) -> usize {
    (value + 127) & !127
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("a physics collection has an odd number of hex digits");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| color_eyre::eyre::eyre!("a physics collection is not hex"))
        })
        .collect()
}

/// One body: four words, its velocity and its lookup words.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub object_index: u32,
    pub name_hash: u32,
    pub node_hash: u32,
    pub enabled: u32,
    pub velocity: [f32; 6],
    /// Eight words; the first points back at the body table and is checked.
    pub lookups: [u32; 8],
}

/// One shape binding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeBinding {
    pub body_index: u32,
    pub local_shape_index: u32,
    pub object_index: u32,
    pub flags: u32,
}

/// One template or material binding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub object_index: u32,
    pub hash: u32,
}

/// A unit's physics scene.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsScene {
    /// The header's first word, 41200 in the reference's output.
    #[serde(default)]
    pub version: u32,
    /// The header's second word.
    #[serde(default)]
    pub unknown_1: u32,
    /// The free-joint lookup table's count; the table's offset is derived.
    #[serde(default)]
    pub free_joint_count: u32,
    /// The header's sixteen floats, a root transform that is identity when the body
    /// poses are already in unit coordinates.
    #[serde(default)]
    pub root_transform: [f32; 16],
    /// The first PhysX collection, hex-encoded.
    #[serde(default)]
    pub dependencies: String,
    /// The second PhysX collection, hex-encoded.
    #[serde(default)]
    pub objects: String,
    #[serde(default)]
    pub bodies: Vec<Body>,
    #[serde(default)]
    pub shapes: Vec<ShapeBinding>,
    #[serde(default)]
    pub templates: Vec<Binding>,
    #[serde(default)]
    pub materials: Vec<Binding>,
}

struct Layout {
    second: usize,
    body: usize,
    shape: usize,
    template: usize,
    material: usize,
    total: usize,
}

fn layout(
    dependencies: usize,
    objects: usize,
    bodies: usize,
    shapes: usize,
    templates: usize,
    materials: usize,
) -> Layout {
    let second = align128(dependencies);
    let body = second + objects;
    let shape = body + bodies * BODY;
    let template = shape + shapes * 16;
    let material = template + templates * 8;
    Layout {
        second,
        body,
        shape,
        template,
        material,
        total: material + materials * 8 + PHYSICS_HEADER,
    }
}

impl PhysicsScene {
    /// Reads a physics scene from a unit's `physics` bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < PHYSICS_HEADER {
            bail!("a physics scene is shorter than its {PHYSICS_HEADER}-byte header");
        }
        let h = bytes.len() - PHYSICS_HEADER;
        let version = u32_at(bytes, h);
        let unknown_1 = u32_at(bytes, h + 4);
        let dep_offset = u32_at(bytes, h + 8) as usize;
        let dep_size = u32_at(bytes, h + 12) as usize;
        let root_transform = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
            .map(|i| f32_at(bytes, h + 16 + i * 4));
        let second = u32_at(bytes, h + 80) as usize;
        let objects_size = u32_at(bytes, h + 84) as usize;
        let body_offset = u32_at(bytes, h + 88) as usize;
        let body_count = u32_at(bytes, h + 92) as usize;
        let free_joint_offset = u32_at(bytes, h + 96) as usize;
        let free_joint_count = u32_at(bytes, h + 100);
        let shape_offset = u32_at(bytes, h + 104) as usize;
        let shape_count = u32_at(bytes, h + 108) as usize;
        let template_offset = u32_at(bytes, h + 112) as usize;
        let template_count = u32_at(bytes, h + 116) as usize;
        let material_offset = u32_at(bytes, h + 120) as usize;
        let material_count = u32_at(bytes, h + 124) as usize;

        // The reference's builder puts the dependency block at the very start, so its
        // offset is part of the layout rather than a field.
        if dep_offset != 0 {
            bail!("a physics scene's dependency block starts at {dep_offset}, not 0");
        }
        // A scene can bind more shapes than it has distinct templates, so the two
        // counts are read separately - `layout` uses each for its own table.
        let want = layout(
            dep_size,
            objects_size,
            body_count,
            shape_count,
            template_count,
            material_count,
        );
        if second != want.second
            || body_offset != want.body
            || free_joint_offset != want.body
            || shape_offset != want.shape
            || template_offset != want.template
            || material_offset != want.material
            || want.total != bytes.len()
        {
            bail!(
                "a physics scene's header does not describe its {} bytes (wanted second {:#x}, bodies {:#x}, shapes {:#x}, templates {:#x}, materials {:#x}, total {:#x})",
                bytes.len(),
                want.second,
                want.body,
                want.shape,
                want.template,
                want.material,
                want.total
            );
        }

        let mut bodies = Vec::with_capacity(body_count);
        for i in 0..body_count {
            let at = body_offset + i * BODY;
            let mut lookups = [0u32; 8];
            for (k, word) in lookups.iter_mut().enumerate() {
                *word = u32_at(bytes, at + 40 + k * 4);
            }
            if lookups[0] as usize != body_offset {
                bail!(
                    "a physics body points at {:#x} instead of its own table at {:#x}",
                    lookups[0],
                    body_offset
                );
            }
            bodies.push(Body {
                object_index: u32_at(bytes, at),
                name_hash: u32_at(bytes, at + 4),
                node_hash: u32_at(bytes, at + 8),
                enabled: u32_at(bytes, at + 12),
                velocity: [0, 1, 2, 3, 4, 5].map(|k| f32_at(bytes, at + 16 + k * 4)),
                lookups,
            });
        }
        let mut shapes = Vec::with_capacity(shape_count);
        for i in 0..shape_count {
            let at = shape_offset + i * 16;
            shapes.push(ShapeBinding {
                body_index: u32_at(bytes, at),
                local_shape_index: u32_at(bytes, at + 4),
                object_index: u32_at(bytes, at + 8),
                flags: u32_at(bytes, at + 12),
            });
        }
        let binding = |offset: usize, i: usize| Binding {
            object_index: u32_at(bytes, offset + i * 8),
            hash: u32_at(bytes, offset + i * 8 + 4),
        };
        let mut templates = Vec::with_capacity(template_count);
        for i in 0..template_count {
            templates.push(binding(template_offset, i));
        }
        let materials = (0..material_count).map(|i| binding(material_offset, i)).collect();

        Ok(Self {
            version,
            unknown_1,
            free_joint_count,
            root_transform,
            dependencies: hex_encode(&bytes[..dep_size]),
            objects: hex_encode(&bytes[second..second + objects_size]),
            bodies,
            shapes,
            templates,
            materials,
        })
    }

    /// Writes the scene back, deriving every offset and count.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let dependencies = hex_decode(&self.dependencies)?;
        let objects = hex_decode(&self.objects)?;
        let want = layout(
            dependencies.len(),
            objects.len(),
            self.bodies.len(),
            self.shapes.len(),
            self.templates.len(),
            self.materials.len(),
        );
        let mut out = vec![0u8; want.total];
        out[..dependencies.len()].copy_from_slice(&dependencies);
        out[want.second..want.second + objects.len()].copy_from_slice(&objects);
        for (i, body) in self.bodies.iter().enumerate() {
            let at = want.body + i * BODY;
            out[at..at + 4].copy_from_slice(&body.object_index.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&body.name_hash.to_le_bytes());
            out[at + 8..at + 12].copy_from_slice(&body.node_hash.to_le_bytes());
            out[at + 12..at + 16].copy_from_slice(&body.enabled.to_le_bytes());
            for (k, value) in body.velocity.iter().enumerate() {
                out[at + 16 + k * 4..at + 20 + k * 4].copy_from_slice(&value.to_le_bytes());
            }
            for (k, word) in body.lookups.iter().enumerate() {
                out[at + 40 + k * 4..at + 44 + k * 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        for (i, shape) in self.shapes.iter().enumerate() {
            let at = want.shape + i * 16;
            for (k, value) in [
                shape.body_index,
                shape.local_shape_index,
                shape.object_index,
                shape.flags,
            ]
            .iter()
            .enumerate()
            {
                out[at + k * 4..at + k * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        for (i, binding) in self.templates.iter().enumerate() {
            let at = want.template + i * 8;
            out[at..at + 4].copy_from_slice(&binding.object_index.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&binding.hash.to_le_bytes());
        }
        for (i, binding) in self.materials.iter().enumerate() {
            let at = want.material + i * 8;
            out[at..at + 4].copy_from_slice(&binding.object_index.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&binding.hash.to_le_bytes());
        }

        let h = want.total - PHYSICS_HEADER;
        let header: [u32; 32] = {
            let mut words = [0u32; 32];
            for (i, value) in self.root_transform.iter().enumerate() {
                words[4 + i] = value.to_bits();
            }
            words[0] = self.version;
            words[1] = self.unknown_1;
            words[2] = 0;
            words[3] = dependencies.len() as u32;
            words[20] = want.second as u32;
            words[21] = objects.len() as u32;
            words[22] = want.body as u32;
            words[23] = self.bodies.len() as u32;
            words[24] = want.body as u32;
            words[25] = self.free_joint_count;
            words[26] = want.shape as u32;
            words[27] = self.shapes.len() as u32;
            words[28] = want.template as u32;
            words[29] = self.templates.len() as u32;
            words[30] = want.material as u32;
            words[31] = self.materials.len() as u32;
            words
        };
        for (i, value) in header.iter().enumerate() {
            out[h + i * 4..h + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        Ok(out)
    }

    /// The scene as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the scene from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// A scene with one 6-byte dependency block, one 10-byte collection, one body,
    /// one shape, one template and one material - the sizes chosen so the 128
    /// alignment actually pads.
    fn sample() -> PhysicsScene {
        // The body's lookup word points at the body table, which the layout decides.
        let body = layout(6, 10, 1, 1, 1, 1).body as u32;
        PhysicsScene {
            version: 41200,
            unknown_1: 0,
            free_joint_count: 0,
            root_transform: std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 }),
            dependencies: "010203040506".to_string(),
            objects: "0a0b0c0d0e0f10111213".to_string(),
            bodies: vec![Body {
                object_index: 7,
                name_hash: 0x11223344,
                node_hash: 0x55667788,
                enabled: 1,
                velocity: [0.0; 6],
                lookups: [body, 0, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff],
            }],
            shapes: vec![ShapeBinding {
                body_index: 0,
                local_shape_index: 0,
                object_index: 7,
                flags: 6,
            }],
            templates: vec![Binding {
                object_index: 7,
                hash: 0x99aabbcc,
            }],
            materials: vec![Binding {
                object_index: 7,
                hash: 0xddeeff00,
            }],
        }
    }

    #[test]
    fn a_sample_scene_round_trips() {
        let scene = sample();
        let bytes = scene.to_binary().unwrap();
        // Six dependency bytes pad to 128, so the second collection starts there.
        assert_eq!(word_at(&bytes, 0), 41200, "the version word");
        assert_eq!(word_at(&bytes, 3), 6, "the dependency block starts at 0");
        assert_eq!(word_at(&bytes, 20), 128, "the second collection is 128-aligned");
        let read = PhysicsScene::from_binary(&bytes).expect("the sample parses");
        assert_eq!(read, scene, "the model round-trips");
        assert_eq!(read.to_binary().unwrap(), bytes, "byte-for-byte");
        // The body's own lookups carry the body table's offset.
        let want = layout(6, 10, 1, 1, 1, 1);
        assert_eq!(read.bodies[0].lookups[0] as usize, want.body);
    }

    #[test]
    fn a_scene_may_bind_more_shapes_than_templates() {
        // Five shipped scenes do: four shape bindings sharing two templates.
        let mut scene = sample();
        scene.shapes.push(ShapeBinding {
            body_index: 0,
            local_shape_index: 1,
            object_index: 7,
            flags: 6,
        });
        let bytes = scene.to_binary().unwrap();
        let read = PhysicsScene::from_binary(&bytes).expect("the sample parses");
        assert_eq!(read.shapes.len(), 2);
        assert_eq!(read.templates.len(), 1, "the two counts are separate");
        assert_eq!(read.to_binary().unwrap(), bytes, "byte-for-byte");
    }

    fn word_at(bytes: &[u8], index: usize) -> u32 {
        let h = bytes.len() - PHYSICS_HEADER;
        u32_at(bytes, h + index * 4)
    }

    #[test]
    fn a_sample_scene_survives_sjson() {
        let scene = sample();
        let sjson = scene.to_sjson().unwrap();
        assert_eq!(PhysicsScene::from_sjson(&sjson).unwrap(), scene);
    }

    #[test]
    fn a_header_that_does_not_describe_the_blob_is_rejected() {
        let bytes = sample().to_binary().unwrap();
        let mut broken = bytes.clone();
        let h = broken.len() - PHYSICS_HEADER;
        // Move the shape table without moving anything else.
        broken[h + 104..h + 108].copy_from_slice(&64u32.to_le_bytes());
        assert!(PhysicsScene::from_binary(&broken).is_err());

        let mut short = bytes;
        short.truncate(short.len() - 4);
        assert!(PhysicsScene::from_binary(&short).is_err());
    }
}
