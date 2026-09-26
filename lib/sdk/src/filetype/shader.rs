//! Darktide `shader43` sections: the compiled shader payload embedded in base
//! materials.
//!
//! A material's shader section starts with a 12 word header:
//!
//! ```text
//! u32 version                         // 43
//! u32 opaque
//! u32 contexts_offset
//! u32 context_count
//! u32 conditions_offset
//! u32 default_data_offset
//! u32 dependency_offset
//! u32 dependency_count
//! u32 group_data_offset
//! u32 group_data_size
//! u32 device_data_offset
//! u32 device_data_size
//! ```
//!
//! The device data is a sequence of programs. Each program is:
//!
//! ```text
//! u32 envelope                        // 1
//! u32 frame_length
//! u8  frame[frame_length]             // an Oodle stream containing a DXBC container
//! u32 metadata_kind                   // 5
//! u32 decoded_dxbc_length
//! u64 frame_key                       // MurmurHash64A(frame, seed 0)
//! counted metadata tables and opaque state
//! ```
//!
//! Frames are Oodle streams: the whole frame, including its Oodle header, is
//! passed to Oodle's decoder. Re-encoding uses Oodle's Kraken compressor.
//!
//! # References
//!
//! The shader43 header and framed program layout were established with the help
//! of the public RainbowFlame reverse engineering write-up by Vansinnet
//! (findings only; no code from that project is used here).

use color_eyre::eyre::{Context as _, Result, bail, eyre};
use oodle::{OodleLZ_CheckCRC, OodleLZ_FuzzSafe};

use crate::filetype::group_data::{DEPENDENCY_LEN, Dependency};
use crate::murmur;

/// Shader section version used by Darktide.
pub const VERSION: u32 = 43;

/// Program metadata kind observed for DXBC/DXIL programs.
const METADATA_KIND: u32 = 5;

/// The first bytes of every program frame (Oodle Kraken).
const FRAME_MAGIC: [u8; 2] = [0x8C, 0x06];

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

/// A byte range, or `None` when it is out of range or runs backwards.
fn check_range(data: &[u8], from: usize, to: usize) -> Option<Vec<u8>> {
    if to < from {
        return None;
    }
    Some(data.get(from..to)?.to_vec())
}

/// Shader stage of a DXBC container, read from its `PSV0` chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    Pixel,
    Vertex,
    Geometry,
    Hull,
    Domain,
    Compute,
    Other,
}

impl Stage {
    fn from_psv_kind(kind: u32) -> Self {
        match kind {
            0 => Stage::Pixel,
            1 => Stage::Vertex,
            2 => Stage::Geometry,
            3 => Stage::Hull,
            4 => Stage::Domain,
            5 => Stage::Compute,
            _ => Stage::Other,
        }
    }
}

/// One framed program inside the device data.
pub struct Program {
    /// Index in the device data.
    pub index: usize,
    /// Offset of the `envelope` word in the device data.
    pub pos: usize,
    /// Offset of `metadata_kind` in the device data.
    pub meta_pos: usize,
    pub frame_length: usize,
    pub decoded_length: usize,
    pub frame_key: u64,
    /// The decoded DXBC container.
    pub container: Vec<u8>,
    pub stage: Stage,
}

impl Program {
    /// The program's complete record in the device data, including its frame
    /// and metadata.
    pub fn record<'a>(&self, device: &'a [u8], next_pos: usize) -> &'a [u8] {
        &device[self.pos..next_pos]
    }
}

/// One constant buffer of a program's metadata tail. The entry is 24 bytes;
/// within it the name hash sits at `+0` and the size in bytes at `+8`, the
/// other four words are not decoded and are kept as they were read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TailCbuffer {
    pub words: [u32; 6],
}

impl TailCbuffer {
    /// Murmur32 of the constant buffer's name in the engine's shader sources.
    pub fn name_hash(&self) -> u32 {
        self.words[0]
    }

    /// Size of the constant buffer in bytes.
    pub fn size(&self) -> u32 {
        self.words[2]
    }
}

/// A program's metadata tail: the counted constant buffer list followed by the
/// engine's resource lists and the shared block. The lists after the constant
/// buffers are not decoded yet, so they are kept verbatim; parsing and writing
/// a tail round-trips its bytes exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tail {
    /// The constant buffered described by the tail, in register order.
    pub cbuffers: Vec<TailCbuffer>,
    /// The tail bytes after the constant buffer list.
    pub rest: Vec<u8>,
}

impl Tail {
    /// Reads the constant buffer list that opens a program's metadata tail.
    /// Returns `None` if the bytes do not look like one.
    pub fn parse(bytes: &[u8]) -> Option<Tail> {
        if bytes.len() < 4 {
            return None;
        }
        let count = u32_at(bytes, 0) as usize;
        if count > 64 {
            return None;
        }
        let list_end = 4 + count * 24;
        if list_end > bytes.len() {
            return None;
        }

        let mut cbuffers = Vec::with_capacity(count);
        for i in 0..count {
            let at = 4 + i * 24;
            let mut words = [0u32; 6];
            for (word, slot) in words.iter_mut().enumerate() {
                *slot = u32_at(bytes, at + word * 4);
            }
            let entry = TailCbuffer { words };
            if entry.name_hash() == 0
                || entry.size() == 0
                || entry.size() >= 8192
                || entry.size() % 16 != 0
            {
                return None;
            }
            cbuffers.push(entry);
        }

        Some(Tail {
            cbuffers,
            rest: bytes[list_end..].to_vec(),
        })
    }

    /// Writes the tail back out: the constant buffer list, then the rest.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.cbuffers.len() * 24 + self.rest.len());
        out.extend_from_slice(&(self.cbuffers.len() as u32).to_le_bytes());
        for entry in &self.cbuffers {
            for word in entry.words {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out.extend_from_slice(&self.rest);
        out
    }
}

/// Finds a named chunk in a DXBC container.
fn find_chunk<'a>(container: &'a [u8], name: &[u8; 4]) -> Option<&'a [u8]> {
    if container.len() < 32 || &container[0..4] != b"DXBC" {
        return None;
    }
    let chunk_count = u32_at(container, 28) as usize;
    if chunk_count > 64 {
        return None;
    }
    for i in 0..chunk_count {
        let at = 32 + i * 4;
        if at + 4 > container.len() {
            return None;
        }
        let offset = u32_at(container, at) as usize;
        if offset + 8 > container.len() {
            continue;
        }
        if &container[offset..offset + 4] == name {
            let size = u32_at(container, offset + 4) as usize;
            return container.get(offset + 8..offset + 8 + size);
        }
    }
    None
}

/// Returns the names and sizes of a DXBC container's chunks.
pub fn chunks(container: &[u8]) -> Vec<(String, usize)> {
    let mut chunks = Vec::new();
    if container.len() < 32 || &container[0..4] != b"DXBC" {
        return chunks;
    }
    let chunk_count = u32_at(container, 28) as usize;
    for i in 0..chunk_count.min(64) {
        let at = 32 + i * 4;
        if at + 4 > container.len() {
            break;
        }
        let offset = u32_at(container, at) as usize;
        if offset + 8 > container.len() {
            continue;
        }
        let name = String::from_utf8_lossy(&container[offset..offset + 4]).to_string();
        let size = u32_at(container, offset + 4) as usize;
        chunks.push((name, size));
    }
    chunks
}

/// Reads the shader stage of a DXBC container from its `PSV0` chunk.
pub fn stage_of(container: &[u8]) -> Stage {
    find_chunk(container, b"PSV0")
        .filter(|psv| psv.len() >= 8)
        .map(|psv| Stage::from_psv_kind(u32_at(psv, 4)))
        .unwrap_or(Stage::Other)
}

/// One semantic of an input or output signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureElement {
    pub name: String,
    pub index: u32,
    pub register: u32,
    pub mask: u8,
}

/// Parses the input and output signatures of a DXBC container.
pub fn signatures(container: &[u8]) -> Option<(Vec<SignatureElement>, Vec<SignatureElement>)> {
    let input = find_chunk(container, b"ISG1").and_then(parse_signature)?;
    let output = find_chunk(container, b"OSG1").and_then(parse_signature)?;
    Some((input, output))
}

/// Parses an `ISG1`/`OSG1` signature chunk into its semantics.
///
/// Signature elements are 32 bytes: a stream index, the name offset, semantic
/// index, system value type, component type, register, mask and read/write
/// mask. The name offset is relative to the start of the chunk data.
fn parse_signature(chunk: &[u8]) -> Option<Vec<SignatureElement>> {
    if chunk.len() < 8 {
        return None;
    }
    let count = u32_at(chunk, 0) as usize;
    let elements_offset = u32_at(chunk, 4) as usize;
    if count > 64 {
        return None;
    }

    let mut elements = Vec::with_capacity(count);
    for i in 0..count {
        let at = elements_offset + i * 32;
        if at + 32 > chunk.len() {
            return None;
        }
        let name_offset = u32_at(chunk, at + 4) as usize;
        let index = u32_at(chunk, at + 8);
        let register = u32_at(chunk, at + 20);
        let mask = chunk[at + 24];

        let name_bytes = chunk.get(name_offset..)?;
        let end = name_bytes.iter().position(|&b| b == 0)?;
        let name = String::from_utf8_lossy(&name_bytes[..end]).to_string();

        elements.push(SignatureElement {
            name,
            index,
            register,
            mask,
        });
    }

    Some(elements)
}

/// Checks that a replacement container can stand in for the original.
///
/// The shader stage and the semantic layout of both signatures must match. The
/// exact bytes may differ (compiler versions encode signatures differently),
/// so the decoded semantics are compared instead.
fn interface_mismatch(original: &[u8], replacement: &[u8]) -> Option<String> {
    let original_stage = stage_of(original);
    let replacement_stage = stage_of(replacement);
    if original_stage != replacement_stage {
        return Some(format!(
            "stage is {replacement_stage:?}, expected {original_stage:?}"
        ));
    }

    for name in [b"ISG1", b"OSG1"] {
        let original_chunk = find_chunk(original, name);
        let replacement_chunk = find_chunk(replacement, name);

        let (Some(original_chunk), Some(replacement_chunk)) = (original_chunk, replacement_chunk)
        else {
            if original_chunk.is_none() && replacement_chunk.is_none() {
                continue;
            }
            return Some(format!(
                "{} is missing from one of the containers",
                String::from_utf8_lossy(name)
            ));
        };

        let (Some(original_signature), Some(replacement_signature)) = (
            parse_signature(original_chunk),
            parse_signature(replacement_chunk),
        ) else {
            return Some(format!(
                "{} could not be parsed",
                String::from_utf8_lossy(name)
            ));
        };

        if original_signature != replacement_signature {
            return Some(format!(
                "{} differs: original {:?}, replacement {:?}",
                String::from_utf8_lossy(name),
                original_signature,
                replacement_signature
            ));
        }
    }

    None
}

/// Decodes a program frame into its DXBC container.
pub fn decode_frame(frame: &[u8], decoded_length: usize) -> Result<Vec<u8>> {
    let decoded = oodle::decompress(
        frame,
        decoded_length,
        OodleLZ_FuzzSafe::Yes,
        OodleLZ_CheckCRC::No,
    )
    .wrap_err("Failed to decompress shader frame")?;

    if !decoded.starts_with(b"DXBC") {
        bail!("Shader frame did not decode to a DXBC container");
    }

    Ok(decoded)
}

/// Encodes a DXBC container as an Oodle frame and verifies the round trip.
pub fn encode_frame(container: &[u8]) -> Result<Vec<u8>> {
    if !container.starts_with(b"DXBC") {
        bail!("Shader replacement is not a DXBC container");
    }

    let frame = oodle::compress_exact(container).wrap_err("Failed to compress shader frame")?;

    let decoded = oodle::decompress(
        &frame,
        container.len(),
        OodleLZ_FuzzSafe::Yes,
        OodleLZ_CheckCRC::No,
    )
    .wrap_err("Failed to verify compressed shader frame")?;
    if decoded != container {
        bail!("Compressed shader frame does not decode back to the container");
    }

    Ok(frame)
}

/// Parses every program in a device data section.
///
/// Programs are found by their record signature rather than by walking the
/// metadata, whose size is not stored explicitly.
pub fn parse_programs(device: &[u8]) -> Result<Vec<Program>> {
    let mut programs = Vec::new();
    let mut pos = 0usize;

    while pos + 12 <= device.len() {
        let envelope = u32_at(device, pos);
        let frame_length = u32_at(device, pos + 4) as usize;
        let frame_start = pos + 8;

        let plausible = envelope == 1
            && frame_length >= 6
            && frame_start + frame_length + 16 <= device.len()
            && device[frame_start..frame_start + 2] == FRAME_MAGIC;

        if !plausible {
            pos += 1;
            continue;
        }

        let frame = &device[frame_start..frame_start + frame_length];
        let meta_pos = frame_start + frame_length;
        let metadata_kind = u32_at(device, meta_pos);
        let decoded_length = u32_at(device, meta_pos + 4) as usize;
        let frame_key = u64_at(device, meta_pos + 8);

        if metadata_kind != METADATA_KIND {
            pos += 1;
            continue;
        }

        let Ok(container) = decode_frame(frame, decoded_length) else {
            pos += 1;
            continue;
        };

        let stage = stage_of(&container);

        programs.push(Program {
            index: programs.len(),
            pos,
            meta_pos,
            frame_length,
            decoded_length,
            frame_key,
            container,
            stage,
        });

        pos = meta_pos + 16;
    }

    if programs.is_empty() {
        bail!("No shader programs found in device data");
    }

    Ok(programs)
}

/// Rebuilds a shader43 section.
///
/// `replace` is called for each program; returning `Some(container)` replaces
/// the program's DXBC container (the frame is re-compressed and the frame
/// length, decoded length, key and the shader header are updated). Returning
/// `None` keeps the program's record byte for byte.
pub fn rebuild(data: &[u8], replace: impl Fn(&Program) -> Option<Vec<u8>>) -> Result<Vec<u8>> {
    if data.len() < 48 {
        bail!("Shader section is too small");
    }

    let version = u32_at(data, 0);
    if version != VERSION {
        bail!("Unexpected shader version {version}, expected {VERSION}");
    }

    let default_data_offset = u32_at(data, 20) as usize;
    let device_data_offset = u32_at(data, 40) as usize;
    let device_data_size = u32_at(data, 44) as usize;

    let device = data
        .get(device_data_offset..device_data_offset + device_data_size)
        .ok_or_else(|| color_eyre::eyre::eyre!("Device data is out of range"))?;
    let default_block = data
        .get(default_data_offset..)
        .ok_or_else(|| color_eyre::eyre::eyre!("Default data is out of range"))?;

    let programs = parse_programs(device)?;
    let mut new_device = Vec::with_capacity(device.len());

    // Everything before the first program (a device level header) is preserved.
    new_device.extend_from_slice(&device[..programs[0].pos]);

    for (i, program) in programs.iter().enumerate() {
        let next_pos = programs
            .get(i + 1)
            .map(|next| next.pos)
            .unwrap_or(device.len());

        let Some(container) = replace(program) else {
            // Keep the original record exactly, including its compressed frame.
            new_device.extend_from_slice(&device[program.pos..next_pos]);
            continue;
        };

        if let Some(reason) = interface_mismatch(&program.container, &container) {
            bail!(
                "Replacement for shader program {} does not match the original interface \
                 (the other stages are unchanged): {reason}",
                program.index
            );
        }

        let frame = encode_frame(&container)?;
        let tail = &device[program.meta_pos + 16..next_pos];

        new_device.extend_from_slice(&1u32.to_le_bytes());
        new_device.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        new_device.extend_from_slice(&frame);
        new_device.extend_from_slice(&METADATA_KIND.to_le_bytes());
        new_device.extend_from_slice(&(container.len() as u32).to_le_bytes());
        new_device.extend_from_slice(&murmur::hash(&frame, 0).to_le_bytes());
        new_device.extend_from_slice(tail);
    }

    // Rebuild the section, moving the default data block after the device data
    // and recomputing the padding around it.
    let mut new_data = Vec::with_capacity(data.len() + new_device.len());
    new_data.extend_from_slice(&data[..device_data_offset]);
    new_data.extend_from_slice(&new_device);
    while new_data.len() % 4 != 0 {
        new_data.push(0);
    }
    let new_default_data_offset = new_data.len();
    new_data.extend_from_slice(default_block);
    while new_data.len() % 16 != 0 {
        new_data.push(0);
    }

    new_data[20..24].copy_from_slice(&(new_default_data_offset as u32).to_le_bytes());
    new_data[44..48].copy_from_slice(&(new_device.len() as u32).to_le_bytes());

    Ok(new_data)
}

/// One record of the contexts table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRecord {
    /// murmur32 of the context's name: `default`, `shadow_caster`, and the
    /// unnamed ones.
    pub name: u32,
    /// The second word, carried because nothing here sets it.
    pub flags: u32,
    /// The query ids this context answers to, with the offset of the conditions
    /// tree each one selects.
    ///
    /// This is a **list, and the record is variable length**: 12 bytes of header
    /// then eight per query. Measured: the six small families carry one or two
    /// queries per record (20 and 28 bytes), while the UI base's `default`
    /// carries 30 and its second context six (252 and 60 bytes). A fixed 20-byte
    /// reader fits only the families whose records all have one query, which is
    /// why the fixed model survived as long as it did.
    ///
    /// The first query's id is the check that identifies the record: the `default`
    /// context of every shipped section carries the group data's own hash there.
    pub queries: Vec<Query>,
}

/// One query a context answers to: the group it selects and where its conditions
/// start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Query {
    /// The query id - for the first query of `default`, the group data's own
    /// header hash.
    pub id: u32,
    /// The offset of the conditions tree this query selects, or
    /// [`NO_CONDITIONS`] when it has none. All six small families are
    /// [`NO_CONDITIONS`], which is why their conditions sections are 0, 28 and 56
    /// bytes rather than the UI declaration's 1436.
    pub conditions: u32,
}

/// The `conditions` word of a query that selects no conditions tree.
pub const NO_CONDITIONS: u32 = 0xFFFF_FFFF;

/// The fixed part of a contexts record, before its queries: `{name, flags, count}`.
pub const CONTEXT_HEADER_LEN: usize = 12;

/// The record length of one query: a query id and a conditions offset.
pub const QUERY_LEN: usize = 8;

impl ContextRecord {
    /// The record's byte length: 12 bytes plus eight per query.
    pub fn len(&self) -> usize {
        CONTEXT_HEADER_LEN + self.queries.len() * QUERY_LEN
    }

    /// The record's bytes.
    pub fn write(&self, out: &mut Vec<u8>) {
        for word in [self.name, self.flags, self.queries.len() as u32] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for query in &self.queries {
            for word in [query.id, query.conditions] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
    }

    /// Reads a record at an offset, returning it and its length.
    pub fn parse(data: &[u8], at: usize) -> Option<(Self, usize)> {
        let word = |offset: usize| -> Option<u32> {
            Some(u32::from_le_bytes(
                data.get(at + offset..at + offset + 4)?.try_into().ok()?,
            ))
        };
        let name = word(0)?;
        let flags = word(4)?;
        let count = word(8)? as usize;
        let len = CONTEXT_HEADER_LEN.checked_add(count.checked_mul(QUERY_LEN)?)?;
        if at.checked_add(len)? > data.len() {
            return None;
        }
        let mut queries = Vec::with_capacity(count);
        for index in 0..count {
            let query = CONTEXT_HEADER_LEN + index * QUERY_LEN;
            queries.push(Query {
                id: word(query)?,
                conditions: word(query + 4)?,
            });
        }
        Some((
            Self {
                name,
                flags,
                queries,
            },
            len,
        ))
    }
}

/// The engine-side bytes a generated section has to be given, because nothing in a
/// declaration and nothing in a compiled program produces them.
///
/// This is the carried list, in one place: the header words the engine sets, the
/// bytes between the group data and the programs, the programs themselves, and
/// whatever follows them. A [`Section::build`] takes these and the declaration's
/// own parts (the contexts, the conditions and a group data), and writes the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Carried {
    /// The header's second word, which no declaration sets.
    pub opaque: u32,
    /// The header's sixth word, which points into the tail.
    pub default_data: u32,
    /// The bytes between the group data and the programs.
    pub trailing: Vec<u8>,
    /// The programs, Oodle-framed DXBC.
    pub device_data: Vec<u8>,
    /// Everything after the programs.
    pub tail: Vec<u8>,
}

impl Carried {
    /// The carried bytes of a template section, so a generated one can be built
    /// against the same engine data rather than against nothing.
    pub fn of(template: &Section) -> Self {
        Self {
            opaque: template.opaque,
            default_data: template.default_data,
            trailing: template.trailing.clone(),
            device_data: template.device_data.clone(),
            tail: template.tail.clone(),
        }
    }
}

/// A whole shader section, read by the layout and laid out again by it.
///
/// The regions are not independent offsets: the contexts are variable length and
/// fill `[contexts_offset, conditions_offset)` exactly, so `conditions_offset` is
/// `48 + the sum of the context record lengths`. The conditions are a byte blob
/// that runs to `dependencies_offset` - a pool of 28-byte nodes on the small
/// families, a permutation tree on the UI base - and the remaining regions follow
/// it in order. A section read and written with what it was read comes back byte
/// for byte, which is a necessary check but not a sufficient one: a wrong reader
/// and its writer can agree and still be wrong, so the substitution tests and the
/// query/group count check are what test the model.
///
/// The words this does not own are carried: the header's opaque and default-data
/// words, the bytes between the group data and the programs, the programs and
/// whatever follows them. The contexts' second word is carried too; every
/// shipped section has zero there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    version: u32,
    opaque: u32,
    default_data: u32,
    contexts: Vec<ContextRecord>,
    /// The conditions region, a byte-addressed blob: a query's `conditions` word
    /// is a byte offset into it, or [`NO_CONDITIONS`].
    conditions: Vec<u8>,
    dependencies: Vec<Dependency>,
    group_data: Vec<u8>,
    /// The bytes between the group data and the programs.
    ///
    /// The header's group data *size* is 1, 1, 3, 2, 3 and 1 bytes short of the
    /// distance to the device data on the six shipped families, and the shortfall
    /// is not constant, so it is neither alignment nor a fixed header. It is
    /// carried, like the link's second word: this does not know what it is, and
    /// guessing would be the one change in the section that cannot be checked.
    trailing: Vec<u8>,
    device_data: Vec<u8>,
    /// Everything after the programs, carried verbatim.
    ///
    /// The header's sixth word - `default_data_offset` - points into this region,
    /// and it is the last thing in the section. On the six shipped families it
    /// lands at the end of the device data on three of them and two to three
    /// bytes into it on the others, so it is carried with the rest rather than
    /// laid out: what a default-data block holds is not established, and the
    /// section round trips byte for byte with the bytes kept whole.
    tail: Vec<u8>,
}

impl Section {
    /// The length of the 12-word header.
    pub const HEADER_LEN: usize = 48;

    /// Reads a section. The bytes are the section itself, without the 20-byte
    /// prefix a content file wraps it in.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let word = |i: usize| u32_at(bytes, i * 4);
        let (version, opaque) = (word(0), word(1));
        if version != VERSION {
            bail!("the section is version {version}, not {VERSION}");
        }
        let (contexts_at, contexts_n) = (word(2) as usize, word(3) as usize);
        let conditions_at = word(4) as usize;
        let default_data = word(5);
        let (dependencies_at, dependencies_n) = (word(6) as usize, word(7) as usize);
        let (group_at, group_len) = (word(8) as usize, word(9) as usize);
        let (device_at, device_len) = (word(10) as usize, word(11) as usize);

        // The contexts are variable length and fill [contexts_offset,
        // conditions_offset) exactly - there is no link table between them. That
        // is measured on all seven sections read so far: the six small families,
        // whose records are 20 or 28 bytes, and the UI base, whose two records
        // are 252 and 60 bytes and whose 36 queries are its 36 groups.
        let mut cursor = contexts_at;
        let mut contexts = Vec::with_capacity(contexts_n);
        for _ in 0..contexts_n {
            let (context, len) = ContextRecord::parse(bytes, cursor)
                .ok_or_else(|| eyre!("a context record at {cursor} did not read"))?;
            cursor += len;
            contexts.push(context);
        }
        if cursor != conditions_at {
            bail!("the contexts end at {cursor} but the conditions start at {conditions_at}");
        }
        // The conditions region is a byte blob addressed by the queries, so it is
        // sliced whole rather than parsed: a pool of 28-byte nodes on the small
        // families, a permutation tree on the UI base. Decoding the tree is open
        // work; until then the bytes are carried.
        let Some(conditions) = check_range(bytes, conditions_at, dependencies_at) else {
            bail!(
                "the conditions region {conditions_at}..{dependencies_at} is out of range \
                 or runs backwards"
            );
        };

        let slice = |at: usize, len: usize| -> Result<Vec<u8>> {
            bytes
                .get(at..at + len)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| eyre!("the region at {at} x {len} is out of range"))
        };

        let section = Self {
            version,
            opaque,
            default_data,
            contexts,
            conditions,
            dependencies: Dependency::read(bytes, dependencies_at, dependencies_n),
            group_data: slice(group_at, group_len)?,
            trailing: slice(
                group_at + group_len,
                device_at.saturating_sub(group_at + group_len),
            )?,
            device_data: slice(device_at, device_len)?,
            tail: bytes
                .get(device_at + device_len..)
                .unwrap_or_default()
                .to_vec(),
        };
        section.check()?;
        Ok(section)
    }

    /// The two structural invariants that a wrong reader can miss.
    ///
    /// The first is measured on all seven sections read so far: the queries
    /// across the contexts are the groups, one query per group. The second is
    /// what makes a query's `conditions` word usable: it is a byte offset into
    /// the conditions blob, or [`NO_CONDITIONS`].
    ///
    /// The default/group-0 relation is checked too: the first context's first
    /// query is the group data's own hash on all seven, which is how a context
    /// names the group it selects.
    pub fn check(&self) -> Result<()> {
        if self.group_data.len() < 8 {
            bail!("the group data is too short for its header");
        }
        let groups = u32::from_le_bytes(self.group_data[0..4].try_into().unwrap());
        let hash = u32::from_le_bytes(self.group_data[4..8].try_into().unwrap());
        let queries: usize = self.contexts.iter().map(|context| context.queries.len()).sum();
        if queries as u32 != groups {
            bail!(
                "the contexts carry {queries} queries and the group data has {groups} groups"
            );
        }
        for (index, context) in self.contexts.iter().enumerate() {
            for (query_index, query) in context.queries.iter().enumerate() {
                if query.conditions != NO_CONDITIONS
                    && query.conditions as usize >= self.conditions.len()
                {
                    bail!(
                        "context {index} query {query_index} selects conditions at {:#x}, \
                         past the {} byte blob",
                        query.conditions,
                        self.conditions.len()
                    );
                }
            }
        }
        let default = self
            .contexts
            .first()
            .and_then(|context| context.queries.first());
        if !matches!(default, Some(query) if query.id == hash) {
            bail!("the first context has no query for the group data's hash {hash:08X}");
        }
        Ok(())
    }

    /// The contexts, in table order.
    pub fn contexts(&self) -> &[ContextRecord] {
        &self.contexts
    }

    /// The contexts, for a section that is changing one - adding or dropping a
    /// context is what moves every offset after it.
    pub fn contexts_mut(&mut self) -> &mut Vec<ContextRecord> {
        &mut self.contexts
    }

    /// Replaces the group data, for a section whose group data has been rebuilt.
    ///
    /// This is the seam the write path uses: the group data is rebuilt by
    /// [`crate::filetype::group_data::GroupData::rebuild`] from the declaration's
    /// own variables, and handed back here. The group's count and hash in the new
    /// bytes are what the section reports, so a change to one cannot leave the
    /// other stale.
    pub fn set_group_data(&mut self, bytes: Vec<u8>) {
        self.group_data = bytes;
    }

    /// The conditions blob: the bytes between the contexts and the dependencies,
    /// addressed by each query's `conditions` word.
    pub fn conditions(&self) -> &[u8] {
        &self.conditions
    }

    /// The dependencies table.
    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }

    /// The group data's bytes.
    pub fn group_data(&self) -> &[u8] {
        &self.group_data
    }

    /// The device data: the programs, carried verbatim.
    pub fn device_data(&self) -> &[u8] {
        &self.device_data
    }

    /// Builds a section from the declaration's own parts and the engine's.
    ///
    /// This is the "no shipped blob" path. [`Section::parse`] is the only other
    /// way to make a [`Section`], so without this a section could only ever be a
    /// template with things changed in it - and the whole carried list exists
    /// because a declaration cannot produce these bytes. What a declaration *can*
    /// produce is the contexts and the conditions, and what the compiler produces
    /// is the group data; everything else arrives in `carried`.
    ///
    /// [`Section::check`] is what makes this a generator rather than a writer: the
    /// queries must be the group data's groups, one each, and their conditions
    /// offsets must land in the blob. A section whose contexts and group data
    /// disagree is refused here rather than written, because that is the one
    /// mistake a round trip could never catch - there would be no template to
    /// catch it against.
    pub fn build(
        contexts: &[ContextRecord],
        conditions: Vec<u8>,
        group_data: Vec<u8>,
        carried: &Carried,
    ) -> Result<Self> {
        if contexts.is_empty() {
            bail!("a section needs at least the default context");
        }
        let section = Self {
            version: VERSION,
            opaque: carried.opaque,
            default_data: carried.default_data,
            contexts: contexts.to_vec(),
            conditions,
            dependencies: vec![Dependency::of()],
            group_data,
            trailing: carried.trailing.clone(),
            device_data: carried.device_data.clone(),
            tail: carried.tail.clone(),
        };
        section.check()?;
        Ok(section)
    }

    /// Lays the section out again, recomputing every offset.
    pub fn into_bytes(self) -> Vec<u8> {
        let contexts_len: usize = self.contexts.iter().map(ContextRecord::len).sum();
        let dependencies_len = self.dependencies.len() * DEPENDENCY_LEN;
        let contexts_at = Self::HEADER_LEN;
        let conditions_at = contexts_at + contexts_len;
        let dependencies_at = conditions_at + self.conditions.len();
        let group_at = dependencies_at + dependencies_len;
        let device_at = group_at + self.group_data.len() + self.trailing.len();

        let mut out = Vec::with_capacity(device_at + self.device_data.len());
        for word in [
            self.version,
            self.opaque,
            contexts_at as u32,
            self.contexts.len() as u32,
            conditions_at as u32,
            self.default_data,
            dependencies_at as u32,
            self.dependencies.len() as u32,
            group_at as u32,
            self.group_data.len() as u32,
            device_at as u32,
            self.device_data.len() as u32,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for context in &self.contexts {
            context.write(&mut out);
        }
        out.extend_from_slice(&self.conditions);
        for dependency in &self.dependencies {
            out.extend_from_slice(&dependency.write());
        }
        out.extend_from_slice(&self.group_data);
        out.extend_from_slice(&self.trailing);
        out.extend_from_slice(&self.device_data);
        out.extend_from_slice(&self.tail);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_can_be_built_without_a_blob() {
        // The "no shipped blob" path: the contexts and the group data come from
        // the declaration and the compiler, and only the engine bytes are carried.
        // It has to lay out and read back with nothing to compare against.
        let mut group_data = vec![0u8; 8];
        group_data[0..4].copy_from_slice(&1u32.to_le_bytes()); // one group
        group_data[4..8].copy_from_slice(&0x8BE2_82AAu32.to_le_bytes());
        group_data.extend_from_slice(&[0x11; 88]);
        let carried = Carried {
            opaque: 0x1234_5678,
            default_data: 0,
            trailing: vec![0x00],
            device_data: vec![0x22; 24],
            tail: vec![0x33; 8],
        };
        let contexts = [ContextRecord {
            name: 0xF276_0503,
            flags: 0,
            queries: vec![Query {
                id: 0x8BE2_82AA,
                conditions: NO_CONDITIONS,
            }],
        }];
        let built =
            Section::build(&contexts, Vec::new(), group_data.clone(), &carried).expect("built");
        let bytes = built.into_bytes();
        let back = Section::parse(&bytes).expect("reads back");
        assert_eq!(back.contexts().len(), 1);
        assert_eq!(back.contexts()[0].queries[0].id, 0x8BE2_82AA);
        assert_eq!(back.conditions(), b"", "a single group needs no conditions");
        assert_eq!(back.group_data(), group_data.clone());
        assert_eq!(back.device_data(), carried.device_data);
        assert_eq!(back.into_bytes(), bytes, "and it is stable");
    }

    #[test]
    fn a_built_section_refuses_contexts_that_do_not_match_the_group_data() {
        // The mistakes a round trip could never catch, because a generated
        // section has no template to catch it against.
        let mut group_data = vec![0u8; 8];
        group_data[0..4].copy_from_slice(&1u32.to_le_bytes());
        group_data[4..8].copy_from_slice(&0x8BE2_82AAu32.to_le_bytes());
        let carried = Carried::default();
        let stale = ContextRecord {
            name: 0xF276_0503,
            flags: 0,
            queries: vec![Query {
                id: 0xDEAD_BEEF,
                conditions: NO_CONDITIONS,
            }],
        };
        assert!(
            Section::build(&[stale.clone()], Vec::new(), group_data.clone(), &carried).is_err(),
            "a first context whose query is not the group data's hash is refused"
        );
        let empty = ContextRecord {
            queries: Vec::new(),
            ..stale.clone()
        };
        assert!(
            Section::build(&[empty], Vec::new(), group_data.clone(), &carried).is_err(),
            "a context with no query at all is refused"
        );
        let two = ContextRecord {
            name: 0xF276_0503,
            flags: 0,
            queries: vec![
                Query {
                    id: 0x8BE2_82AA,
                    conditions: NO_CONDITIONS,
                },
                Query {
                    id: 0x8BE2_82AA,
                    conditions: NO_CONDITIONS,
                },
            ],
        };
        assert!(
            Section::build(&[two], Vec::new(), group_data, &carried).is_err(),
            "queries that do not equal the group count are refused"
        );
    }

    /// A section with variable-length contexts: the first record is 28 bytes (two
    /// queries), the second 20 (one), three queries against three groups, and a
    /// 28-byte conditions blob the first query addresses.
    fn section() -> Section {
        let mut group_data = vec![0u8; 8];
        group_data[0..4].copy_from_slice(&3u32.to_le_bytes());
        group_data[4..8].copy_from_slice(&0x8BE2_82AAu32.to_le_bytes());
        group_data.extend_from_slice(&[0x11; 96]);
        Section {
            version: VERSION,
            opaque: 0x1234_5678,
            default_data: 0,
            contexts: vec![
                ContextRecord {
                    name: 0xF276_0503,
                    flags: 0,
                    queries: vec![
                        Query {
                            id: 0x8BE2_82AA,
                            conditions: 0,
                        },
                        Query {
                            id: 0x99C0_9062,
                            conditions: NO_CONDITIONS,
                        },
                    ],
                },
                ContextRecord {
                    name: 0x3100_C3D2,
                    flags: 0,
                    queries: vec![Query {
                        id: 0xC580_0413,
                        conditions: NO_CONDITIONS,
                    }],
                },
            ],
            conditions: vec![0xAB; 28],
            dependencies: vec![Dependency::of()],
            group_data,
            trailing: vec![0x00, 0x00, 0x00],
            device_data: vec![0x22; 40],
            tail: vec![0x33; 16],
        }
    }

    #[test]
    fn a_section_laid_out_by_the_layout_comes_back_byte_for_byte() {
        // Read a section and write it with what was read: nothing may move. This
        // is necessary but not sufficient - a wrong reader and writer can agree -
        // so it is paired with the query count and conditions bounds checks.
        let bytes = section().into_bytes();
        let read = Section::parse(&bytes).expect("the section reads");
        assert_eq!(read.contexts().len(), 2);
        assert_eq!(read.contexts()[0].len(), 28);
        assert_eq!(read.contexts()[1].len(), 20);
        assert_eq!(read.conditions().len(), 28);
        assert_eq!(read.dependencies().len(), 1);
        assert_eq!(read.group_data().len(), 104);
        assert_eq!(read.device_data().len(), 40);
        assert_eq!(read.into_bytes(), bytes, "a section round trips");
    }

    #[test]
    fn the_layout_is_the_sum_of_the_context_records() {
        // 48 + 28 + 20 is where the conditions start, and the header says so
        // rather than the writer having copied an offset.
        let bytes = section().into_bytes();
        assert_eq!(u32_at(&bytes, 8), 48, "contexts start after the header");
        assert_eq!(u32_at(&bytes, 16), 48 + 28 + 20);
        assert_eq!(
            u32_at(&bytes, 24),
            u32_at(&bytes, 16) + 28,
            "dependencies follow the conditions blob"
        );
        assert_eq!(
            u32_at(&bytes, 32),
            u32_at(&bytes, 24) + 8,
            "the group data follows the eight byte entry"
        );

        // An offset that runs backwards is a header to refuse, not to subtract.
        let mut wrong = bytes.clone();
        wrong[24..28].copy_from_slice(&0u32.to_le_bytes()); // dependencies at 0
        assert!(
            Section::parse(&wrong).is_err(),
            "a conditions offset that runs backwards is refused"
        );
    }

    #[test]
    fn a_conditions_offset_past_the_blob_is_refused() {
        let mut wrong = section();
        wrong.contexts[1].queries[0].conditions = 28; // the blob is 28 bytes
        let bytes = wrong.into_bytes();
        assert!(
            Section::parse(&bytes).is_err(),
            "a query whose conditions offset is past the blob is refused"
        );
    }

    #[test]
    fn a_section_version_is_checked() {
        let mut bytes = section().into_bytes();
        bytes[0..4].copy_from_slice(&44u32.to_le_bytes());
        assert!(
            Section::parse(&bytes).is_err(),
            "a version is not 43 is refused"
        );
    }

    #[test]
    fn stage_from_psv_kind() {
        assert_eq!(Stage::from_psv_kind(0), Stage::Pixel);
        assert_eq!(Stage::from_psv_kind(1), Stage::Vertex);
        assert_eq!(Stage::from_psv_kind(9), Stage::Other);
    }

    #[test]
    fn tail_round_trips() {
        // A tail shaped like the shipped ones: one constant buffer (c_per_object
        // hashed by hand, 240 bytes) then resource list words and a block.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for word in [0xB5639618u32, 0, 240, 0, 1, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[0u8; 32]);

        let tail = Tail::parse(&bytes).expect("a well formed tail");
        assert_eq!(tail.cbuffers.len(), 1);
        assert_eq!(tail.cbuffers[0].name_hash(), 0xB5639618);
        assert_eq!(tail.cbuffers[0].size(), 240);
        assert_eq!(tail.bytes(), bytes);
    }

    #[test]
    fn tail_rejects_garbage() {
        // A size that is not a multiple of 16, and a count that runs past the
        // end of the tail, must both fail to parse.
        let mut bad_size = Vec::new();
        bad_size.extend_from_slice(&1u32.to_le_bytes());
        for word in [1u32, 0, 33, 0, 0, 0] {
            bad_size.extend_from_slice(&word.to_le_bytes());
        }
        assert!(Tail::parse(&bad_size).is_none());

        let mut short = Vec::new();
        short.extend_from_slice(&4u32.to_le_bytes());
        short.extend_from_slice(&[0u8; 24]);
        assert!(Tail::parse(&short).is_none());
    }
}
