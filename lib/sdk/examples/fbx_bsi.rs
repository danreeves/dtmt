//! Converts a binary FBX into the `.bsi` geometry `dtmt build` compiles a unit
//! from.
//!
//! A unit is authored as `<name>.unit` (the SJSON a mod edits) plus `<name>.bsi`
//! (the geometry: vertex streams, triangle lists and the node hierarchy). The
//! game's tooling exports the BSI from a DCC tool; this example reads the FBX
//! the DCC tool writes instead, so a mesh that only exists as an FBX can be
//! brought over without one.
//!
//! ```text
//! cargo run -p sdk --example fbx_bsi -- <file.fbx> <out.bsi> [--material <name>]
//! cargo run -p sdk --example fbx_bsi -- --dump <file.fbx>
//! ```
//!
//! What it reads, and what it writes:
//!
//! - `Objects/Geometry`: `Vertices` (the positions), `PolygonVertexIndex` (the
//!   triangles; a negative index ends a polygon), and the normal, tangent,
//!   binormal and UV layers. The layer mapping information decides whether a
//!   layer is per-vertex or per-polygon-vertex; the streams are written the way
//!   the shipped files write them, with one index list per stream.
//! - `Objects/Model`: the name and the local transform (`Lcl Translation`,
//!   `Lcl Rotation` in degrees, `Lcl Scaling`), composed into the node's `local`.
//! - `Connections`: which model owns which geometry.
//!
//! The BSI is written as the same SJSON dialect the shipped files use, so it
//! stays readable and diffs cleanly.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use color_eyre::eyre::{Result, bail};
use flate2::read::ZlibDecoder;
use std::io::Read;

/// One node of an FBX file: a name, its properties and its children.
#[derive(Debug, Default)]
struct FbxNode {
    name: String,
    properties: Vec<Property>,
    children: Vec<FbxNode>,
}

/// One FBX property.
#[derive(Debug, Clone)]
enum Property {
    Bool(bool),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Text(String),
    F32Array(Vec<f32>),
    F64Array(Vec<f64>),
    I32Array(Vec<i32>),
    I64Array(Vec<i64>),
    BoolArray(Vec<bool>),
    Raw(Vec<u8>),
}

impl Property {
    fn as_f64_array(&self) -> Option<Vec<f64>> {
        match self {
            Property::F64Array(values) => Some(values.clone()),
            Property::F32Array(values) => Some(values.iter().map(|v| *v as f64).collect()),
            _ => None,
        }
    }

    fn as_i32_array(&self) -> Option<Vec<i32>> {
        match self {
            Property::I32Array(values) => Some(values.clone()),
            Property::I64Array(values) => Some(values.iter().map(|v| *v as i32).collect()),
            _ => None,
        }
    }

    fn as_text(&self) -> Option<&str> {
        match self {
            Property::Text(text) => Some(text),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Property::F64(value) => Some(*value),
            Property::F32(value) => Some(*value as f64),
            Property::I32(value) => Some(*value as f64),
            Property::I64(value) => Some(*value as f64),
            _ => None,
        }
    }
}

impl FbxNode {
    fn child(&self, name: &str) -> Option<&FbxNode> {
        self.children.iter().find(|child| child.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a FbxNode> {
        self.children.iter().filter(move |child| child.name == name)
    }
}

/// A cursor over the file's bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8> {
        let value = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| color_eyre::eyre::eyre!("the file ended"))?;
        self.at += 1;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32> {
        let end = self.at + 4;
        let value = u32::from_le_bytes(
            self.bytes
                .get(self.at..end)
                .ok_or_else(|| color_eyre::eyre::eyre!("the file ended"))?
                .try_into()
                .unwrap(),
        );
        self.at = end;
        Ok(value)
    }

    fn u64(&mut self) -> Result<u64> {
        let end = self.at + 8;
        let value = u64::from_le_bytes(
            self.bytes
                .get(self.at..end)
                .ok_or_else(|| color_eyre::eyre::eyre!("the file ended"))?
                .try_into()
                .unwrap(),
        );
        self.at = end;
        Ok(value)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.at + length;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| color_eyre::eyre::eyre!("the file ended"))?;
        self.at = end;
        Ok(bytes)
    }
}

/// Reads an FBX property of the given type code.
fn read_property(reader: &mut Reader, kind: u8) -> Result<Property> {
    Ok(match kind {
        b'C' => Property::Bool(reader.u8()? != 0),
        b'Y' => Property::I16(i16::from_le_bytes([reader.u8()?, reader.u8()?])),
        b'I' => Property::I32(reader.u32()? as i32),
        b'L' => Property::I64(reader.u64()? as i64),
        b'F' => Property::F32(f32::from_bits(reader.u32()?)),
        b'D' => Property::F64(f64::from_bits(reader.u64()?)),
        b'S' => {
            let length = reader.u32()? as usize;
            Property::Text(String::from_utf8_lossy(reader.take(length)?).into_owned())
        }
        b'R' => {
            let length = reader.u32()? as usize;
            Property::Raw(reader.take(length)?.to_vec())
        }
        b'f' | b'd' | b'l' | b'i' | b'b' => {
            let length = reader.u32()? as usize;
            let encoding = reader.u32()?;
            let compressed = reader.u32()? as usize;
            let data = reader.take(compressed)?;
            let raw: Vec<u8> = if encoding == 1 {
                let mut decoded = Vec::new();
                ZlibDecoder::new(data).read_to_end(&mut decoded)?;
                decoded
            } else {
                data.to_vec()
            };
            let element = match kind {
                b'f' | b'i' | b'b' => 4,
                _ => 8,
            };
            if raw.len() < length * element {
                bail!("an FBX array is short");
            }
            match kind {
                b'f' => Property::F32Array(
                    (0..length)
                        .map(|index| f32::from_bits(u32::from_le_bytes(raw[index * 4..index * 4 + 4].try_into().unwrap())))
                        .collect(),
                ),
                b'd' => Property::F64Array(
                    (0..length)
                        .map(|index| f64::from_bits(u64::from_le_bytes(raw[index * 8..index * 8 + 8].try_into().unwrap())))
                        .collect(),
                ),
                b'i' => Property::I32Array(
                    (0..length)
                        .map(|index| i32::from_le_bytes(raw[index * 4..index * 4 + 4].try_into().unwrap()))
                        .collect(),
                ),
                b'l' => Property::I64Array(
                    (0..length)
                        .map(|index| i64::from_le_bytes(raw[index * 8..index * 8 + 8].try_into().unwrap()))
                        .collect(),
                ),
                _ => Property::BoolArray(raw.iter().take(length).map(|byte| *byte != 0).collect()),
            }
        }
        other => bail!("an FBX property has the unknown type '{}'", other as char),
    })
}

/// Reads the FBX node records until `end` (or the end of the file).
fn read_nodes(reader: &mut Reader, version: u32, end: usize) -> Result<Vec<FbxNode>> {
    let mut nodes = Vec::new();
    let wide = version >= 7500;
    while reader.at < end {
        let record_end = if wide {
            reader.u64()? as usize
        } else {
            reader.u32()? as usize
        };
        let property_count = if wide {
            reader.u64()? as usize
        } else {
            reader.u32()? as usize
        };
        let property_length = if wide {
            reader.u64()? as usize
        } else {
            reader.u32()? as usize
        };
        let name_length = reader.u8()? as usize;
        // The null record that closes a list is all zeroes.
        if record_end == 0 && property_count == 0 && property_length == 0 && name_length == 0 {
            break;
        }
        let name = String::from_utf8_lossy(reader.take(name_length)?).into_owned();

        let properties_start = reader.at;
        let mut properties = Vec::with_capacity(property_count);
        for _ in 0..property_count {
            let kind = reader.u8()?;
            properties.push(read_property(reader, kind)?);
        }
        if reader.at != properties_start + property_length {
            bail!("the property list of '{name}' is {property_length} bytes, read {}", reader.at - properties_start);
        }

        let children = read_nodes(reader, version, record_end)?;
        reader.at = record_end;
        nodes.push(FbxNode {
            name,
            properties,
            children,
        });
    }
    Ok(nodes)
}

/// Parses an FBX file into its top-level nodes.
fn parse_fbx(bytes: &[u8]) -> Result<(u32, Vec<FbxNode>)> {
    if bytes.len() < 27 || &bytes[..18] != b"Kaydara FBX Binary" {
        bail!("not a binary FBX file");
    }
    let version = u32::from_le_bytes(bytes[23..27].try_into().unwrap());
    if version == 0 || version > 100_000 {
        bail!("the FBX version word is {version}");
    }
    let mut reader = Reader { bytes, at: 27 };
    let nodes = read_nodes(&mut reader, version, bytes.len())?;
    Ok((version, nodes))
}

/// The pieces of an FBX mesh the BSI needs.
struct Mesh {
    name: String,
    material: String,
    positions: Vec<f64>,
    triangles: Vec<i32>,
    normals: Option<(Vec<f64>, Vec<i32>)>,
    uvs: Option<(Vec<f64>, Vec<i32>)>,
    local: [f32; 16],
}

/// The conversion from the FBX's space to the engine's: the file's unit
/// (centimeters times `UnitScaleFactor`) to meters, and the up axis. The FBX
/// is Y-up; the engine's BSI space is Blender's - Z-up, meters, column-major
/// matrices - which the Blender tools export unchanged.
#[derive(Clone, Copy)]
struct Space {
    scale: f64,
    /// For each engine axis, the FBX axis it comes from and its sign.
    axis: [(usize, f64); 3],
}

impl Space {
    /// Reads `GlobalSettings`: the unit scale factor and the up axis.
    fn from_fbx(nodes: &[FbxNode]) -> Result<Self> {
        let settings = nodes.iter().find(|node| node.name == "GlobalSettings");
        let property = |name: &str| -> Option<f64> {
            let properties = settings?.child("Properties70")?;
            properties
                .children
                .iter()
                .find(|property| {
                    property.properties.first().and_then(Property::as_text) == Some(name)
                })
                .and_then(|property| {
                    property.properties.iter().filter_map(Property::as_f64).next()
                })
        };

        let up_axis = property("UpAxis").unwrap_or(1.0);
        let up_sign = property("UpAxisSign").unwrap_or(1.0);
        if up_axis != 1.0 {
            bail!(
                "the FBX's up axis is {up_axis}, only Y-up files (what Blender and \
                 Maya export) are converted"
            );
        }
        if up_sign < 0.0 {
            bail!("the FBX's up axis sign is {up_sign}");
        }

        Ok(Self {
            scale: property("UnitScaleFactor").unwrap_or(1.0) / 100.0,
            // (x, y, z) -> (x, -z, y), the inverse of the Blender exporter's
            // Z-up to Y-up rotation.
            axis: [(0, 1.0), (2, -1.0), (1, 1.0)],
        })
    }

    /// Converts a position: the axis change and the unit scale.
    fn point(&self, v: [f64; 3]) -> [f64; 3] {
        self.direction([v[0] * self.scale, v[1] * self.scale, v[2] * self.scale])
    }

    /// Converts a direction (a normal): the axis change only.
    fn direction(&self, v: [f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (engine, (fbx, sign)) in self.axis.iter().enumerate() {
            out[engine] = sign * v[*fbx];
        }
        out
    }
}

fn matrix_from(model: &FbxNode, space: &Space) -> [f32; 16] {
    let mut translation = [0.0f64; 3];
    let mut rotation = [0.0f64; 3];
    let mut scaling = [1.0f64; 3];
    if let Some(properties) = model.child("Properties70") {
        for property in &properties.children {
            let Some(Property::Text(name)) = property.properties.first() else {
                continue;
            };
            let values: Vec<f64> = property.properties[4.min(property.properties.len())..]
                .iter()
                .filter_map(|value| value.as_f64())
                .collect();
            if values.len() < 3 {
                continue;
            }
            match name.as_str() {
                "Lcl Translation" => translation.copy_from_slice(&values[..3]),
                "Lcl Rotation" => rotation.copy_from_slice(&values[..3]),
                "Lcl Scaling" => scaling.copy_from_slice(&values[..3]),
                _ => {}
            }
        }
    }

    // The FBX default rotation order is XYZ, in degrees.
    let (sx, cx) = rotation[0].to_radians().sin_cos();
    let (sy, cy) = rotation[1].to_radians().sin_cos();
    let (sz, cz) = rotation[2].to_radians().sin_cos();
    let rotation = [
        [cy * cz, cz * sx * sy - cx * sz, cx * cz * sy + sx * sz],
        [cy * sz, cx * cz + sx * sy * sz, -cz * sx + cx * sy * sz],
        [-sy, cy * sx, cx * cy],
    ];

    // The local matrix is `rotation * scaling` with the translation in the
    // last column, and the BSI stores it column-major.
    let mut matrix = [0.0f32; 16];
    for (column, (fbx_column, column_sign)) in space.axis.iter().enumerate() {
        for (row, (fbx_row, row_sign)) in space.axis.iter().enumerate() {
            matrix[column * 4 + row] = (rotation[*fbx_row][*fbx_column]
                * scaling[*fbx_column]
                * row_sign
                * column_sign) as f32;
        }
        matrix[column * 4 + 3] = (translation[*fbx_column] * column_sign * space.scale) as f32;
    }
    matrix[15] = 1.0;
    matrix
}

/// Reads the geometry layers a mesh node carries.
fn read_mesh(geometry: &FbxNode, name: String, local: [f32; 16], space: &Space) -> Result<Mesh> {
    let positions = geometry
        .child("Vertices")
        .and_then(|node| node.properties.first())
        .and_then(Property::as_f64_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("the geometry has no vertices"))?;
    let positions: Vec<f64> = positions
        .chunks_exact(3)
        .flat_map(|vertex| space.point([vertex[0], vertex[1], vertex[2]]))
        .collect();
    let triangles = geometry
        .child("PolygonVertexIndex")
        .and_then(|node| node.properties.first())
        .and_then(Property::as_i32_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("the geometry has no polygon index"))?;
    // The FBX marks the last index of each polygon by taking the bitwise
    // complement of the vertex index, so the raw list is not the triangle list.
    let triangles: Vec<i32> = triangles
        .iter()
        .map(|index| if *index < 0 { !*index } else { *index })
        .collect();

    let layer = |element: &str, data: &str, indices: &str| -> Option<(Vec<f64>, Vec<i32>)> {
        let layer = geometry.child(element)?;
        let values = layer
            .child(data)
            .and_then(|node| node.properties.first())
            .and_then(Property::as_f64_array)?;
        let index = layer
            .child(indices)
            .and_then(|node| node.properties.first())
            .and_then(Property::as_i32_array)
            .unwrap_or_default();
        Some((values, index))
    };

    // A normal is a direction: the axis change applies, the unit scale does
    // not.
    let normals = layer("LayerElementNormal", "Normals", "NormalsIndex").map(|(values, index)| {
        let converted: Vec<f64> = values
            .chunks_exact(3)
            .flat_map(|normal| space.direction([normal[0], normal[1], normal[2]]))
            .collect();
        (converted, index)
    });
    // The BSI's V origin is the engine's, which is the one the Blender tools
    // flip Blender's UVs into.
    let uvs = layer("LayerElementUV", "UV", "UVIndex").map(|(values, index)| {
        let flipped: Vec<f64> = values
            .chunks_exact(2)
            .flat_map(|uv| [uv[0], 1.0 - uv[1]])
            .collect();
        (flipped, index)
    });

    Ok(Mesh {
        name,
        material: String::new(),
        positions,
        triangles,
        normals,
        uvs,
        local,
    })
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut dump = false;
    let mut material: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--dump" => dump = true,
            "--material" => {
                index += 1;
                material = args.get(index).cloned();
            }
            other => positional.push(other.to_string()),
        }
        index += 1;
    }
    let Some(path) = positional.first() else {
        bail!("usage: fbx_bsi <file.fbx> [out.bsi] [--material <name>] [--dump]");
    };
    let bytes = fs::read(path)?;
    let (version, nodes) = parse_fbx(&bytes)?;

    if dump {
        println!("FBX version {version}");
        fn print(node: &FbxNode, depth: usize) {
            let properties = node
                .properties
                .iter()
                .map(|property| match property {
                    Property::Text(text) => format!("\"{text}\""),
                    Property::F64(value) => format!("{value}"),
                    Property::F32(value) => format!("{value}"),
                    Property::I32(value) => format!("{value}"),
                    Property::I64(value) => format!("{value}"),
                    Property::F32Array(values) => format!("[f32 x{}]", values.len()),
                    Property::F64Array(values) => format!("[f64 x{}]", values.len()),
                    Property::I32Array(values) => format!("[i32 x{}]", values.len()),
                    Property::I64Array(values) => format!("[i64 x{}]", values.len()),
                    Property::BoolArray(values) => format!("[bool x{}]", values.len()),
                    Property::Bool(value) => format!("{value}"),
                    Property::I16(value) => format!("{value}"),
                    Property::Raw(raw) => format!("[raw x{}]", raw.len()),
                })
                .collect::<Vec<_>>()
                .join(" ");
            println!("{}{} {}", "  ".repeat(depth), node.name, properties);
            for child in &node.children {
                print(child, depth + 1);
            }
        }
        for node in &nodes {
            print(node, 0);
        }
        return Ok(());
    }

    let Some(out) = positional.get(1) else {
        bail!("the output path is missing");
    };

    // The model nodes and the connections: a geometry belongs to the model that
    // a `Connections` record points at, and the material's name is the one the
    // unit's slot uses.
    let objects = nodes
        .iter()
        .find(|node| node.name == "Objects")
        .ok_or_else(|| color_eyre::eyre::eyre!("the FBX has no Objects section"))?;
    let connections: Vec<&FbxNode> = nodes
        .iter()
        .find(|node| node.name == "Connections")
        .map(|node| node.children_named("C").collect())
        .unwrap_or_default();

    let mut models: BTreeMap<i64, &FbxNode> = BTreeMap::new();
    let mut material_names: BTreeMap<i64, String> = BTreeMap::new();
    for model in objects.children_named("Model") {
        if let Some(Property::I64(id)) = model.properties.first() {
            models.insert(*id, model);
        }
    }
    for node in objects.children_named("Material") {
        if let Some(Property::I64(id)) = node.properties.first() {
            let name = node
                .properties
                .get(1)
                .and_then(Property::as_text)
                .unwrap_or("Material")
                .split('\u{0}')
                .next()
                .unwrap_or("Material")
                .to_string();
            material_names.insert(*id, name);
        }
    }
    let parent_of = |id: i64| -> Option<i64> {
        connections.iter().find_map(|connection| {
            let ids: Vec<i64> = connection
                .properties
                .iter()
                .filter_map(|property| match property {
                    Property::I64(value) => Some(*value),
                    _ => None,
                })
                .collect();
            (ids.len() >= 2 && ids[0] == id).then_some(ids[1])
        })
    };

    let space = Space::from_fbx(&nodes)?;

    let mut meshes = Vec::new();
    for geometry in objects.children_named("Geometry") {
        let Some(Property::I64(geometry_id)) = geometry.properties.first() else {
            continue;
        };
        let Some(model_id) = parent_of(*geometry_id) else {
            continue;
        };
        let Some(model) = models.get(&model_id).copied() else {
            continue;
        };
        let model_name = model
            .properties
            .get(1)
            .and_then(Property::as_text)
            .unwrap_or("mesh")
            .split('\u{0}')
            .next()
            .unwrap_or("mesh")
            .to_string();
        // The material the unit's slot names: the `Material` node connected to
        // this model, or the model's name.
        let name = material.clone().unwrap_or_else(|| {
            connections
                .iter()
                .find_map(|connection| {
                    let ids: Vec<i64> = connection
                        .properties
                        .iter()
                        .filter_map(|property| match property {
                            Property::I64(value) => Some(*value),
                            _ => None,
                        })
                        .collect();
                    (ids.len() >= 2 && ids[1] == model_id)
                        .then(|| material_names.get(&ids[0]).cloned())
                        .flatten()
                })
                .unwrap_or_else(|| model_name.clone())
        });
        let mut mesh = read_mesh(geometry, model_name, matrix_from(model, &space), &space)?;
        mesh.material = name;
        meshes.push(mesh);
    }

    if meshes.is_empty() {
        bail!("the FBX has no mesh a model owns");
    }

    let mut out_text = String::new();
    out_text.push_str("geometries = {\n");
    for mesh in &meshes {
        let corners = mesh.triangles.len();
        let vertices = mesh.positions.len() / 3;
        out_text.push_str(&format!("\t{} = {{\n", mesh.name));
        out_text.push_str("\t\tindices = {\n");
        out_text.push_str(&format!("\t\t\tsize = {corners}\n"));

        // The compiler pairs each stream with the index list at the same
        // position, and gathers the streams: one index list per stream, each
        // indexing its own array.
        let mut lists: Vec<Vec<i32>> = vec![mesh.triangles.clone()];
        if let Some((data, index)) = &mesh.normals {
            lists.push(attribute_indices(
                data.len() / 3,
                vertices,
                corners,
                index,
                &mesh.triangles,
            ));
        }
        if let Some((data, index)) = &mesh.uvs {
            lists.push(attribute_indices(
                data.len() / 2,
                vertices,
                corners,
                index,
                &mesh.triangles,
            ));
        }
        // COLOR is per vertex, so it reuses the position list. The stream is
        // always written: the standard material's shader declares the channel,
        // and a unit that does not provide it fails to build its pipeline
        // state (the game crashes with E_INVALIDARG on the shader).
        lists.push(mesh.triangles.clone());

        out_text.push_str("\t\t\tstreams = [\n");
        for list in &lists {
            out_text.push_str("\t\t\t\t[\n\t\t\t\t\t");
            for (index, value) in list.iter().enumerate() {
                out_text.push_str(&format!("{value} "));
                if index % 16 == 15 {
                    out_text.push_str("\n\t\t\t\t\t");
                }
            }
            out_text.push_str("\n\t\t\t\t]\n");
        }
        out_text.push_str("\t\t\t]\n");
        out_text.push_str("\t\t\ttype = \"TRIANGLE_LIST\"\n");
        out_text.push_str("\t\t}\n");

        let triangles = corners / 3;
        out_text.push_str("\t\tmaterials = [ {\n");
        out_text.push_str(&format!("\t\t\t\tname = \"{}\"\n", mesh.material));
        out_text.push_str("\t\t\t\tprimitives = [\n\t\t\t\t\t");
        for index in 0..triangles {
            out_text.push_str(&format!("{index} "));
            if index % 16 == 15 {
                out_text.push_str("\n\t\t\t\t\t");
            }
        }
        out_text.push_str("\n\t\t\t\t]\n\t\t\t} ]\n");
        let mut streams: Vec<String> = Vec::new();
        let mut write_stream =
            |name: &str, kind: &str, stride: u32, components: usize, data: &[f64]| {
                let size = data.len() / components;
                let mut block = String::new();
                block.push_str("\t\t\t{\n");
                block.push_str("\t\t\t\tchannels = [ {\n");
                block.push_str("\t\t\t\t\t\tindex = 0\n");
                block.push_str(&format!("\t\t\t\t\t\tname = \"{name}\"\n"));
                block.push_str(&format!("\t\t\t\t\t\ttype = \"{kind}\"\n"));
                block.push_str("\t\t\t\t\t} ]\n");
                block.push_str("\t\t\t\tdata = [\n\t\t\t\t\t");
                for (index, value) in data.iter().enumerate() {
                    block.push_str(&format!("{value:.6} "));
                    if index % 12 == 11 {
                        block.push_str("\n\t\t\t\t\t");
                    }
                }
                block.push_str("\n\t\t\t\t]\n");
                block.push_str(&format!("\t\t\t\tsize = {size}\n\t\t\t\tstride = {stride}\n"));
                block.push_str("\t\t\t}");
                streams.push(block);
            };

        write_stream("POSITION", "CT_FLOAT3", 12, 3, &mesh.positions);
        if let Some((data, _)) = &mesh.normals {
            write_stream("NORMAL", "CT_FLOAT3", 12, 3, data);
        }
        if let Some((data, _)) = &mesh.uvs {
            write_stream("TEXCOORD", "CT_FLOAT2", 8, 2, data);
        }
        let white: Vec<f64> = (0..vertices).flat_map(|_| [1.0, 1.0, 1.0, 1.0]).collect();
        write_stream("COLOR", "CT_FLOAT4", 16, 4, &white);

        out_text.push_str("\t\tstreams = [\n");
        // A separator between the streams, and none before the closing
        // bracket: the game's own exporter writes its lists that way.
        out_text.push_str(&streams.join(",\n"));
        out_text.push_str("\n\t\t]\n\t}\n");
    }
    out_text.push_str("}\n");

    out_text.push_str("nodes = {\n");
    for mesh in &meshes {
        out_text.push_str(&format!("\t{} = {{\n", mesh.name));
        out_text.push_str(&format!("\t\tgeometries = [ \"{}\" ]\n", mesh.name));
        out_text.push_str("\t\tlocal = [ ");
        for value in mesh.local {
            out_text.push_str(&format!("{value} "));
        }
        out_text.push_str("]\n\t}\n");
    }
    out_text.push_str("}\n");

    let out = Path::new(out);
    fs::write(out, out_text)?;
    println!(
        "wrote {} ({} mesh(es), {} triangles)",
        out.display(),
        meshes.len(),
        meshes.iter().map(|mesh| mesh.triangles.len() / 3).sum::<usize>()
    );
    Ok(())
}

/// The index list a stream needs: the layer's own index when the FBX writes one,
/// the triangle list when its data is per vertex, and a per-corner list when it
/// is per corner.
fn attribute_indices(
    count: usize,
    vertices: usize,
    corners: usize,
    index: &[i32],
    triangles: &[i32],
) -> Vec<i32> {
    if !index.is_empty() {
        return index.to_vec();
    }
    if count == vertices {
        triangles.to_vec()
    } else {
        (0..corners as i32).collect()
    }
}

