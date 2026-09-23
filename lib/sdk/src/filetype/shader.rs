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

use color_eyre::eyre::{Context, Result, bail};
use oodle::{OodleLZ_CheckCRC, OodleLZ_FuzzSafe};

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

/// Shader stage of a DXBC container, read from its `PSV0` chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq)]
struct SignatureElement {
    name: String,
    index: u32,
    register: u32,
    mask: u8,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_from_psv_kind() {
        assert_eq!(Stage::from_psv_kind(0), Stage::Pixel);
        assert_eq!(Stage::from_psv_kind(1), Stage::Vertex);
        assert_eq!(Stage::from_psv_kind(9), Stage::Other);
    }
}
