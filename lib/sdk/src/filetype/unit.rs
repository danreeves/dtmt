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

use color_eyre::eyre::{self, Context as _, Result, bail};
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
    let minimum = match name {
        "POSITION" | "NORMAL" | "COLOR" => 3,
        "TEXCOORD" => 2,
        _ => 4,
    };
    if components < minimum {
        bail!("Channel '{name}' has {components} components, expected at least {minimum}");
    }
    match name {
        // Positions are stored as half4 with w = 1.
        "POSITION" => {
            let mut out = Vec::with_capacity(size as usize * 8);
            for chunk in data.chunks_exact(components) {
                for value in [chunk[0], chunk[1], chunk[2], 1.0] {
                    out.extend_from_slice(&f16(value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat {
                component,
                kind: 17,
                stride: 8,
                data: out,
                vertices: size,
            }))
        }
        // Normals are octahedral-encoded half2.
        "NORMAL" => {
            let mut out = Vec::with_capacity(size as usize * 4);
            for chunk in data.chunks_exact(components) {
                for value in oct_encode([chunk[0], chunk[1], chunk[2]]) {
                    out.extend_from_slice(&f16(value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat {
                component,
                kind: 15,
                stride: 4,
                data: out,
                vertices: size,
            }))
        }
        // Texture coordinates are half2.
        "TEXCOORD" => {
            let mut out = Vec::with_capacity(size as usize * 4);
            for chunk in data.chunks_exact(components) {
                for value in &chunk[..2] {
                    out.extend_from_slice(&f16(*value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat {
                component,
                kind: 15,
                stride: 4,
                data: out,
                vertices: size,
            }))
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
            Ok(Some(VertexFormat {
                component,
                kind: 17,
                stride: 8,
                data: out,
                vertices: size,
            }))
        }
        "BLENDINDICES" => {
            let mut out = Vec::with_capacity(size as usize * 4);
            for chunk in data.chunks_exact(components) {
                for value in chunk {
                    out.push(*value as u8);
                }
            }
            Ok(Some(VertexFormat {
                component,
                kind: 19,
                stride: 4,
                data: out,
                vertices: size,
            }))
        }
        "BLENDWEIGHTS" => {
            let mut out = Vec::with_capacity(size as usize * 8);
            for chunk in data.chunks_exact(components) {
                for value in chunk {
                    out.extend_from_slice(&f16(*value).to_le_bytes());
                }
            }
            Ok(Some(VertexFormat {
                component,
                kind: 17,
                stride: 8,
                data: out,
                vertices: size,
            }))
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
        if let Some(format) = compile_stream(
            &stream.name,
            &stream.data,
            stream.vertices(),
            stream.components,
        )? {
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

/// Convert an IEEE 754 half back to `f32` (the inverse of `f16`).
fn f32_from_f16(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exponent = ((bits >> 10) & 0x1f) as u32;
    let mantissa = (bits & 0x3ff) as u32;
    let value = if exponent == 0 {
        (mantissa as f32) * 2.0f32.powi(-24)
    } else if exponent == 0x1f {
        if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        (mantissa as f32 / 1024.0 + 1.0) * 2.0f32.powi(exponent as i32 - 15)
    };
    if sign == 1 { -value } else { value }
}

/// Decode an octahedral half2 back to a unit direction (the inverse of
/// `oct_encode`).
fn oct_decode(u: f32, v: f32) -> [f32; 3] {
    let mut x = u * 2.0 - 1.0;
    let mut y = v * 2.0 - 1.0;
    let z = 1.0 - (x.abs() + y.abs());
    if z < 0.0 {
        let nx = (1.0 - y.abs()) * if x >= 0.0 { 1.0 } else { -1.0 };
        let ny = (1.0 - x.abs()) * if y >= 0.0 { 1.0 } else { -1.0 };
        x = nx;
        y = ny;
    }
    let length = (x * x + y * y + z * z).sqrt().max(1e-8);
    [x / length, y / length, z / length]
}

/// Unpack one compiled vertex stream back into source floats: the inverse of
/// `compile_stream`, keyed by the channel component and the compiled type.
/// The first slice of unit decompilation.
fn decode_stream(component: u32, kind: u32, data: &[u8]) -> Result<Vec<f32>> {
    fn halves(data: &[u8]) -> Vec<f32> {
        data.chunks_exact(2)
            .map(|chunk| f32_from_f16(u16::from_le_bytes([chunk[0], chunk[1]])))
            .collect()
    }
    match (component, kind) {
        // POSITION: half4, drop w.
        (0, 17) => {
            let mut out = Vec::with_capacity(data.len() / 8 * 3);
            for vertex in halves(data).chunks_exact(4) {
                out.extend_from_slice(&vertex[..3]);
            }
            Ok(out)
        }
        // NORMAL: octahedral half2.
        (1, 15) => {
            let values = halves(data);
            let mut out = Vec::with_capacity(values.len() / 2 * 3);
            for pair in values.chunks_exact(2) {
                out.extend_from_slice(&oct_decode(pair[0], pair[1]));
            }
            Ok(out)
        }
        // TEXCOORD, COLOR and BLENDWEIGHTS: half values as they are.
        (5, 15) | (4, 17) | (8, 17) => Ok(halves(data)),
        // COLOR and BLENDINDICES: bytes.
        (4, 19) => Ok(data.iter().map(|byte| *byte as f32 / 255.0).collect()),
        (7, 19) => Ok(data.iter().map(|byte| *byte as f32).collect()),
        (component, kind) => bail!("Cannot decode channel component {component} type {kind}"),
    }
}

/// A little little-endian reader for payload structures (the counterpart of
/// [`Writer`], used by the decompiler).
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self
            .data
            .get(self.at..self.at + 4)
            .ok_or_else(|| eyre::eyre!("payload ends inside a u32 at {}", self.at))?;
        self.at += 4;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self
            .data
            .get(self.at..self.at + 2)
            .ok_or_else(|| eyre::eyre!("payload ends inside a u16 at {}", self.at))?;
        self.at += 2;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        let bytes = self
            .data
            .get(self.at..self.at + 8)
            .ok_or_else(|| eyre::eyre!("payload ends inside a u64 at {}", self.at))?;
        self.at += 8;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn bool(&mut self) -> Result<bool> {
        let byte = *self
            .data
            .get(self.at)
            .ok_or_else(|| eyre::eyre!("payload ends inside a bool at {}", self.at))?;
        self.at += 1;
        Ok(byte != 0)
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn byte_array(&mut self) -> Result<Vec<u8>> {
        let length = self.u32()? as usize;
        let bytes = self
            .data
            .get(self.at..self.at + length)
            .ok_or_else(|| eyre::eyre!("payload ends inside a byte array at {}", self.at))?;
        self.at += length;
        Ok(bytes.to_vec())
    }

    fn u32_array(&mut self) -> Result<Vec<u32>> {
        let count = self.u32()? as usize;
        (0..count).map(|_| self.u32()).collect()
    }
}

/// One compiled vertex stream, kept packed for a byte-exact rewrite.
struct DecodedStream {
    data: Vec<u8>,
    validity: u32,
    stream_type: u32,
    vertices: u32,
    stride: u32,
}

struct DecodedChannel {
    component: u32,
    kind: u32,
    set: u32,
    stream: u32,
    is_instance: bool,
}

struct DecodedGeometry {
    version: u32,
    streams: Vec<DecodedStream>,
    channels: Vec<DecodedChannel>,
    index_validity: u32,
    index_stream_type: u32,
    index_format: u32,
    index_count: u32,
    index_data: Vec<u8>,
    batches: Vec<[u32; 4]>,
    bounds: [f32; 10],
    materials: Vec<u32>,
    unk2: u32,
}

fn parse_mesh_geometry(reader: &mut Reader<'_>) -> Result<DecodedGeometry> {
    let version = reader.u32()?;
    let mut streams = Vec::new();
    for _ in 0..reader.u32()? {
        streams.push(DecodedStream {
            data: reader.byte_array()?,
            validity: reader.u32()?,
            stream_type: reader.u32()?,
            vertices: reader.u32()?,
            stride: reader.u32()?,
        });
    }
    let mut channels = Vec::new();
    for _ in 0..reader.u32()? {
        channels.push(DecodedChannel {
            component: reader.u32()?,
            kind: reader.u32()?,
            set: reader.u32()?,
            stream: reader.u32()?,
            is_instance: reader.bool()?,
        });
    }
    let index_validity = reader.u32()?;
    let index_stream_type = reader.u32()?;
    let index_format = reader.u32()?;
    let index_count = reader.u32()?;
    let index_data = reader.byte_array()?;
    let mut batches = Vec::new();
    for _ in 0..reader.u32()? {
        batches.push([reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?]);
    }
    let mut bounds = [0.0f32; 10];
    for value in &mut bounds {
        *value = reader.f32()?;
    }
    let materials = reader.u32_array()?;
    let unk2 = reader.u32()?;
    Ok(DecodedGeometry {
        version,
        streams,
        channels,
        index_validity,
        index_stream_type,
        index_format,
        index_count,
        index_data,
        batches,
        bounds,
        materials,
        unk2,
    })
}

fn write_decoded_geometry(w: &mut Writer, geometry: &DecodedGeometry) {
    w.u32(geometry.version);
    w.u32(geometry.streams.len() as u32);
    for stream in &geometry.streams {
        w.byte_array(&stream.data);
        w.u32(stream.validity);
        w.u32(stream.stream_type);
        w.u32(stream.vertices);
        w.u32(stream.stride);
    }
    w.u32(geometry.channels.len() as u32);
    for channel in &geometry.channels {
        w.u32(channel.component);
        w.u32(channel.kind);
        w.u32(channel.set);
        w.u32(channel.stream);
        w.bool(channel.is_instance);
    }
    w.u32(geometry.index_validity);
    w.u32(geometry.index_stream_type);
    w.u32(geometry.index_format);
    w.u32(geometry.index_count);
    w.byte_array(&geometry.index_data);
    w.u32(geometry.batches.len() as u32);
    for batch in &geometry.batches {
        for word in batch {
            w.u32(*word);
        }
    }
    for value in geometry.bounds {
        w.f32(value);
    }
    w.u32_array(&geometry.materials);
    w.u32(geometry.unk2);
}

/// A scene graph node, kept in the order the payload stores its fields.
struct DecodedNode {
    rotation: [f32; 9],
    position: [f32; 3],
    scale: [f32; 3],
    world: [f32; 16],
    parent_type: u16,
    parent_index: u16,
    name: u32,
}

struct DecodedSceneGraph {
    nodes: Vec<DecodedNode>,
    unk6: Vec<(u32, u32)>,
}

fn parse_scene_graph(reader: &mut Reader<'_>) -> Result<DecodedSceneGraph> {
    let mut nodes = Vec::new();
    for _ in 0..reader.u32()? {
        let mut rotation = [0.0f32; 9];
        for value in &mut rotation {
            *value = reader.f32()?;
        }
        let mut position = [0.0f32; 3];
        for value in &mut position {
            *value = reader.f32()?;
        }
        let mut scale = [0.0f32; 3];
        for value in &mut scale {
            *value = reader.f32()?;
        }
        nodes.push(DecodedNode {
            rotation,
            position,
            scale,
            world: [0.0f32; 16],
            parent_type: 0,
            parent_index: 0,
            name: 0,
        });
    }
    for node in &mut nodes {
        for value in &mut node.world {
            *value = reader.f32()?;
        }
    }
    for node in &mut nodes {
        node.parent_type = reader.u16()?;
        node.parent_index = reader.u16()?;
    }
    for node in &mut nodes {
        node.name = reader.u32()?;
    }
    let mut unk6 = Vec::new();
    for _ in 0..reader.u32()? {
        unk6.push((reader.u32()?, reader.u32()?));
    }
    Ok(DecodedSceneGraph { nodes, unk6 })
}

fn write_decoded_scene_graph(w: &mut Writer, graph: &DecodedSceneGraph) {
    w.u32(graph.nodes.len() as u32);
    for node in &graph.nodes {
        for value in node.rotation {
            w.f32(value);
        }
        for value in node.position {
            w.f32(value);
        }
        for value in node.scale {
            w.f32(value);
        }
    }
    for node in &graph.nodes {
        for value in node.world {
            w.f32(value);
        }
    }
    for node in &graph.nodes {
        w.u16(node.parent_type);
        w.u16(node.parent_index);
    }
    for node in &graph.nodes {
        w.u32(node.name);
    }
    w.u32(graph.unk6.len() as u32);
    for (a, b) in &graph.unk6 {
        w.u32(*a);
        w.u32(*b);
    }
}

/// A mesh object: name, indices and render flags.
struct DecodedMesh {
    name: u32,
    node_index: u32,
    geometry_index: u32,
    skin_index: u32,
    unk4: u32,
    unk5: u32,
    unk6: u32,
    bounds: [f32; 10],
    unk7: u32,
}

fn parse_mesh_objects(reader: &mut Reader<'_>) -> Result<Vec<DecodedMesh>> {
    let mut meshes = Vec::new();
    for _ in 0..reader.u32()? {
        let mut mesh = DecodedMesh {
            name: reader.u32()?,
            node_index: reader.u32()?,
            geometry_index: reader.u32()?,
            skin_index: reader.u32()?,
            unk4: reader.u32()?,
            unk5: reader.u32()?,
            unk6: reader.u32()?,
            bounds: [0.0f32; 10],
            unk7: 0,
        };
        for value in &mut mesh.bounds {
            *value = reader.f32()?;
        }
        mesh.unk7 = reader.u32()?;
        meshes.push(mesh);
    }
    Ok(meshes)
}

fn write_decoded_mesh_objects(w: &mut Writer, meshes: &[DecodedMesh]) {
    w.u32(meshes.len() as u32);
    for mesh in meshes {
        w.u32(mesh.name);
        w.u32(mesh.node_index);
        w.u32(mesh.geometry_index);
        w.u32(mesh.skin_index);
        w.u32(mesh.unk4);
        w.u32(mesh.unk5);
        w.u32(mesh.unk6);
        for value in mesh.bounds {
            w.f32(value);
        }
        w.u32(mesh.unk7);
    }
}

struct DecodedLodStep {
    range: [f32; 2],
    meshes: Vec<u32>,
    stream_offset: u32,
    unk5: u32,
}

struct DecodedLod {
    name: u32,
    unk2: u64,
    unk3: u32,
    steps: Vec<DecodedLodStep>,
    bounds: [f32; 10],
    unk4: u32,
    unk5: u32,
    order: Vec<u32>,
    unk7: u32,
    unk8: bool,
}

fn parse_lod_objects(reader: &mut Reader<'_>) -> Result<Vec<DecodedLod>> {
    let mut lods = Vec::new();
    for _ in 0..reader.u32()? {
        let name = reader.u32()?;
        let unk2 = reader.u64()?;
        let unk3 = reader.u32()?;
        let mut steps = Vec::new();
        for _ in 0..reader.u32()? {
            let range = [reader.f32()?, reader.f32()?];
            let meshes = reader.u32_array()?;
            steps.push(DecodedLodStep {
                range,
                meshes,
                stream_offset: reader.u32()?,
                unk5: reader.u32()?,
            });
        }
        let mut bounds = [0.0f32; 10];
        for value in &mut bounds {
            *value = reader.f32()?;
        }
        let unk4 = reader.u32()?;
        let unk5 = reader.u32()?;
        let order = reader.u32_array()?;
        let unk7 = reader.u32()?;
        let unk8 = reader.bool()?;
        lods.push(DecodedLod {
            name,
            unk2,
            unk3,
            steps,
            bounds,
            unk4,
            unk5,
            order,
            unk7,
            unk8,
        });
    }
    Ok(lods)
}

fn write_decoded_lods(w: &mut Writer, lods: &[DecodedLod]) {
    w.u32(lods.len() as u32);
    for lod in lods {
        w.u32(lod.name);
        w.u64(lod.unk2);
        w.u32(lod.unk3);
        w.u32(lod.steps.len() as u32);
        for step in &lod.steps {
            w.f32(step.range[0]);
            w.f32(step.range[1]);
            w.u32_array(&step.meshes);
            w.u32(step.stream_offset);
            w.u32(step.unk5);
        }
        for value in lod.bounds {
            w.f32(value);
        }
        w.u32(lod.unk4);
        w.u32(lod.unk5);
        w.u32_array(&lod.order);
        w.u32(lod.unk7);
        w.bool(lod.unk8);
    }
}

/// Emits the `.unit` SJSON for a decoded payload. Names that only survive as
/// hashes are written as `#HEX` tokens, which the compiler takes as hashes.
fn unit_sjson(slots: &[(u32, u64)], meshes: &[u32], lods: &[DecodedLod]) -> String {
    let mut text = String::from("materials = {\n");
    for (slot, resource) in slots {
        text.push_str(&format!("\t\"#{slot:08X}\" = \"#{resource:016X}\"\n"));
    }
    text.push_str("}\n");
    if !lods.is_empty() {
        text.push_str("lod = [\n");
        for lod in lods {
            text.push_str(&format!(
                "\t{{\n\t\tname = \"#{:08X}\"\n\t\tsteps = [\n",
                lod.name
            ));
            for step in &lod.steps {
                let names: Vec<String> = step
                    .meshes
                    .iter()
                    .filter_map(|mesh| meshes.get(*mesh as usize))
                    .map(|mesh| format!("\"#{mesh:08X}\""))
                    .collect();
                text.push_str(&format!(
                    "\t\t\t{{ renderables = [ {} ] visible_height_range = [ {} {} ] }}\n",
                    names.join(" "),
                    step.range[0],
                    step.range[1]
                ));
            }
            text.push_str("\t\t]\n\t}\n");
        }
        text.push_str("]\n");
    }
    text.push_str("renderables = {\n");
    for mesh in meshes {
        text.push_str(&format!(
            "\t\"#{mesh:08X}\" = {{ culling = \"bounding_volume\" shadow_caster = true viewport_visible = true }}\n"
        ));
    }
    text.push_str("}\n");
    text
}

/// Component and channel-type names for the emitted `.bsi`, matching the
/// compiler's reader.
fn channel_name(component: u32) -> Result<&'static str> {
    Ok(match component {
        0 => "POSITION",
        1 => "NORMAL",
        4 => "COLOR",
        5 => "TEXCOORD",
        7 => "BLENDINDICES",
        8 => "BLENDWEIGHTS",
        other => bail!("Cannot emit channel component {other}"),
    })
}

fn channel_type(kind: u32) -> Result<&'static str> {
    Ok(match kind {
        17 => "CT_HALF4",
        15 => "CT_HALF2",
        19 => "CT_UBYTE4",
        other => bail!("Cannot emit channel type {other}"),
    })
}

/// Emits the `.bsi` SJSON for decoded geometries and mesh objects. Each
/// geometry is named after the mesh object that uses it; the names double as
/// the `.unit` renderable and node names.
fn bsi_sjson(geometries: &[DecodedGeometry], meshes: &[DecodedMesh]) -> Result<String> {
    let mut names: Vec<Option<u32>> = vec![None; geometries.len()];
    for mesh in meshes {
        if mesh.geometry_index == 0 {
            continue;
        }
        if let Some(slot) = names.get_mut(mesh.geometry_index as usize - 1)
            && slot.is_none()
        {
            *slot = Some(mesh.name);
        }
    }

    let mut text = String::from("geometries = {\n");
    for (index, geometry) in geometries.iter().enumerate() {
        let name = names
            .get(index)
            .and_then(|name| *name)
            .ok_or_else(|| eyre::eyre!("geometry {index} has no mesh object"))?;

        let indices: Vec<u32> = if geometry.index_format == 0 {
            geometry
                .index_data
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]) as u32)
                .collect()
        } else {
            geometry
                .index_data
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect()
        };

        text.push_str(&format!(
            "\t\"#{name:08X}\" = {{\n\t\tindices = {{\n\t\t\tsize = {}\n\t\t\tstreams = [ [ {} ] ]\n\t\t\ttype = \"TRIANGLE_LIST\"\n\t\t}}\n",
            indices.len(),
            indices.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ")
        ));
        text.push_str("\t\tmaterials = [\n");
        for batch in &geometry.batches {
            let slot = geometry
                .materials
                .get(batch[0] as usize)
                .copied()
                .unwrap_or(0);
            let primitives: Vec<String> = (batch[1]..batch[1] + batch[2])
                .map(|triangle| triangle.to_string())
                .collect();
            text.push_str(&format!(
                "\t\t\t{{ name = \"#{slot:08X}\" primitives = [ {} ] }}\n",
                primitives.join(" ")
            ));
        }
        text.push_str("\t\t]\n\t\tstreams = [\n");
        for (stream_index, stream) in geometry.streams.iter().enumerate() {
            let channel = geometry.channels.get(stream_index).ok_or_else(|| {
                eyre::eyre!("geometry {index} stream {stream_index} has no channel")
            })?;
            let data = decode_stream(channel.component, channel.kind, &stream.data)?;
            let components = data.len() / stream.vertices.max(1) as usize;
            text.push_str(&format!(
                "\t\t\t{{ channels = [ {{ index = 0 name = \"{}\" type = \"{}\" }} ] data = [ {} ] size = {} stride = {} }}\n",
                channel_name(channel.component)?,
                channel_type(channel.kind)?,
                data.iter().map(|value| format!("{value}")).collect::<Vec<_>>().join(" "),
                stream.vertices,
                components * 4
            ));
        }
        text.push_str("\t\t]\n\t}\n");
    }
    text.push_str("}\n");

    text.push_str("nodes = {\n");
    for (index, geometry) in geometries.iter().enumerate() {
        let _ = geometry;
        let name = names[index].unwrap();
        text.push_str(&format!(
            "\t\"#{name:08X}\" = {{ geometries = [ \"#{name:08X}\" ] local = [ 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 ] }}\n"
        ));
    }
    text.push_str("}\n");
    Ok(text)
}

/// Decompiles a static unit payload into its `.unit` and `.bsi` SJSON texts.
///
/// Skins, animations, actors, cameras, lights, terrains, joints and movers are
/// not supported yet; a payload that uses them fails with a clear error.
pub fn decompile(payload: &[u8]) -> Result<(String, String)> {
    let mut reader = Reader::new(payload);
    let version = reader.u32()?;
    if version != UNIT_VERSION {
        bail!("unit payload version {version:#x}, expected {UNIT_VERSION:#x}");
    }
    let mut geometries = Vec::new();
    for _ in 0..reader.u32()? {
        geometries.push(parse_mesh_geometry(&mut reader)?);
    }
    if reader.u32()? != 0 {
        bail!("skinned units are not supported yet");
    }
    if reader.byte_array()?.len() != 0 {
        bail!("simple animations are not supported yet");
    }
    if reader.u32()? != 0 {
        bail!("animation groups are not supported yet");
    }
    let _scene_graph = parse_scene_graph(&mut reader)?;
    let meshes = parse_mesh_objects(&mut reader)?;
    if reader.u32()? != 0 || reader.u32()? != 0 || reader.u32_array()?.len() != 0 {
        bail!("actors are not supported yet");
    }
    if reader.u32()? != 0 || reader.u32()? != 0 {
        bail!("cameras and lights are not supported yet");
    }
    if reader.byte_array()?.len() != 0 {
        bail!("an unknown device blob is not empty");
    }
    let lods = parse_lod_objects(&mut reader)?;
    // Terrains, unknowns, joints and movers.
    if reader.u32()? != 0
        || reader.u32()? != 0
        || reader.u32()? != 0
        || reader.u32()? != 0
        || reader.u32()? != 0
    {
        bail!("terrains, joints or movers are not supported yet");
    }
    let _animation_bones = reader.bool()?;
    let _animation_state_machine = reader.byte_array()?;
    let _dynamic_data = reader.byte_array()?;
    if reader.u32()? != 0 {
        bail!("visibility groups are not supported yet");
    }
    let _flow = reader.byte_array()?;
    let _flow_dynamic = reader.byte_array()?;
    let _triangle_finder = reader.byte_array()?;
    let _physics = reader.byte_array()?;
    let _default_material = reader.u64()?;
    let mut slots = Vec::new();
    for _ in 0..reader.u32()? {
        slots.push((reader.u32()?, reader.u64()?));
    }

    let mesh_names: Vec<u32> = meshes.iter().map(|mesh| mesh.name).collect();
    Ok((
        unit_sjson(&slots, &mesh_names, &lods),
        bsi_sjson(&geometries, &meshes)?,
    ))
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

/// Resolves a name token to a 32 bit hash: an eight digit hex string (with or
/// without a leading `#`) is taken as the hash itself, anything else is hashed
/// like the engine hashes names. This lets a decompiled unit keep the exact
/// hashes it read from a compiled payload.
fn hash32_token(token: &str) -> u32 {
    let trimmed = token.strip_prefix('#').unwrap_or(token);
    if trimmed.len() == 8 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        u32::from_str_radix(trimmed, 16).unwrap_or(0)
    } else {
        u32::from(Murmur32::hash(token))
    }
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
        slots.push((u32::from(hash32_token(slot)), resource));
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
            .map(|material| u32::from(hash32_token(&material.name)))
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
        w.u32(u32::from(hash32_token(node.name)));
    }
    w.u32(0); // unknown list

    // Mesh objects.
    w.u32(meshes.len() as u32);
    for (renderable, geometry_index, node_index) in &meshes {
        w.u32(u32::from(hash32_token(renderable)));
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
        w.u32(u32::from(hash32_token(&lod.name)));
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
    fn mesh_geometry_round_trips() {
        let name = resource_name("units/mods/test/roundtrip");
        let file = compile(name, UNIT, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();

        // The payload opens with the version word and the geometry count; the
        // first geometry follows.
        let mut reader = Reader::new(&payload[8..]);
        let geometry = parse_mesh_geometry(&mut reader).unwrap();
        assert_eq!(geometry.version, 1);
        assert_eq!(geometry.streams.len(), 3);
        assert_eq!(geometry.channels.len(), 3);
        assert_eq!(geometry.channels[0].component, 0, "POSITION first");
        assert_eq!(geometry.index_count, 3);
        assert_eq!(geometry.materials.len(), 1);
        assert_eq!(geometry.batches.len(), 1);

        let mut writer = Writer::new();
        write_decoded_geometry(&mut writer, &geometry);
        assert_eq!(writer.buf, payload[8..8 + writer.buf.len()]);
    }

    #[test]
    fn scene_graph_round_trips() {
        let name = resource_name("units/mods/test/scene");
        let file = compile(name, UNIT, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();

        // After the version word, the geometry count and the first geometry,
        // then the (empty) skins and simple-animation sections.
        let mut reader = Reader::new(&payload[8..]);
        let geometry = parse_mesh_geometry(&mut reader).unwrap();
        assert_eq!(reader.u32().unwrap(), 0, "skins");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "simple animation");
        assert_eq!(reader.u32().unwrap(), 0, "simple animation groups");
        let graph = parse_scene_graph(&mut reader).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].name, u32::from(Murmur32::hash("g_cube")));

        let mut writer = Writer::new();
        write_decoded_geometry(&mut writer, &geometry);
        writer.u32(0);
        writer.byte_array(&[]);
        writer.u32(0);
        write_decoded_scene_graph(&mut writer, &graph);
        assert_eq!(writer.buf, payload[8..8 + writer.buf.len()]);
    }

    #[test]
    fn mesh_objects_round_trip() {
        let name = resource_name("units/mods/test/meshes");
        let file = compile(name, UNIT, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();

        let mut reader = Reader::new(&payload[8..]);
        let geometry = parse_mesh_geometry(&mut reader).unwrap();
        assert_eq!(reader.u32().unwrap(), 0, "skins");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "simple animation");
        assert_eq!(reader.u32().unwrap(), 0, "simple animation groups");
        let graph = parse_scene_graph(&mut reader).unwrap();
        let meshes = parse_mesh_objects(&mut reader).unwrap();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].name, u32::from(Murmur32::hash("g_cube")));
        assert_eq!(meshes[0].geometry_index, 1);

        let mut writer = Writer::new();
        write_decoded_geometry(&mut writer, &geometry);
        writer.u32(0);
        writer.byte_array(&[]);
        writer.u32(0);
        write_decoded_scene_graph(&mut writer, &graph);
        write_decoded_mesh_objects(&mut writer, &meshes);
        assert_eq!(writer.buf, payload[8..8 + writer.buf.len()]);
    }

    #[test]
    fn trailer_sections_are_as_expected() {
        let name = resource_name("units/mods/test/trailer");
        let file = compile(name, UNIT, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();

        let mut reader = Reader::new(&payload[8..]);
        let _geometry = parse_mesh_geometry(&mut reader).unwrap();
        assert_eq!(reader.u32().unwrap(), 0, "skins");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "simple animation");
        assert_eq!(reader.u32().unwrap(), 0, "simple animation groups");
        let _graph = parse_scene_graph(&mut reader).unwrap();
        let meshes = parse_mesh_objects(&mut reader).unwrap();
        assert_eq!(meshes.len(), 1);

        // Actors, cameras and the unknown blob.
        assert_eq!(reader.u32().unwrap(), 0, "actors");
        assert_eq!(reader.u32().unwrap(), 0, "actors 2");
        assert_eq!(reader.u32_array().unwrap().len(), 0, "unknown list");
        assert_eq!(reader.u32().unwrap(), 0, "cameras");
        assert_eq!(reader.u32().unwrap(), 0, "lights");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "unknown blob");
        // LOD objects, terrains, unknowns, joints and movers.
        assert_eq!(reader.u32().unwrap(), 0, "lod objects");
        assert_eq!(reader.u32().unwrap(), 0, "terrains");
        assert_eq!(reader.u32().unwrap(), 0, "unknown 11");
        assert_eq!(reader.u32().unwrap(), 0, "joints");
        assert_eq!(reader.u32().unwrap(), 0, "movers");
        assert_eq!(reader.u32().unwrap(), 0, "unknown 15");
        // Animation state, visibility groups and flow data.
        assert!(!reader.bool().unwrap(), "animation bones flag");
        assert_eq!(
            reader.byte_array().unwrap().len(),
            0,
            "animation state machine"
        );
        assert_eq!(
            reader.byte_array().unwrap(),
            vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
            "dynamic data sentinel"
        );
        assert_eq!(reader.u32().unwrap(), 0, "visibility groups");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "flow");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "flow dynamic data");
        assert_eq!(
            reader.byte_array().unwrap(),
            vec![0, 0, 0, 0],
            "triangle finder"
        );
        assert_eq!(reader.byte_array().unwrap().len(), 0, "physics");
        // Default material and the material list.
        assert_eq!(reader.u64().unwrap(), 0, "default material");
        assert_eq!(reader.u32().unwrap(), 1, "materials");
        assert_eq!(reader.u32().unwrap(), 0x44f4_a503, "material slot");
        assert_eq!(reader.u64().unwrap(), 0x1122_3344_5566_7788, "material");
        // Trailing unknowns and the skeleton name.
        assert_eq!(reader.u32().unwrap(), 0, "unknown 16");
        assert_eq!(reader.u32().unwrap(), 0, "unknown 17");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "unknown 18");
        assert_eq!(reader.u32().unwrap(), 0, "unknown 19");
        assert_eq!(reader.u64().unwrap(), 0, "skeleton name");
        assert_eq!(reader.u32().unwrap(), 0, "unknown 20");
        assert_eq!(reader.at, payload.len() - 8, "payload consumed exactly");
    }

    #[test]
    fn lod_objects_parse_and_rewrite() {
        let unit = r##"
materials = {
    m_cube = "#1122334455667788"
}
lod = [
    {
        name = "lod"
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
        let file = compile(resource_name("units/mods/test/lods"), unit, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();

        let mut reader = Reader::new(&payload[8..]);
        let geometry = parse_mesh_geometry(&mut reader).unwrap();
        assert_eq!(reader.u32().unwrap(), 0, "skins");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "simple animation");
        assert_eq!(reader.u32().unwrap(), 0, "simple animation groups");
        let graph = parse_scene_graph(&mut reader).unwrap();
        let meshes = parse_mesh_objects(&mut reader).unwrap();
        assert_eq!(reader.u32().unwrap(), 0, "actors");
        assert_eq!(reader.u32().unwrap(), 0, "actors 2");
        assert_eq!(reader.u32_array().unwrap().len(), 0, "unknown list");
        assert_eq!(reader.u32().unwrap(), 0, "cameras");
        assert_eq!(reader.u32().unwrap(), 0, "lights");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "unknown blob");
        let lods = parse_lod_objects(&mut reader).unwrap();
        assert_eq!(lods.len(), 1);
        assert_eq!(lods[0].name, u32::from(Murmur32::hash("lod")));
        assert_eq!(lods[0].steps.len(), 2);
        assert_eq!(lods[0].steps[0].range, [1.0, 0.1]);
        assert_eq!(lods[0].steps[0].meshes, vec![0]);
        assert_eq!(lods[0].order, vec![0, 0]);

        let mut writer = Writer::new();
        write_decoded_geometry(&mut writer, &geometry);
        writer.u32(0);
        writer.byte_array(&[]);
        writer.u32(0);
        write_decoded_scene_graph(&mut writer, &graph);
        write_decoded_mesh_objects(&mut writer, &meshes);
        writer.u32(0);
        writer.u32(0);
        writer.u32_array(&[]);
        writer.u32(0);
        writer.u32(0);
        writer.byte_array(&[]);
        write_decoded_lods(&mut writer, &lods);
        assert_eq!(writer.buf, payload[8..8 + writer.buf.len()]);
    }

    #[test]
    fn unit_sjson_uses_hash_names() {
        let text = unit_sjson(&[(0x44F4_A503, 0x1122_3344_5566_7788)], &[0x553C_252C], &[]);
        assert!(
            text.contains("\"#44F4A503\" = \"#1122334455667788\""),
            "{text}"
        );
        assert!(text.contains("\"#553C252C\" = {"), "{text}");
    }

    #[test]
    fn bsi_sjson_describes_the_geometry() {
        let name = resource_name("units/mods/test/bsi");
        let file = compile(name, UNIT, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();
        let mut reader = Reader::new(&payload[8..]);
        let geometry = parse_mesh_geometry(&mut reader).unwrap();
        assert_eq!(reader.u32().unwrap(), 0, "skins");
        assert_eq!(reader.byte_array().unwrap().len(), 0, "simple animation");
        assert_eq!(reader.u32().unwrap(), 0, "simple animation groups");
        let _graph = parse_scene_graph(&mut reader).unwrap();
        let meshes = parse_mesh_objects(&mut reader).unwrap();

        let text = bsi_sjson(&[geometry], &meshes).unwrap();
        assert!(text.contains("\"#553C252C\" = {"), "{text}");
        assert!(text.contains("size = 3"), "indices: {text}");
        assert!(
            text.contains("name = \"POSITION\" type = \"CT_HALF4\""),
            "{text}"
        );
        assert!(
            text.contains("name = \"NORMAL\" type = \"CT_HALF2\""),
            "{text}"
        );
        assert!(
            text.contains("name = \"TEXCOORD\" type = \"CT_HALF2\""),
            "{text}"
        );
        assert!(
            text.contains("name = \"#44F4A503\""),
            "material slot: {text}"
        );
        assert!(text.contains("primitives = [ 0 ]"), "{text}");
    }

    #[test]
    fn decompile_round_trips_a_payload() {
        let name = resource_name("units/mods/test/decompile");
        let file = compile(name.clone(), UNIT, BSI.as_bytes()).unwrap();
        let payload = file.variants()[0].data();

        let (unit_text, bsi_text) = decompile(payload).unwrap();
        assert!(bsi_text.contains("\"#553C252C\" = {"), "{bsi_text}");
        assert!(
            unit_text.contains("\"#44F4A503\" = \"#1122334455667788\""),
            "{unit_text}"
        );

        // Recompiling the emitted pair gives an equivalent payload. Normals are
        // octahedral-encoded, so their unpack/re-pack is not bit exact; compare
        // the shape instead.
        let again = compile(name.clone(), &unit_text, bsi_text.as_bytes()).unwrap();
        let payload_again = again.variants()[0].data();
        assert_eq!(payload.len(), payload_again.len());
        let mut a = Reader::new(&payload[8..]);
        let mut b = Reader::new(&payload_again[8..]);
        let left = parse_mesh_geometry(&mut a).unwrap();
        let right = parse_mesh_geometry(&mut b).unwrap();
        assert_eq!(left.streams.len(), right.streams.len());
        assert_eq!(left.index_count, right.index_count);
        assert_eq!(left.batches, right.batches);
        assert_eq!(left.materials, right.materials);
    }

    #[test]
    fn streams_round_trip_through_decode() {
        // POSITION: half4, xyz plus a discarded w.
        let positions = [0.5f32, -1.0, 2.0, 0.25, 0.0, 1.0];
        let format = compile_stream("POSITION", &positions, 2, 3)
            .unwrap()
            .unwrap();
        assert_eq!(format.kind, 17);
        let decoded = decode_stream(0, format.kind, &format.data).unwrap();
        assert_eq!(decoded.len(), 6);
        for (before, after) in positions.iter().zip(&decoded) {
            assert!((before - after).abs() < 1e-3, "{before} vs {after}");
        }

        // NORMAL: octahedral half2.
        let normals = [0.0f32, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, -1.0, 0.0];
        let format = compile_stream("NORMAL", &normals, 3, 3).unwrap().unwrap();
        assert_eq!(format.kind, 15);
        let decoded = decode_stream(1, format.kind, &format.data).unwrap();
        for (before, after) in normals.iter().zip(&decoded) {
            assert!((before - after).abs() < 1e-2, "{before} vs {after}");
        }

        // TEXCOORD: half2.
        let uvs = [0.0f32, 1.0, 0.5, 0.25];
        let format = compile_stream("TEXCOORD", &uvs, 2, 2).unwrap().unwrap();
        let decoded = decode_stream(5, format.kind, &format.data).unwrap();
        assert_eq!(decoded, uvs);
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
                streams: vec![vec![0, 1, 2, 0, 2, 3], vec![0, 0, 1, 0, 1, 1]],
                kind: "TRIANGLE_LIST".to_string(),
            },
            streams: vec![
                BsiStream {
                    channels: channel("POSITION"),
                    data: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0],
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
    fn hex_names_are_taken_as_hashes() {
        // A decompiled unit names its slots and renderables by hash; the
        // compiler must not re-hash those strings.
        let bsi = BSI.replace("g_cube", "553C252C");
        let unit = UNIT
            .replace("g_cube", "553C252C")
            .replace("m_cube", "44F4A503");
        let file = compile(resource_name("units/mods/test/hex"), &unit, bsi.as_bytes()).unwrap();
        let payload = file.variants()[0].data();
        assert!(contains_u32(payload, 0x553C_252C), "mesh/node name");
        assert!(contains_u32(payload, 0x44F4_A503), "material slot");
        assert!(!contains_u32(
            payload,
            u32::from(Murmur32::hash("553C252C"))
        ));
    }

    #[test]
    fn normalizer_accepts_space_separated_values() {
        #[derive(Deserialize)]
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
