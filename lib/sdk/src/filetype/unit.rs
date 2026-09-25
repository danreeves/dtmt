//! Darktide `.unit` compilation.
//!
//! A unit is authored as two SJSON files: `<name>.unit` describes the
//! renderables, LOD steps and material slots, and `<name>.bsi` (optionally
//! zlib-wrapped in a `bsiz` container) carries the geometry: node hierarchy,
//! per-attribute vertex streams and triangle lists.
//!
//! The engine reads a compiled payload (version `0x73`) that mirrors the
//! runtime structures of the Bitsquid/Stingray lineage: mesh geometries with
//! packed vertex streams, a scene graph, mesh objects, LOD objects and a
//! material list. This module turns the authoring pair into that payload.

use std::collections::BTreeMap;
use std::io::Read;

use color_eyre::eyre::{self, bail, Context as _, Result};
use serde::Deserialize;

use crate::murmur::{IdString64, Murmur32, Murmur64};
use crate::{BundleFile, BundleFileType, BundleFileVariant};

/// The compiled unit payload begins with this version word.
pub const UNIT_VERSION: u32 = 0x73;

#[derive(Debug, Deserialize)]
struct LodDef {
    name: String,
    #[serde(default)]
    bounding_volume: Option<String>,
    #[serde(default)]
    orientation: Option<String>,
    steps: Vec<StepDef>,
}

#[derive(Debug, Deserialize)]
struct StepDef {
    renderables: Vec<String>,
    visible_height_range: [f32; 2],
}

/// Renderable flags; the compiled form only needs the name and its geometry.
#[derive(Debug, Deserialize)]
struct RenderableDef {}

#[derive(Debug, Deserialize)]
struct CameraSettings {
    #[serde(default)]
    far_range: f32,
    #[serde(default)]
    interest_point_distance: f32,
    #[serde(default)]
    orthographic_plane: Vec<f32>,
    #[serde(default)]
    orthographic_zoom: f32,
    #[serde(default)]
    position: Vec<f32>,
    #[serde(default)]
    projection_type: String,
    #[serde(default)]
    rotation: Vec<f32>,
    #[serde(default)]
    rotation_speed: f32,
    #[serde(default)]
    translation_speed: f32,
}

#[derive(Debug, Deserialize)]
struct FlowFraming {
    #[serde(default)]
    scale: f32,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
}

/// Editor-only metadata. Declared so its float values parse with the typed
/// reader: unknown values would be skipped generically, and the SJSON tokenizer
/// cannot skip a float (it tokenizes `4.579212` as `4` plus leftovers).
#[derive(Debug, Deserialize)]
struct EditorMetadata {
    #[serde(default)]
    camera_settings: Option<CameraSettings>,
    #[serde(default)]
    flow_framing: Option<FlowFraming>,
}

#[derive(Debug, Deserialize)]
struct Light {
    #[serde(default)]
    box_max: Vec<f32>,
    #[serde(default)]
    box_min: Vec<f32>,
    #[serde(default)]
    cast_shadows: bool,
    #[serde(default)]
    color: Vec<f32>,
    #[serde(default)]
    falloff_end: f32,
    #[serde(default)]
    falloff_exponent: f32,
    #[serde(default)]
    falloff_start: f32,
    #[serde(default)]
    node: String,
    #[serde(default)]
    spot_angle_end: f32,
    #[serde(default)]
    spot_angle_start: f32,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct UnitDef {
    materials: BTreeMap<String, String>,
    #[serde(default)]
    lod: Vec<LodDef>,
    renderables: BTreeMap<String, RenderableDef>,
    #[serde(default)]
    editor_metadata: Option<EditorMetadata>,
    #[serde(default)]
    lights: BTreeMap<String, Light>,
}

#[derive(Debug, Deserialize)]
struct BsiDef {
    geometries: BTreeMap<String, BsiGeometry>,
    #[serde(default)]
    nodes: BTreeMap<String, BsiNode>,
    /// Declared so animation float streams parse typed; they are not compiled.
    #[serde(default)]
    animations: Vec<BsiAnimation>,
    #[serde(default)]
    source_path: String,
}

#[derive(Debug, Deserialize)]
struct BsiGeometry {
    indices: BsiIndices,
    streams: Vec<BsiStream>,
    #[serde(default)]
    materials: Vec<BsiMaterial>,
}

#[derive(Debug, Deserialize)]
struct BsiIndices {
    size: u32,
    streams: Vec<Vec<u32>>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct BsiStream {
    channels: Vec<BsiChannel>,
    data: Vec<f32>,
    size: u32,
    stride: u32,
}

#[derive(Debug, Deserialize)]
struct BsiChannel {
    #[serde(default)]
    index: u32,
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct BsiAnimationChannel {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct BsiAnimationStream {
    #[serde(default)]
    channels: Vec<BsiAnimationChannel>,
    #[serde(default)]
    data: Vec<f32>,
    #[serde(default)]
    size: u32,
    #[serde(default)]
    stride: u32,
}

#[derive(Debug, Deserialize)]
struct BsiAnimation {
    #[serde(default)]
    node: String,
    #[serde(default)]
    parameter: String,
    #[serde(default)]
    stream: Option<BsiAnimationStream>,
    #[serde(default)]
    times: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct BsiMaterial {
    name: String,
    #[serde(default)]
    primitives: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct BsiNode {
    #[serde(default)]
    children: BTreeMap<String, BsiNode>,
    #[serde(default)]
    geometries: Vec<String>,
    local: [f32; 16],
    #[serde(default)]
    parent: Option<String>,
}

/// Decode a `.bsi` payload, unwrapping the optional `bsiz` zlib container.
fn decode_bsi(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(b"bsiz") {
        if bytes.len() < 8 {
            bail!("bsiz container is too short");
        }
        let mut decoded = String::new();
        flate2::read::ZlibDecoder::new(&bytes[8..])
            .read_to_string(&mut decoded)
            .wrap_err("Failed to decompress the bsiz container")?;
        Ok(decoded)
    } else {
        String::from_utf8(bytes.to_vec()).wrap_err("BSI is not valid UTF-8")
    }
}

/// The SJSON writer requires value separators (a comma or a line break) between
/// elements, while the game's own export tools also put several space-separated
/// values on one line. Insert line breaks where they are missing so both
/// dialects parse.
fn normalize_sjson(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut prev_value_end = false;
    let mut pending_space = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if prev_value_end && pending_space {
                    out.push('\n');
                }
                out.push(c);
                let mut escaped = false;
                for s in chars.by_ref() {
                    out.push(s);
                    if escaped {
                        escaped = false;
                    } else if s == '\\' {
                        escaped = true;
                    } else if s == '"' {
                        break;
                    }
                }
                prev_value_end = true;
                pending_space = false;
            }
            '/' if chars.peek() == Some(&'/') => {
                // Line comment: copy it and let the newline reset the state.
                out.push(c);
                for s in chars.by_ref() {
                    out.push(s);
                    if s == '\n' {
                        break;
                    }
                }
                prev_value_end = false;
                pending_space = false;
            }
            '\n' => {
                out.push('\n');
                prev_value_end = false;
                pending_space = false;
            }
            ' ' | '\t' | '\r' => {
                // Drop the whitespace itself; a separator is inserted when the
                // next token needs one. Leaving it in would put a space between
                // two values on separate lines (`0  \n1`), which the parser's
                // single-character horizontal whitespace rule rejects.
                pending_space = true;
            }
            ',' => {
                out.push(',');
                prev_value_end = false;
                pending_space = false;
            }
            '=' => {
                out.push('=');
                prev_value_end = false;
                pending_space = false;
            }
            ']' | '}' => {
                out.push(c);
                prev_value_end = true;
                pending_space = false;
            }
            '[' | '{' => {
                if prev_value_end && pending_space {
                    out.push('\n');
                }
                out.push(c);
                prev_value_end = false;
                pending_space = false;
            }
            _ => {
                if prev_value_end && pending_space {
                    out.push('\n');
                }
                out.push(c);
                prev_value_end = true;
                pending_space = false;
            }
        }
    }
    out
}

/// Convert an `f32` to an IEEE 754 half float.
fn f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if exponent >= 0x1f {
        return sign | 0x7c00;
    }
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        let mantissa = (mantissa | 0x80_0000) >> (1 - exponent);
        return sign | (mantissa >> 13) as u16;
    }
    sign | ((exponent as u16) << 10) | (mantissa >> 13) as u16
}

/// Octahedral normal encoding, stored as a half2 in `[0, 1]`.
fn oct_encode(n: [f32; 3]) -> [f32; 2] {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-8);
    let (mut x, mut y, z) = (n[0] / len, n[1] / len, n[2] / len);
    let l = (x.abs() + y.abs() + z.abs()).max(1e-8);
    x /= l;
    y /= l;
    if z < 0.0 {
        let nx = (1.0 - y.abs()) * if x >= 0.0 { 1.0 } else { -1.0 };
        let ny = (1.0 - x.abs()) * if y >= 0.0 { 1.0 } else { -1.0 };
        x = nx;
        y = ny;
    }
    [(x + 1.0) * 0.5, (y + 1.0) * 0.5]
}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn bool(&mut self, v: bool) {
        self.buf.push(if v { 1 } else { 0 });
    }

    fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    fn byte_array(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.bytes(v);
    }

    fn u32_array(&mut self, v: &[u32]) {
        self.u32(v.len() as u32);
        for value in v {
            self.u32(*value);
        }
    }
}

/// One compiled vertex stream (one channel per stream).
struct VertexFormat {
    component: u32,
    kind: u32,
    stride: u32,
    data: Vec<u8>,
    vertices: u32,
}

fn compile_stream(
    name: &str,
    data: &[f32],
    size: u32,
    components: usize,
) -> Result<Option<VertexFormat>> {
    if data.len() != size as usize * components {
        bail!(
            "Channel '{name}' has {} values, expected {} for {size} vertices",
            data.len(),
            size as usize * components
        );
    }
    let component = match name {
        "POSITION" => 0,
        "NORMAL" => 1,
        "COLOR" => 4,
        "TEXCOORD" => 5,
        "BLENDINDICES" => 7,
        "BLENDWEIGHTS" => 8,
        // The compiled Darktide vertex declaration has no tangent or binormal
        // component, and shipped units do not carry them either, so streams
        // that only VT2-style sources export are skipped.
        "TANGENT" | "BINORMAL" => return Ok(None),
        other => bail!("Unsupported vertex channel '{other}'"),
    };
    match name {
        // Positions are stored as half4 with w = 1.
        "POSITION" => {
            let mut out = Vec::with_capacity(size as usize * 8);
            for chunk in data.chunks_exact(components) {
                for value in [chunk[0], chunk[1], chunk[2], 1.0] {
                    out.extend_from_slice(&f16(value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat { component, kind: 17, stride: 8, data: out, vertices: size }))
        }
        // Normals are octahedral-encoded half2.
        "NORMAL" => {
            let mut out = Vec::with_capacity(size as usize * 4);
            for chunk in data.chunks_exact(components) {
                for value in oct_encode([chunk[0], chunk[1], chunk[2]]) {
                    out.extend_from_slice(&f16(value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat { component, kind: 15, stride: 4, data: out, vertices: size }))
        }
        // Texture coordinates are half2.
        "TEXCOORD" => {
            let mut out = Vec::with_capacity(size as usize * 4);
            for chunk in data.chunks_exact(components) {
                for value in &chunk[..2] {
                    out.extend_from_slice(&f16(*value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat { component, kind: 15, stride: 4, data: out, vertices: size }))
        }
        // Colors become half4.
        "COLOR" => {
            let mut out = Vec::with_capacity(size as usize * 8);
            for chunk in data.chunks_exact(components) {
                let rgba = [
                    chunk[0],
                    chunk[1],
                    chunk[2],
                    if components > 3 { chunk[3] } else { 1.0 },
                ];
                for value in rgba {
                    out.extend_from_slice(&f16(value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat { component, kind: 17, stride: 8, data: out, vertices: size }))
        }
        "BLENDINDICES" => {
            let mut out = Vec::with_capacity(size as usize * 4);
            for chunk in data.chunks_exact(components) {
                for value in chunk {
                    out.push(*value as u8);
                }
            }
            Ok(Some(VertexFormat { component, kind: 19, stride: 4, data: out, vertices: size }))
        }
        "BLENDWEIGHTS" => {
            let mut out = Vec::with_capacity(size as usize * 8);
            for chunk in data.chunks_exact(components) {
                for value in chunk {
                    out.extend_from_slice(&f16(*value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat { component, kind: 17, stride: 8, data: out, vertices: size }))
        }
        _ => unreachable!(),
    }
}

fn geometry_bounds(geometry: &BsiGeometry) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for stream in &geometry.streams {
        if stream.channels[0].name == "POSITION" {
            for chunk in stream.data.chunks_exact(3) {
                for axis in 0..3 {
                    min[axis] = min[axis].min(chunk[axis]);
                    max[axis] = max[axis].max(chunk[axis]);
                }
            }
        }
    }
    if min[0] > max[0] {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    (min, max)
}

fn write_bounding_volume(w: &mut Writer, min: [f32; 3], max: [f32; 3]) {
    for v in min {
        w.f32(v);
    }
    for v in max {
        w.f32(v);
    }
    // Four extras of unknown meaning (bounding sphere data in shipped files).
    for _ in 0..4 {
        w.f32(0.0);
    }
}

/// One vertex attribute stream after gathering.
struct UnifiedStream {
    name: String,
    components: usize,
    data: Vec<f32>,
}

impl UnifiedStream {
    fn vertices(&self) -> u32 {
        (self.data.len() / self.components.max(1)) as u32
    }
}

/// BSI attributes are indexed independently - the format stores one index list
/// per vertex stream, each indexing its own vertex array. Darktide's compiled
/// geometry instead uses one vertex per unique attribute tuple and a single
/// index list, so gather the streams: walk the corner lists together and emit a
/// vertex for every distinct tuple of per-stream indices.
fn unify_geometry(geometry: &BsiGeometry) -> Result<(Vec<UnifiedStream>, Vec<u32>)> {
    struct Source<'a> {
        name: String,
        components: usize,
        data: &'a [f32],
        size: usize,
    }

    let mut sources = Vec::new();
    for stream in &geometry.streams {
        let channel = stream
            .channels
            .first()
            .ok_or_else(|| eyre::eyre!("Geometry stream has no channels"))?;
        let components = (stream.stride / 4) as usize;
        if components == 0 {
            bail!("Channel '{}' has a zero stride", channel.name);
        }
        if stream.data.len() != stream.size as usize * components {
            bail!(
                "Channel '{}' has {} values, expected {} for {} vertices",
                channel.name,
                stream.data.len(),
                stream.size as usize * components,
                stream.size
            );
        }
        sources.push(Source {
            name: channel.name.clone(),
            components,
            data: &stream.data,
            size: stream.size as usize,
        });
    }

    // One index list shared by every stream, or one per stream.
    let lists: Vec<&Vec<u32>> = if geometry.indices.streams.len() == 1 {
        sources
            .iter()
            .map(|_| &geometry.indices.streams[0])
            .collect()
    } else if geometry.indices.streams.len() == sources.len() {
        geometry.indices.streams.iter().collect()
    } else {
        bail!(
            "Geometry has {} index lists for {} vertex streams",
            geometry.indices.streams.len(),
            sources.len()
        );
    };
    let corners = geometry.indices.size as usize;
    for (index, list) in lists.iter().enumerate() {
        if list.len() != corners {
            bail!(
                "Index list {} has {} values, expected {}",
                index,
                list.len(),
                corners
            );
        }
    }

    let mut gathered: Vec<UnifiedStream> = sources
        .iter()
        .map(|source| UnifiedStream {
            name: source.name.clone(),
            components: source.components,
            data: Vec::new(),
        })
        .collect();
    let mut seen = std::collections::HashMap::new();
    let mut indices = Vec::with_capacity(corners);
    let mut vertex_count = 0u32;
    for corner in 0..corners {
        let key: Vec<u32> = lists.iter().map(|list| list[corner]).collect();
        if let Some(&index) = seen.get(&key) {
            indices.push(index);
            continue;
        }
        for (stream_index, source) in sources.iter().enumerate() {
            let vertex = key[stream_index] as usize;
            if vertex >= source.size {
                bail!(
                    "Index {} is out of range for channel '{}' ({} vertices)",
                    vertex,
                    source.name,
                    source.size
                );
            }
            let at = vertex * source.components;
            gathered[stream_index]
                .data
                .extend_from_slice(&source.data[at..at + source.components]);
        }
        let index = vertex_count;
        seen.insert(key, index);
        indices.push(index);
        vertex_count += 1;
    }

    Ok((gathered, indices))
}

fn write_mesh_geometry(w: &mut Writer, geometry: &BsiGeometry, slot_ids: &[u32]) -> Result<()> {
    w.u32(1); // mesh geometry version
    let (streams, indices) = unify_geometry(geometry)?;
    let mut formats = Vec::new();
    for stream in &streams {
        if let Some(format) =
            compile_stream(&stream.name, &stream.data, stream.vertices(), stream.components)?
        {
            formats.push(format);
        }
    }
    w.u32(formats.len() as u32);
    for format in &formats {
        w.byte_array(&format.data);
        w.u32(0); // validity: static
        w.u32(0); // stream type: array
        w.u32(format.vertices);
        w.u32(format.stride);
    }
    // Channels, one per stream.
    w.u32(formats.len() as u32);
    for (index, format) in formats.iter().enumerate() {
        w.u32(format.component);
        w.u32(format.kind);
        w.u32(0); // channel set
        w.u32(index as u32);
        w.bool(false);
    }

    // Index stream: 16 bit when possible.
    let use32 = indices.iter().any(|i| *i > u16::MAX as u32);
    w.u32(0); // validity
    w.u32(0); // stream type
    w.u32(if use32 { 1 } else { 0 });
    w.u32(indices.len() as u32);
    let mut index_data = Vec::with_capacity(indices.len() * if use32 { 4 } else { 2 });
    for index in &indices {
        if use32 {
            index_data.extend_from_slice(&index.to_le_bytes());
        } else {
            index_data.extend_from_slice(&(*index as u16).to_le_bytes());
        }
    }
    w.byte_array(&index_data);

    // Batch ranges: one per contiguous run of triangles in a material.
    let mut batches: Vec<(u32, u32, u32)> = Vec::new();
    for (material_index, material) in geometry.materials.iter().enumerate() {
        let mut triangles = material.primitives.clone();
        triangles.sort_unstable();
        triangles.dedup();
        let mut run_start: Option<u32> = None;
        let mut previous = 0u32;
        for triangle in triangles {
            match run_start {
                None => {
                    run_start = Some(triangle);
                    previous = triangle;
                }
                Some(_) if triangle == previous + 1 => previous = triangle,
                Some(start) => {
                    batches.push((material_index as u32, start, previous - start + 1));
                    run_start = Some(triangle);
                    previous = triangle;
                }
            }
        }
        if let Some(start) = run_start {
            batches.push((material_index as u32, start, previous - start + 1));
        }
    }
    w.u32(batches.len() as u32);
    for (material_index, start, size) in &batches {
        w.u32(*material_index);
        w.u32(*start);
        w.u32(*size);
        w.u32(0); // bone set
    }

    let (min, max) = geometry_bounds(geometry);
    write_bounding_volume(w, min, max);
    w.u32_array(slot_ids);
    w.u32(0); // unknown flags
    Ok(())
}

struct FlatNode<'a> {
    name: &'a str,
    node: &'a BsiNode,
    parent: Option<usize>,
}

fn flatten_nodes<'a>(
    nodes: &'a BTreeMap<String, BsiNode>,
    parent: Option<usize>,
    out: &mut Vec<FlatNode<'a>>,
) {
    for (name, node) in nodes {
        let index = out.len();
        out.push(FlatNode { name, node, parent });
        flatten_nodes(&node.children, Some(index), out);
    }
}

fn mul4(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for column in 0..4 {
        for row in 0..4 {
            let mut value = 0.0;
            for k in 0..4 {
                value += a[k * 4 + row] * b[column * 4 + k];
            }
            out[column * 4 + row] = value;
        }
    }
    out
}

/// Compile a `.unit`/`.bsi` pair into a bundle file.
pub fn compile(name: IdString64, unit_sjson: &str, bsi: &[u8]) -> Result<BundleFile> {
    let unit_text = normalize_sjson(unit_sjson);
    let def: UnitDef =
        serde_sjson::from_str(&unit_text).wrap_err("Failed to deserialize the unit SJSON")?;
    let bsi_text = normalize_sjson(&decode_bsi(bsi)?);
    let bsi: BsiDef = serde_sjson::from_str(&bsi_text).wrap_err("Failed to deserialize the BSI")?;

    let payload = compile_payload(&def, &bsi)?;

    let mut variant = BundleFileVariant::new();
    variant.set_data(payload);
    let mut file = BundleFile::new(name, BundleFileType::Unit);
    file.add_variant(variant);
    Ok(file)
}

fn compile_payload(def: &UnitDef, bsi: &BsiDef) -> Result<Vec<u8>> {
    // Material slots: name -> resource hash.
    let mut slots: Vec<(u32, IdString64)> = Vec::new();
    for (slot, material) in &def.materials {
        let resource = if let Some(hex) = material.strip_prefix('#') {
            IdString64::from(
                u64::from_str_radix(hex, 16)
                    .wrap_err_with(|| format!("Invalid material hash '{material}'"))?,
            )
        } else {
            IdString64::from(Murmur64::hash(material))
        };
        slots.push((u32::from(Murmur32::hash(slot)), resource));
    }

    // Scene graph nodes in depth-first order.
    let flat = {
        let mut flat = Vec::new();
        flatten_nodes(&bsi.nodes, None, &mut flat);
        flat
    };
    // One mesh object per renderable.
    let mut meshes: Vec<(String, usize, usize)> = Vec::new(); // (name, geometry, node)
    for renderable in def.renderables.keys() {
        let geometry_index = bsi.geometries.keys().position(|g| g == renderable);
        let node_index = flat
            .iter()
            .position(|node| node.node.geometries.iter().any(|g| g == renderable));
        match (geometry_index, node_index) {
            (Some(geometry), Some(node)) => meshes.push((renderable.clone(), geometry, node)),
            _ => bail!("Renderable '{renderable}' has no matching geometry or node in the BSI"),
        }
    }
    let geometry_at = |index: usize| -> Result<&BsiGeometry> {
        bsi.geometries
            .values()
            .nth(index)
            .ok_or_else(|| eyre::eyre!("Geometry index {index} is out of range"))
    };

    // World transforms.
    let mut worlds = vec![[0.0f32; 16]; flat.len()];
    for (index, node) in flat.iter().enumerate() {
        let mut local = node.node.local;
        for axis in 0..3 {
            let length = (local[axis * 4] * local[axis * 4]
                + local[axis * 4 + 1] * local[axis * 4 + 1]
                + local[axis * 4 + 2] * local[axis * 4 + 2])
                .sqrt()
                .max(1e-8);
            local[axis * 4] /= length;
            local[axis * 4 + 1] /= length;
            local[axis * 4 + 2] /= length;
        }
        worlds[index] = match node.parent {
            Some(parent) => mul4(&worlds[parent], &local),
            None => local,
        };
    }

    let mut w = Writer::new();
    w.u32(UNIT_VERSION);

    // Mesh geometries.
    w.u32(bsi.geometries.len() as u32);
    for geometry in bsi.geometries.values() {
        let slot_ids: Vec<u32> = geometry
            .materials
            .iter()
            .map(|material| u32::from(Murmur32::hash(&material.name)))
            .collect();
        write_mesh_geometry(&mut w, geometry, &slot_ids)?;
    }

    // Skins, simple animation and groups.
    w.u32(0);
    w.byte_array(&[]);
    w.u32(0);

    // Scene graph: local TRS per node, then world matrices, then parents, then names.
    w.u32(flat.len() as u32);
    for node in &flat {
        let local = node.node.local;
        w.f32(local[0]);
        w.f32(local[1]);
        w.f32(local[2]);
        w.f32(local[4]);
        w.f32(local[5]);
        w.f32(local[6]);
        w.f32(local[8]);
        w.f32(local[9]);
        w.f32(local[10]);
        w.f32(local[12]);
        w.f32(local[13]);
        w.f32(local[14]);
        w.f32(1.0);
        w.f32(1.0);
        w.f32(1.0);
    }
    for world in &worlds {
        for value in world {
            w.f32(*value);
        }
    }
    for (index, node) in flat.iter().enumerate() {
        match node.parent {
            Some(parent) => {
                w.u16(1);
                w.u16(parent as u16);
            }
            None => {
                w.u16(0);
                w.u16(index as u16);
            }
        }
    }
    for node in &flat {
        w.u32(u32::from(Murmur32::hash(node.name)));
    }
    w.u32(0); // unknown list

    // Mesh objects.
    w.u32(meshes.len() as u32);
    for (renderable, geometry_index, node_index) in &meshes {
        w.u32(u32::from(Murmur32::hash(renderable)));
        w.u32(*node_index as u32);
        w.u32((geometry_index + 1) as u32);
        w.u32(0); // skin index
        // Render flags as they appear on shipped static props.
        w.u32(0x000c_2001);
        w.u32(3);
        w.u32(1);
        let (min, max) = geometry_bounds(geometry_at(*geometry_index)?);
        write_bounding_volume(&mut w, min, max);
        w.u32(0); // unknown
    }

    // Actors, actors2 and an unknown list.
    w.u32(0);
    w.u32(0);
    w.u32_array(&[]);
    // Cameras and lights.
    w.u32(0);
    w.u32(0);
    // Unknown byte array.
    w.byte_array(&[]);

    // LOD objects.
    w.u32(def.lod.len() as u32);
    for lod in &def.lod {
        w.u32(u32::from(Murmur32::hash(&lod.name)));
        w.u64(0); // unknown hash
        w.u32(0); // unknown
        w.u32(lod.steps.len() as u32);
        for (step_index, step) in lod.steps.iter().enumerate() {
            w.f32(step.visible_height_range[0]);
            w.f32(step.visible_height_range[1]);
            let mesh_indices: Vec<u32> = step
                .renderables
                .iter()
                .filter_map(|renderable| {
                    meshes
                        .iter()
                        .position(|(name, _, _)| name == renderable)
                        .map(|index| index as u32)
                })
                .collect();
            w.u32_array(&mesh_indices);
            w.u32(0); // stream offset
            w.u32(step_index as u32);
        }
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for (_, geometry_index, _) in &meshes {
            let (mesh_min, mesh_max) = geometry_bounds(geometry_at(*geometry_index)?);
            for axis in 0..3 {
                min[axis] = min[axis].min(mesh_min[axis]);
                max[axis] = max[axis].max(mesh_max[axis]);
            }
        }
        write_bounding_volume(&mut w, min, max);
        w.u32(0);
        w.u32(0);
        let order: Vec<u32> = lod
            .steps
            .iter()
            .flat_map(|step| step.renderables.iter())
            .filter_map(|renderable| {
                meshes
                    .iter()
                    .position(|(name, _, _)| name == renderable)
                    .map(|index| index as u32)
            })
            .collect();
        w.u32_array(&order);
        w.u32(0);
        w.bool(false);
        let _ = lod.bounding_volume;
        let _ = lod.orientation;
    }

    // Terrains, unknown lists, joints, movers.
    w.u32(0);
    w.u32(0);
    w.u32(0);
    w.u32(0);
    w.u32(0);
    // Animation bones flag, animation state machine, dynamic data. The
    // `ffffffff 00000000` sentinel matches shipped inline units.
    w.bool(false);
    w.byte_array(&[]);
    w.byte_array(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    // Visibility groups.
    w.u32(0);
    // Flow and flow dynamic data.
    w.byte_array(&[]);
    w.byte_array(&[]);
    // Mesh geometry triangle finder and physics scene data.
    w.byte_array(&[0u8; 4]);
    w.byte_array(&[]);
    // Default material and the material list.
    w.u64(0);
    w.u32(slots.len() as u32);
    for (slot, resource) in &slots {
        w.u32(*slot);
        w.u64(u64::from(resource.to_murmur64()));
    }
    // Unknown lists, skeleton name and the trailing unknown list.
    w.u32(0);
    w.u32(0);
    w.byte_array(&[]);
    w.u32(0);
    w.u64(0);
    w.u32(0);

    Ok(w.buf)
}

/// The bundle name of a unit resource: murmur64 of the path without extension.
pub fn resource_name(path: &str) -> IdString64 {
    IdString64::from(Murmur64::hash(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BSI: &str = r#"
geometries = {
    g_cube = {
        indices = {
            size = 3
            streams = [ [ 0 1 2 ] ]
            type = "TRIANGLE_LIST"
        }
        materials = [ {
            name = "m_cube"
            primitives = [ 0 ]
        } ]
        streams = [
            {
                channels = [ { index = 0 name = "POSITION" type = "CT_FLOAT3" } ]
                data = [ 0 0 0  1 0 0  0 1 0 ]
                size = 3
                stride = 12
            }
            {
                channels = [ { index = 0 name = "NORMAL" type = "CT_FLOAT3" } ]
                data = [ 0 0 1  0 0 1  0 0 1 ]
                size = 3
                stride = 12
            }
            {
                channels = [ { index = 0 name = "TEXCOORD" type = "CT_FLOAT2" } ]
                data = [ 0 0  1 0  0 1 ]
                size = 3
                stride = 8
            }
        ]
    }
}
nodes = {
    g_cube = {
        geometries = [ "g_cube" ]
        local = [ 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 ]
    }
}
"#;

    const UNIT: &str = r##"
materials = {
    m_cube = "#1122334455667788"
}
renderables = {
    g_cube = {
        always_keep = false
        culling = "bounding_volume"
        occluder = false
        shadow_caster = true
        surface_queries = false
        viewport_visible = true
    }
}
"##;

    fn u32_at(data: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
    }

    fn contains_u32(data: &[u8], value: u32) -> bool {
        data.windows(4).any(|window| window == value.to_le_bytes())
    }

    fn contains_u64(data: &[u8], value: u64) -> bool {
        data.windows(8).any(|window| window == value.to_le_bytes())
    }

    #[test]
    fn compiles_a_minimal_unit() {
        let name = resource_name("units/mods/test/cube");
        let file = match compile(name, UNIT, BSI.as_bytes()) {
            Ok(file) => file,
            Err(error) => panic!("{error:?}"),
        };
        assert_eq!(file.file_type(), BundleFileType::Unit);

        let payload = file.variants()[0].data();
        assert_eq!(u32_at(payload, 0), UNIT_VERSION);
        assert_eq!(u32_at(payload, 4), 1, "one mesh geometry");
        assert_eq!(u32_at(payload, 8), 1, "mesh geometry version");
        assert_eq!(u32_at(payload, 12), 3, "three vertex streams");

        // The material slot, the material resource and the node name are all in
        // the payload as murmur hashes.
        assert!(contains_u32(payload, u32::from(Murmur32::hash("m_cube"))));
        assert!(contains_u64(payload, 0x1122_3344_5566_7788));
        assert!(contains_u32(payload, u32::from(Murmur32::hash("g_cube"))));

        // Sixteen vertices' worth of half4 positions would be wrong here: the
        // payload is small (three vertices).
        assert!(payload.len() < 1024, "payload {} bytes", payload.len());
    }

    #[test]
    fn unify_gathers_independently_indexed_streams() {
        fn channel(name: &str) -> Vec<BsiChannel> {
            vec![BsiChannel {
                index: 0,
                name: name.to_string(),
                kind: "CT_FLOAT3".to_string(),
            }]
        }

        let geometry = BsiGeometry {
            indices: BsiIndices {
                size: 6,
                streams: vec![
                    vec![0, 1, 2, 0, 2, 3],
                    vec![0, 0, 1, 0, 1, 1],
                ],
                kind: "TRIANGLE_LIST".to_string(),
            },
            streams: vec![
                BsiStream {
                    channels: channel("POSITION"),
                    data: vec![
                        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0,
                    ],
                    size: 4,
                    stride: 12,
                },
                BsiStream {
                    channels: channel("NORMAL"),
                    data: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    size: 2,
                    stride: 12,
                },
            ],
            materials: Vec::new(),
        };

        let (streams, indices) = unify_geometry(&geometry).unwrap();
        // Four distinct (position, normal) tuples behind six corners.
        assert_eq!(streams[0].vertices(), 4);
        assert_eq!(streams[1].vertices(), 4);
        assert_eq!(indices, vec![0, 1, 2, 0, 2, 3]);
        // Vertex 2 uses position 2 and normal 1.
        assert_eq!(&streams[1].data[6..9], &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn compiles_lod_objects_from_the_unit_sjson() {
        let unit = r##"
materials = {
    m_cube = "#1122334455667788"
}
lod = [
    {
        bounding_volume = "g_cube"
        name = "lod"
        orientation = "g_cube"
        steps = [
            { renderables = [ "g_cube" ] visible_height_range = [ 1 0.1 ] }
            { renderables = [ "g_cube" ] visible_height_range = [ 0.1 0 ] }
        ]
    }
]
renderables = {
    g_cube = { culling = "bounding_volume" shadow_caster = true viewport_visible = true }
}
"##;
        let file = compile(resource_name("units/mods/test/lod"), unit, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();
        // One LOD object named "lod" with the two authored steps.
        assert!(contains_u32(payload, u32::from(Murmur32::hash("lod"))));
        assert_eq!(u32_at(payload, 4), 1, "one mesh geometry");
    }

    #[test]
    fn normalizer_accepts_space_separated_values() {        #[derive(Deserialize)]
        struct Test {
            a: Vec<u32>,
            b: BTreeMap<String, u32>,
        }

        let text = "a = [ 1 2 3 ]\nb = { c = 4 d = 5 }\n";
        let parsed: Test = serde_sjson::from_str(&normalize_sjson(text)).unwrap();
        assert_eq!(parsed.a, vec![1, 2, 3]);
        assert_eq!(parsed.b["c"], 4);
        assert_eq!(parsed.b["d"], 5);
    }
}

