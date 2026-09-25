//! Family presets: generating a `shader43` section from our own programs.
//!
//! A shader *family* (the `gui` UI shader, the entity shader, ...) shares an
//! engine-side wrapper: the contexts the engine queries, the conditions tree
//! that selects a group, the group data with its variable tables, the packed
//! device preamble and one metadata tail per program. A [`Preset`] captures
//! that wrapper once from a shipped base material, so a mod can generate a
//! complete section from its own compiled programs instead of shipping a
//! shipped shader blob.
//!
//! ```text
//! let preset = Preset::from_material(&data)?;          // extract once per family
//! std::fs::write("ui.preset", preset.to_text())?;
//!
//! let preset = Preset::from_text(&text)?;              // generate from it
//! let section = preset.generate(&containers)?;         // Stage -> DXBC container
//! ```

use std::collections::HashMap;
use std::fs;

use color_eyre::eyre::{Context, Result, bail};

use super::shader::{self, Stage};
use crate::murmur;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Resolves a slot or variable token: eight hex digits are used as the hash
/// directly, anything else is hashed like the engine hashes names.
fn hash_token(token: &str) -> u32 {
    if token.len() == 8 && token.chars().all(|c| c.is_ascii_hexdigit()) {
        u32::from_str_radix(token, 16).unwrap_or(0)
    } else {
        u32::from(murmur::Murmur32::hash(token))
    }
}

/// Sizes the engine uses for material variable records, by record type.
const VARIABLE_SIZES: [(u32, u32); 5] = [(0, 4), (1, 8), (2, 12), (3, 16), (4, 64)];

/// Reads a 20 byte group data variable record, if the bytes look like one.
fn read_variable(data: &[u8], at: usize) -> Option<(u32, u32, u32, u32, u32)> {
    if at + 20 > data.len() {
        return None;
    }
    let kind = u32_at(data, at);
    let flags = u32_at(data, at + 4);
    let hash = u32_at(data, at + 8);
    let offset = u32_at(data, at + 12);
    let size = u32_at(data, at + 16);
    if hash == 0 || kind > 12 || flags > 3 || offset > 8192 {
        return None;
    }
    if let Some(&(.., expected)) = VARIABLE_SIZES.iter().find(|(code, ..)| *code == kind)
        && size != expected
    {
        return None;
    }
    Some((kind, flags, hash, offset, size))
}

/// Rewrites the variable's record everywhere it occurs in the group data: the
/// canonical 20 byte records and the verbatim copies the packed serialization
/// embeds at arbitrary (often unaligned) offsets. The engine reads the packed
/// copies too, so patching only the aligned records leaves the old offset in
/// effect and the new slot is never written. Returns the number of records
/// replaced and the slot's previous `(offset, size)`, which callers use to grow
/// the constant buffer in the program tails.
fn patch_variable(
    data: &mut [u8],
    slot: u32,
    name: u32,
    offset: u32,
    size: u32,
) -> (usize, Option<(u32, u32)>) {
    let Some(&(kind, ..)) = VARIABLE_SIZES.iter().find(|(.., bytes)| *bytes == size) else {
        return (0, None);
    };

    // Learn the slot's current record (kind, flags, offset, size) from an
    // aligned one, then replace that exact byte sequence wherever it appears.
    let mut old = None;
    for at in (0..data.len().saturating_sub(19)).step_by(4) {
        let Some((kind, flags, hash, old_offset, old_size)) = read_variable(data, at) else {
            continue;
        };
        if hash == slot {
            old = Some((kind, flags, old_offset, old_size));
            break;
        }
    }
    let Some((slot_kind, flags, slot_offset, slot_size)) = old else {
        return (0, None);
    };

    let mut pattern = Vec::with_capacity(20);
    for word in [slot_kind, flags, slot, slot_offset, slot_size] {
        pattern.extend_from_slice(&word.to_le_bytes());
    }
    let mut replacement = Vec::with_capacity(20);
    for word in [kind, flags, name, offset, size] {
        replacement.extend_from_slice(&word.to_le_bytes());
    }

    let mut patched = 0;
    let mut at = 0;
    while at + 20 <= data.len() {
        if data[at..at + 20] == pattern {
            data[at..at + 20].copy_from_slice(&replacement);
            patched += 1;
            at += 20;
        } else {
            at += 1;
        }
    }
    (patched, Some((slot_offset, slot_size)))
}

/// Clones a channel's group data records: the canonical 20 byte variable
/// records and the packed 28 byte copies, bumping each run's count. Returns how
/// many records were inserted.
fn clone_channel_group_data(data: &mut Vec<u8>, template: u32, name: u32) -> usize {
    struct Insert {
        at: usize,
        bytes: Vec<u8>,
        count_at: usize,
    }

    /// A packed record carries the cbuffer hash and a zero at +20/+24.
    fn packed(data: &[u8], at: usize) -> bool {
        at + 28 <= data.len() && u32_at(data, at + 20) > 0xFFFF && u32_at(data, at + 24) == 0
    }

    let mut inserts: Vec<Insert> = Vec::new();

    // Canonical runs of aligned 20 byte records, each preceded by its count.
    let mut at = 0;
    while at + 20 <= data.len() {
        if read_variable(data, at).map(|record| record.2) == Some(template) {
            let mut start = at;
            while start >= 20 && read_variable(data, start - 20).is_some() {
                start -= 20;
            }
            let mut end = at + 20;
            while read_variable(data, end).is_some() {
                end += 20;
            }
            let records = (end - start) / 20;
            if start >= 4 && u32_at(data, start - 4) as usize == records {
                let mut clone = data[at..at + 20].to_vec();
                clone[8..12].copy_from_slice(&name.to_le_bytes());
                inserts.push(Insert {
                    at: at + 20,
                    bytes: clone,
                    count_at: start - 4,
                });
            }
        }
        at += 4;
    }

    // Packed runs of 28 byte records, the count once before the run.
    let mut at = 0;
    while at + 28 <= data.len() {
        if u32_at(data, at) == template && packed(data, at) {
            let mut start = at;
            while start >= 28 && packed(data, start - 28) {
                start -= 28;
            }
            let mut end = at + 28;
            while packed(data, end) {
                end += 28;
            }
            let records = (end - start) / 28;
            if start >= 4 && u32_at(data, start - 4) as usize == records {
                let mut clone = data[at..at + 28].to_vec();
                clone[0..4].copy_from_slice(&name.to_le_bytes());
                inserts.push(Insert {
                    at: at + 28,
                    bytes: clone,
                    count_at: start - 4,
                });
            }
        }
        at += 1;
    }

    // Apply back to front so earlier offsets stay valid; every insertion bumps
    // its run's count.
    inserts.sort_by_key(|insert| insert.at);
    let mut cloned = 0;
    for insert in inserts.into_iter().rev() {
        data.splice(insert.at..insert.at, insert.bytes);
        let count = u32_at(data, insert.count_at) + 1;
        data[insert.count_at..insert.count_at + 4].copy_from_slice(&count.to_le_bytes());
        cloned += 1;
    }
    cloned
}

/// Appends a copy of the `template` record to every run of consecutive records
/// that contains it, renamed to `name` with the given `offset` and `size`. The
/// run's count word is bumped when one can be found (the word just before the
/// run or, in copies that use a `{kind, count}` header, the word before that).
/// Returns the number of records added and the template's `(offset, size)`,
/// which callers use to grow the constant buffer in the program tails.
fn clone_variable(
    data: &mut Vec<u8>,
    template: u32,
    name: u32,
    offset: u32,
    size: u32,
) -> (usize, Option<(u32, u32)>) {
    // Canonical occurrences of the template record (aligned or not).
    let mut occurrences = Vec::new();
    let mut at = 0;
    while at + 20 <= data.len() {
        if read_variable(data, at).map(|record| record.2) == Some(template) {
            occurrences.push(at);
            at += 20;
        } else {
            at += 1;
        }
    }
    let old = occurrences
        .first()
        .and_then(|at| read_variable(data, *at))
        .map(|(_, _, _, offset, size)| (offset, size));

    let mut cloned = 0;
    // Work backwards so insertions do not move the occurrences still to come.
    for occurrence in occurrences.into_iter().rev() {
        let Some((kind, flags, ..)) = read_variable(data, occurrence) else {
            continue;
        };
        // Expand to the surrounding run of records.
        let mut start = occurrence;
        while start >= 20 && read_variable(data, start - 20).is_some() {
            start -= 20;
        }
        let mut end = occurrence + 20;
        while read_variable(data, end).is_some() {
            end += 20;
        }
        let records = (end - start) / 20;
        // Look for the run's count in the 16 bytes before it (a `{kind, count}`
        // header puts it one word before the first record).
        let mut count_at = None;
        for candidate in (start.saturating_sub(16)..start).step_by(4).rev() {
            if u32_at(data, candidate) as usize == records {
                count_at = Some(candidate);
                break;
            }
        }

        let mut record = Vec::with_capacity(20);
        for word in [kind, flags, name, offset, size] {
            record.extend_from_slice(&word.to_le_bytes());
        }
        data.splice(end..end, record);
        if let Some(count_at) = count_at {
            let count = (records + 1) as u32;
            data[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
        }
        cloned += 1;
    }
    (cloned, old)
}

/// Grows the constant buffer in every program tail that covered `old_end` before
/// the change so it reaches `needed`.
fn grow_tails(programs: &mut [(Stage, Vec<u8>)], old_end: u32, needed: u32) {
    for (_, tail) in programs {
        let Some(mut parsed) = shader::Tail::parse(tail) else {
            continue;
        };
        let mut changed = false;
        for entry in &mut parsed.cbuffers {
            if entry.size() >= old_end && entry.size() < needed {
                entry.words[2] = needed;
                changed = true;
            }
        }
        if changed {
            *tail = parsed.bytes();
        }
    }
}

/// Replaces every occurrence of a 32 bit name hash with another, returning how
/// many were replaced. Used for channel renames, whose names are not part of a
/// fixed record shape.
fn replace_hash(data: &mut [u8], old: u32, new: u32) -> usize {
    let old = old.to_le_bytes();
    let new = new.to_le_bytes();
    let mut replaced = 0;
    let mut at = 0;
    while at + 4 <= data.len() {
        if data[at..at + 4] == old {
            data[at..at + 4].copy_from_slice(&new);
            replaced += 1;
            at += 4;
        } else {
            at += 1;
        }
    }
    replaced
}

/// The length of a block channel record, by its `kind`: texture channels are
/// 60 bytes (kind 4) or 73 bytes (kind 5). Other kinds are only known to exist
/// (kind 2 is `global_texture2D`), not how long they are.
pub fn channel_record_len(kind: u32) -> Option<usize> {
    match kind {
        4 => Some(60),
        5 => Some(73),
        _ => None,
    }
}

/// Finds a block channel record by name hash, returning its `(offset, kind,
/// len)`. A record starts with the name hash and carries a `count` of one.
fn find_channel_record(data: &[u8], hash: u32) -> Option<(usize, u32, usize)> {
    let needle = hash.to_le_bytes();
    let mut at = 0;
    while at + 12 <= data.len() {
        if data[at..at + 4] == needle
            && let Some(len) = channel_record_len(u32_at(data, at + 4))
            && u32_at(data, at + 8) == 1
            && at + len <= data.len()
        {
            return Some((at, u32_at(data, at + 4), len));
        }
        at += 1;
    }
    None
}

/// Finds the channel record stream at the end of a preamble or program tail: the
/// chain of records that consumes the buffer exactly, returning its start
/// offset (the word before it is the record count) and its record count.
fn find_channel_stream(data: &[u8]) -> Option<(usize, usize)> {
    for start in 0..data.len() {
        let mut at = start;
        let mut count = 0;
        while at < data.len() {
            if at + 12 > data.len() || u32_at(data, at + 8) != 1 {
                break;
            }
            let Some(len) = channel_record_len(u32_at(data, at + 4)) else {
                break;
            };
            if at + len > data.len() {
                break;
            }
            at += len;
            count += 1;
        }
        if at == data.len() && count > 0 {
            return Some((start, count));
        }
    }
    None
}

/// Inserts `replacement` directly after every occurrence of `template` in
/// `data`, returning how many copies were inserted. Used to add a channel
/// record to the preamble and to each program tail's block.
fn clone_record(data: &mut Vec<u8>, template: &[u8], replacement: &[u8]) -> usize {
    let mut cloned = 0;
    let mut at = 0;
    while at + template.len() <= data.len() {
        if data[at..at + template.len()] == *template {
            data.splice(
                at + template.len()..at + template.len(),
                replacement.iter().copied(),
            );
            cloned += 1;
            at += template.len() + replacement.len();
        } else {
            at += 1;
        }
    }
    cloned
}

fn from_hex(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("hex string has an odd length");
    }

    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        bytes.push(
            u8::from_str_radix(std::str::from_utf8(pair)?, 16).wrap_err("invalid hex string")?,
        );
    }
    Ok(bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02X}");
    }
    text
}

/// A variable slot re-purpose: the record of a shipped variable is rewritten to
/// a new name, offset and size. The record type follows from the size (4 = float,
/// 8 = float2, 12 = float3, 16 = float4, 64 = float4x4), which is exactly how the
/// engine stores material variables in a group's tables.
#[derive(Clone, Debug)]
pub struct VariableRewrite {
    /// The shipped variable whose record is taken over: a name or an 8 digit
    /// murmur32 hash.
    pub slot: String,
    /// The new variable's name (or its 8 digit hash), as Lua will address it.
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

/// A new variable slot: a copy of a shipped record is appended to every run of
/// records that contains it, with a new name, offset and size. Unlike a
/// [`VariableRewrite`] this does not take a shipped slot away; the group data
/// grows by one record per copy. The run's count word is grown when it can be
/// found (the word before the run, or in the `{kind, count}` header before it).
#[derive(Clone, Debug)]
pub struct VariableClone {
    /// The shipped variable whose record is copied: a name or an 8 digit
    /// murmur32 hash.
    pub template: String,
    /// The new variable's name (or its 8 digit hash), as Lua will address it.
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

/// A channel (texture) rename: the shipped channel's name hash is replaced by a
/// new one everywhere it occurs - the group data records (canonical and packed),
/// the device preamble and every program tail's block - so a material can bind
/// its own texture under a new channel name. Offsets, sizes and registers stay
/// as they are; only the name changes.
#[derive(Clone, Debug)]
pub struct ChannelRename {
    /// The shipped channel whose name is taken over: a name or an 8 digit
    /// murmur32 hash.
    pub slot: String,
    /// The new channel name (or its 8 digit hash), as the material and its
    /// textures block will address it.
    pub name: String,
}

/// A new channel (texture) record: a copy of a shipped channel's block record
/// with a new name hash. The block's channel records are fixed-length per kind
/// (a record is `{name_hash, kind, count}` plus byte-packed flags; kind 4
/// records are 60 bytes, kind 5 are 73), so a copy is inserted directly after
/// every occurrence of the template record in the preamble and in each program
/// tail's block. Unlike [`ChannelRename`] this leaves the shipped channel alone
/// and adds one; the group data descriptor for the new channel is not generated
/// yet.
#[derive(Clone, Debug)]
pub struct ChannelClone {
    /// The shipped channel whose record is copied: a name or an 8 digit
    /// murmur32 hash.
    pub template: String,
    /// The new channel name (or its 8 digit hash), as the material and its
    /// textures block will address it.
    pub name: String,
}

/// The wrapper of one shader family.
pub struct Preset {
    pub version: u32,
    pub opaque: u32,
    pub context_count: u32,
    pub dependency_count: u32,
    pub contexts: Vec<u8>,
    pub conditions: Vec<u8>,
    pub dependencies: Vec<u8>,
    pub group_data: Vec<u8>,
    /// The packed table before the first program record.
    pub device_preamble: Vec<u8>,
    /// One entry per program of the template's device data, in order.
    pub programs: Vec<(Stage, Vec<u8>)>,
    /// Distinct program tails in the order `program` lines reference them.
    pub tails: Vec<Vec<u8>>,
    /// Variable slots to re-purpose when the section is generated.
    pub variables: Vec<VariableRewrite>,
    /// Variable slots to add when the section is generated.
    pub clones: Vec<VariableClone>,
    /// Channel names to rename when the section is generated.
    pub channels: Vec<ChannelRename>,
    /// Channel records to add when the section is generated.
    pub channel_clones: Vec<ChannelClone>,
}

impl Preset {
    /// Extracts the wrapper of a raw material data file's shader section.
    pub fn from_material(data: &[u8]) -> Result<Self> {
        if data.len() < 28 {
            bail!("material is too small");
        }

        let shader_offset = u32_at(data, 12) as usize;
        let shader_size = u32_at(data, 16) as usize;
        let section = data
            .get(shader_offset..shader_offset + shader_size)
            .ok_or_else(|| color_eyre::eyre::eyre!("shader section is out of range"))?;

        let slice = |start: usize, end: usize| -> Result<Vec<u8>> {
            section
                .get(start..end)
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("section slice {start:#x}..{end:#x} out of range")
                })
        };

        let contexts = slice(u32_at(section, 8) as usize, u32_at(section, 16) as usize)?;
        let conditions = slice(u32_at(section, 16) as usize, u32_at(section, 24) as usize)?;
        let dependencies = slice(u32_at(section, 24) as usize, u32_at(section, 32) as usize)?;
        let group_data = slice(
            u32_at(section, 32) as usize,
            u32_at(section, 32) as usize + u32_at(section, 36) as usize,
        )?;

        let device_offset = u32_at(section, 40) as usize;
        let device_size = u32_at(section, 44) as usize;
        let device = section
            .get(device_offset..device_offset + device_size)
            .ok_or_else(|| color_eyre::eyre::eyre!("device data is out of range"))?;
        let programs = shader::parse_programs(device)?;

        let first_program = programs.first().map(|program| program.pos).unwrap_or(0);
        let device_preamble = device
            .get(..first_program)
            .ok_or_else(|| color_eyre::eyre::eyre!("device preamble is out of range"))?
            .to_vec();

        let programs = programs
            .iter()
            .map(|program| {
                let tail_start = program.meta_pos + 16;
                let tail_end = programs
                    .iter()
                    .find(|next| next.pos > tail_start)
                    .map(|next| next.pos)
                    .unwrap_or(device.len());
                let tail = device
                    .get(tail_start..tail_end)
                    .ok_or_else(|| color_eyre::eyre::eyre!("program tail is out of range"))?
                    .to_vec();
                Ok((program.stage, tail))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            version: u32_at(section, 0),
            opaque: u32_at(section, 4),
            context_count: u32_at(section, 12),
            dependency_count: u32_at(section, 28),
            contexts,
            conditions,
            dependencies,
            group_data,
            device_preamble,
            programs,
            tails: Vec::new(),
            variables: Vec::new(),
            clones: Vec::new(),
            channels: Vec::new(),
            channel_clones: Vec::new(),
        })
    }

    /// Parses a preset from its text form.
    pub fn from_text(text: &str) -> Result<Self> {
        let mut preset = Self {
            version: 43,
            opaque: 0,
            context_count: 0,
            dependency_count: 0,
            contexts: Vec::new(),
            conditions: Vec::new(),
            dependencies: Vec::new(),
            group_data: Vec::new(),
            device_preamble: Vec::new(),
            programs: Vec::new(),
            tails: Vec::new(),
            variables: Vec::new(),
            clones: Vec::new(),
            channels: Vec::new(),
            channel_clones: Vec::new(),
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Program lines carry two fields (stage plus tail hex, or `#n` to
            // refer to a deduplicated tail declared below).
            if let Some(rest) = line.strip_prefix("program ") {
                let (stage, tail) = rest
                    .split_once(' ')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed program line"))?;
                let stage = match stage {
                    "Vertex" => Stage::Vertex,
                    "Pixel" => Stage::Pixel,
                    other => bail!("unsupported program stage '{other}'"),
                };
                let tail = match tail.strip_prefix('#') {
                    Some(index) => preset
                        .tails
                        .get(index.parse::<usize>()?)
                        .cloned()
                        .ok_or_else(|| color_eyre::eyre::eyre!("unknown tail #{index}"))?,
                    None => from_hex(tail)?,
                };
                preset.programs.push((stage, tail));
                continue;
            }

            // Deduplicated tails, referenced by `program <stage> #n`.
            if let Some(rest) = line.strip_prefix("tail ") {
                let (index, hex) = rest
                    .split_once(' ')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed tail line"))?;
                let index = index.parse::<usize>()?;
                if preset.tails.len() <= index {
                    preset.tails.resize(index + 1, Vec::new());
                }
                preset.tails[index] = from_hex(hex)?;
                continue;
            }

            // Variable slot re-purposes carry four fields.
            if let Some(rest) = line.strip_prefix("variable ") {
                let mut fields = rest.split(' ');
                let (Some(slot), Some(name), Some(offset), Some(size)) =
                    (fields.next(), fields.next(), fields.next(), fields.next())
                else {
                    bail!("malformed variable line: {line}");
                };
                preset.variables.push(VariableRewrite {
                    slot: slot.to_string(),
                    name: name.to_string(),
                    offset: offset.parse()?,
                    size: size.parse()?,
                });
                continue;
            }

            // Variable slot additions carry four fields too.
            if let Some(rest) = line.strip_prefix("clone ") {
                let mut fields = rest.split(' ');
                let (Some(template), Some(name), Some(offset), Some(size)) =
                    (fields.next(), fields.next(), fields.next(), fields.next())
                else {
                    bail!("malformed clone line: {line}");
                };
                preset.clones.push(VariableClone {
                    template: template.to_string(),
                    name: name.to_string(),
                    offset: offset.parse()?,
                    size: size.parse()?,
                });
                continue;
            }

            // Channel renames carry two fields.
            if let Some(rest) = line.strip_prefix("channel ") {
                let mut fields = rest.split(' ');
                let (Some(slot), Some(name)) = (fields.next(), fields.next()) else {
                    bail!("malformed channel line: {line}");
                };
                preset.channels.push(ChannelRename {
                    slot: slot.to_string(),
                    name: name.to_string(),
                });
                continue;
            }

            // Channel record clones carry two fields too.
            if let Some(rest) = line.strip_prefix("clone_channel ") {
                let mut fields = rest.split(' ');
                let (Some(template), Some(name)) = (fields.next(), fields.next()) else {
                    bail!("malformed clone_channel line: {line}");
                };
                preset.channel_clones.push(ChannelClone {
                    template: template.to_string(),
                    name: name.to_string(),
                });
                continue;
            }

            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "version" => preset.version = value.parse()?,
                "opaque" => preset.opaque = value.parse()?,
                "context_count" => preset.context_count = value.parse()?,
                "dependency_count" => preset.dependency_count = value.parse()?,
                "contexts" => preset.contexts = from_hex(value)?,
                "conditions" => preset.conditions = from_hex(value)?,
                "dependencies" => preset.dependencies = from_hex(value)?,
                "group_data" => preset.group_data = from_hex(value)?,
                "device_preamble" => preset.device_preamble = from_hex(value)?,
                other => bail!("unknown preset key '{other}'"),
            }
        }

        Ok(preset)
    }

    /// Serializes the preset to its text form.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("version {}\n", self.version));
        text.push_str(&format!("opaque {}\n", self.opaque));
        text.push_str(&format!("context_count {}\n", self.context_count));
        text.push_str(&format!("dependency_count {}\n", self.dependency_count));
        text.push_str(&format!("contexts {}\n", to_hex(&self.contexts)));
        text.push_str(&format!("conditions {}\n", to_hex(&self.conditions)));
        text.push_str(&format!("dependencies {}\n", to_hex(&self.dependencies)));
        text.push_str(&format!("group_data {}\n", to_hex(&self.group_data)));
        text.push_str(&format!(
            "device_preamble {}\n",
            to_hex(&self.device_preamble)
        ));

        // Deduplicate the tails: programs that share one reference the same
        // `tail` line, which shrinks family presets a lot (the UI family has 96
        // programs but only about 20 distinct tails).
        let mut distinct: Vec<&Vec<u8>> = Vec::new();
        let mut indexes = Vec::with_capacity(self.programs.len());
        for (_, tail) in &self.programs {
            match distinct.iter().position(|other| **other == *tail) {
                Some(index) => indexes.push(index),
                None => {
                    distinct.push(tail);
                    indexes.push(distinct.len() - 1);
                }
            }
        }
        for (index, tail) in distinct.iter().enumerate() {
            text.push_str(&format!("tail {index} {}\n", to_hex(tail)));
        }
        for ((stage, _), index) in self.programs.iter().zip(&indexes) {
            text.push_str(&format!("program {stage:?} #{index}\n"));
        }

        for variable in &self.variables {
            text.push_str(&format!(
                "variable {} {} {} {}\n",
                variable.slot, variable.name, variable.offset, variable.size
            ));
        }

        for clone in &self.clones {
            text.push_str(&format!(
                "clone {} {} {} {}\n",
                clone.template, clone.name, clone.offset, clone.size
            ));
        }

        for channel in &self.channels {
            text.push_str(&format!("channel {} {}\n", channel.slot, channel.name));
        }

        for clone in &self.channel_clones {
            text.push_str(&format!(
                "clone_channel {} {}\n",
                clone.template, clone.name
            ));
        }

        text
    }

    /// Builds the device data: the preset's preamble, then one framed program
    /// record per preset program, using `containers[stage]` and that program's
    /// tail.
    pub fn build_device(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        self.build_device_with(&self.device_preamble, &self.programs, containers)
    }

    /// Like [`Preset::build_device`], but with the preamble and program tails to
    /// use (the generator may have patched them).
    fn build_device_with(
        &self,
        preamble: &[u8],
        programs: &[(Stage, Vec<u8>)],
        containers: &HashMap<Stage, Vec<u8>>,
    ) -> Result<Vec<u8>> {
        let mut device = preamble.to_vec();

        for (index, (stage, tail)) in programs.iter().enumerate() {
            let container = containers.get(stage).ok_or_else(|| {
                color_eyre::eyre::eyre!("no container given for program {index} ({stage:?})")
            })?;

            let frame = shader::encode_frame(container)?;
            let key = murmur::hash(&frame, 0);

            device.extend_from_slice(&1u32.to_le_bytes());
            device.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            device.extend_from_slice(&frame);
            device.extend_from_slice(&5u32.to_le_bytes());
            device.extend_from_slice(&(container.len() as u32).to_le_bytes());
            device.extend_from_slice(&key.to_le_bytes());
            device.extend_from_slice(tail);
        }

        Ok(device)
    }

    /// Assembles a complete shader section from the preset and our programs.
    pub fn generate(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        Ok(self.generate_with_report(containers)?.0)
    }

    /// Like [`Preset::generate`], but also reports how many variable records the
    /// preset's `variable` and `clone` lines rewrote and added (zero when it
    /// declares none).
    pub fn generate_with_report(
        &self,
        containers: &HashMap<Stage, Vec<u8>>,
    ) -> Result<(Vec<u8>, usize, usize)> {
        let mut group_data = self.group_data.clone();
        let mut preamble = self.device_preamble.clone();
        let mut programs = self.programs.clone();
        let mut rewritten = 0usize;
        let mut cloned = 0usize;
        for variable in &self.variables {
            if !matches!(variable.size, 4 | 8 | 12 | 16 | 64) {
                bail!(
                    "variable '{}' has size {}, expected one of 4, 8, 12, 16, 64",
                    variable.name,
                    variable.size
                );
            }
            let (count, old) = patch_variable(
                &mut group_data,
                hash_token(&variable.slot),
                hash_token(&variable.name),
                variable.offset,
                variable.size,
            );
            rewritten += count;

            // Grow the constant buffer that covered the slot so the shader can
            // actually read the new space: find the tail entry whose size covers
            // the old range and is smaller than the new one.
            if let Some((old_offset, old_size)) = old {
                grow_tails(
                    &mut programs,
                    old_offset + old_size,
                    variable.offset + variable.size,
                );
            }
        }

        for clone in &self.clones {
            if !matches!(clone.size, 4 | 8 | 12 | 16 | 64) {
                bail!(
                    "clone '{}' has size {}, expected one of 4, 8, 12, 16, 64",
                    clone.name,
                    clone.size
                );
            }
            let (count, old) = clone_variable(
                &mut group_data,
                hash_token(&clone.template),
                hash_token(&clone.name),
                clone.offset,
                clone.size,
            );
            cloned += count;

            // A cloned slot needs the same cbuffer growth as a rewritten one.
            if let Some((old_offset, old_size)) = old {
                grow_tails(
                    &mut programs,
                    old_offset + old_size,
                    clone.offset + clone.size,
                );
            }
        }

        // Channel renames change only names: the hash is replaced in the group
        // data (canonical and packed copies), in the preamble and in every
        // program tail's block, so the library, the material and the shader all
        // agree on the new name.
        for channel in &self.channels {
            let slot = hash_token(&channel.slot);
            let name = hash_token(&channel.name);
            rewritten += replace_hash(&mut group_data, slot, name);
            rewritten += replace_hash(&mut preamble, slot, name);
            for (_, tail) in &mut programs {
                rewritten += replace_hash(tail, slot, name);
            }
        }

        // Channel record clones add a record to the preamble's record stream and
        // to every program tail's block, right where the template record sits.
        // The record's length follows its kind, so the exact template bytes can
        // be searched for without parsing the surrounding block. The group data
        // describes the channel as a variable record (its type and slot), so
        // that record is cloned too, with the template's offset and size.
        for clone in &self.channel_clones {
            let template = hash_token(&clone.template);
            let name = hash_token(&clone.name);
            if template == name {
                continue;
            }
            // The group data describes the channel in canonical and packed
            // framings; both are cloned with their run counts.
            cloned += clone_channel_group_data(&mut group_data, template, name);
            // The stream's record count lives in the word right before it; it has
            // to grow with the inserted record or the engine reads the stream
            // wrong (an unbumped count made the game run out of memory).
            let stream = find_channel_stream(&preamble);
            let Some((offset, _kind, len)) = find_channel_record(&preamble, template) else {
                continue;
            };
            let record = preamble[offset..offset + len].to_vec();
            let mut replacement = record.clone();
            replacement[0..4].copy_from_slice(&name.to_le_bytes());
            let inserted = clone_record(&mut preamble, &record, &replacement);
            if inserted > 0
                && let Some((start, count)) = stream
                && start >= 4
            {
                let at = start - 4;
                let grown = (count + inserted) as u32;
                preamble[at..at + 4].copy_from_slice(&grown.to_le_bytes());
            }
            cloned += inserted;
        }

        let device = self.build_device_with(&preamble, &programs, containers)?;

        let contexts_offset = 48usize;
        let conditions_offset = contexts_offset + self.contexts.len();
        let dependencies_offset = conditions_offset + self.conditions.len();
        let group_offset = dependencies_offset + self.dependencies.len();
        let device_offset = group_offset + group_data.len();
        let default_offset = device_offset + device.len();

        let header = [
            self.version,
            self.opaque,
            contexts_offset as u32,
            self.context_count,
            conditions_offset as u32,
            default_offset as u32,
            dependencies_offset as u32,
            self.dependency_count,
            group_offset as u32,
            group_data.len() as u32,
            device_offset as u32,
            device.len() as u32,
        ];

        let mut section = Vec::with_capacity(default_offset + 32);
        for word in header {
            section.extend_from_slice(&word.to_le_bytes());
        }
        section.extend_from_slice(&self.contexts);
        section.extend_from_slice(&self.conditions);
        section.extend_from_slice(&self.dependencies);
        section.extend_from_slice(&group_data);
        section.extend_from_slice(&device);
        section.extend_from_slice(&[0u8; 16]);
        while section.len() % 16 != 0 {
            section.push(0);
        }

        Ok((section, rewritten, cloned))
    }

    /// Extracts a preset straight from a material data file path.
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let data = fs::read(path)?;
        Self::from_material(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_preset() -> Preset {
        Preset {
            version: 1,
            opaque: 0,
            context_count: 0,
            dependency_count: 0,
            contexts: Vec::new(),
            conditions: Vec::new(),
            dependencies: Vec::new(),
            group_data: Vec::new(),
            device_preamble: Vec::new(),
            programs: Vec::new(),
            tails: Vec::new(),
            variables: Vec::new(),
            clones: Vec::new(),
            channels: Vec::new(),
            channel_clones: Vec::new(),
        }
    }

    #[test]
    fn variable_rewrite_patches_records() {
        let mut preset = empty_preset();

        // One canonical record: {type 3, flags 0, hash, offset 224, size 16}.
        let slot = hash_token("dev_wireframe_color");
        let mut group = Vec::new();
        for word in [3u32, 0, slot, 224, 16] {
            group.extend_from_slice(&word.to_le_bytes());
        }
        preset.group_data = group;
        preset.variables.push(VariableRewrite {
            slot: "dev_wireframe_color".to_string(),
            name: "mod_probe".to_string(),
            offset: 240,
            size: 16,
        });

        let (section, rewritten, _) = preset.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!(rewritten, 1);

        // The group data starts right after the 48 byte header when the other
        // sections are empty.
        let group = &section[48..68];
        assert_eq!(u32_at(group, 0), 3);
        assert_eq!(u32_at(group, 8), hash_token("mod_probe"));
        assert_eq!(u32_at(group, 12), 240);
        assert_eq!(u32_at(group, 16), 16);
    }

    #[test]
    fn variable_rewrite_leaves_other_records_alone() {
        let mut preset = empty_preset();

        let other = hash_token("texture_map");
        let mut group = Vec::new();
        for word in [5u32, 0, other, 0, 4] {
            group.extend_from_slice(&word.to_le_bytes());
        }
        preset.group_data = group;
        preset.variables.push(VariableRewrite {
            slot: "dev_wireframe_color".to_string(),
            name: "mod_probe".to_string(),
            offset: 240,
            size: 16,
        });

        let (section, rewritten, _) = preset.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!(rewritten, 0);
        assert_eq!(&section[48..68], &preset.group_data[..]);
    }

    #[test]
    fn clone_appends_a_record_to_each_run() {
        let mut preset = empty_preset();

        // Two runs: the first is a `{kind, count}` header plus one record, the
        // second is a bare record with its count as the preceding word.
        let slot = hash_token("dev_wireframe_color");
        let mut group = Vec::new();
        for word in [3u32, 1] {
            group.extend_from_slice(&word.to_le_bytes());
        }
        for word in [3u32, 0, slot, 224, 16] {
            group.extend_from_slice(&word.to_le_bytes());
        }
        group.extend_from_slice(&1u32.to_le_bytes());
        for word in [3u32, 0, slot, 224, 16] {
            group.extend_from_slice(&word.to_le_bytes());
        }
        preset.group_data = group;
        preset.clones.push(VariableClone {
            template: "dev_wireframe_color".to_string(),
            name: "mod_extra".to_string(),
            offset: 240,
            size: 16,
        });

        let (section, rewritten, cloned) = preset.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!((rewritten, cloned), (0, 2));

        let group = &section[48..];
        // First run: its header count grew and the clone sits after record 1.
        assert_eq!(u32_at(group, 4), 2);
        assert_eq!(u32_at(group, 36), hash_token("mod_extra"));
        assert_eq!(u32_at(group, 40), 240);
        assert_eq!(u32_at(group, 44), 16);
        // The second run's count word also grew; the first insertion shifted it
        // to just before record 2 (which starts at 52).
        assert_eq!(u32_at(group, 48), 2);
        assert_eq!(u32_at(group, 52 + 8), slot);
        assert_eq!(u32_at(group, 72 + 8), hash_token("mod_extra"));
    }
    #[test]
    fn grow_tails_expands_the_covering_entry() {
        let tail = shader::Tail {
            cbuffers: vec![shader::TailCbuffer {
                words: [0xB5639618, 0, 240, 0, 0, 0],
            }],
            rest: Vec::new(),
        }
        .bytes();
        let mut programs = vec![(Stage::Pixel, tail)];
        grow_tails(&mut programs, 240, 256);
        let parsed = shader::Tail::parse(&programs[0].1).unwrap();
        assert_eq!(parsed.cbuffers[0].size(), 256);

        // An entry that does not cover the old range stays untouched.
        let tail = shader::Tail {
            cbuffers: vec![shader::TailCbuffer {
                words: [0x1, 0, 64, 0, 0, 0],
            }],
            rest: Vec::new(),
        }
        .bytes();
        let mut programs = vec![(Stage::Pixel, tail)];
        grow_tails(&mut programs, 240, 256);
        let parsed = shader::Tail::parse(&programs[0].1).unwrap();
        assert_eq!(parsed.cbuffers[0].size(), 64);
    }

    #[test]
    fn clone_reports_the_template_range() {
        let mut data = Vec::new();
        let slot = hash_token("dev_wireframe_color");
        for word in [3u32, 0, slot, 224, 16] {
            data.extend_from_slice(&word.to_le_bytes());
        }
        let (count, old) = clone_variable(&mut data, slot, hash_token("mod_extra"), 240, 16);
        assert_eq!(count, 1);
        assert_eq!(old, Some((224, 16)));
        assert_eq!(u32_at(&data, 20 + 8), hash_token("mod_extra"));
        assert_eq!(u32_at(&data, 20 + 12), 240);
    }

    #[test]
    fn channel_rename_replaces_the_name_everywhere() {
        let mut preset = empty_preset();
        let slot = hash_token("texture_map");
        let name = hash_token("mod_map");

        // The name appears in the group data and the preamble.
        let mut group = Vec::new();
        for word in [1u32, 0, slot, 72, 8] {
            group.extend_from_slice(&word.to_le_bytes());
        }
        preset.group_data = group;
        let mut preamble = Vec::new();
        for word in [7u32, slot, 9] {
            preamble.extend_from_slice(&word.to_le_bytes());
        }
        preset.device_preamble = preamble;
        preset.channels.push(ChannelRename {
            slot: "texture_map".to_string(),
            name: "mod_map".to_string(),
        });

        let (section, rewritten, _) = preset.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!(rewritten, 2, "group and preamble");

        // Group data starts at 48 with the other sections empty.
        let group = &section[48..];
        assert_eq!(u32_at(group, 8), name);
        assert!(!group.windows(4).any(|window| window == slot.to_le_bytes()));

        // A tail is patched the same way (the generate path needs a DXBC
        // container per program, so this checks the replacement itself).
        let mut tail = slot.to_le_bytes().to_vec();
        assert_eq!(replace_hash(&mut tail, slot, name), 1);
        assert_eq!(tail, name.to_le_bytes());
    }

    #[test]
    fn tails_round_trip_through_dedup() {
        let mut preset = empty_preset();
        let tail_a = vec![1u8, 2, 3, 4];
        let tail_b = vec![5u8, 6, 7, 8];
        preset.programs = vec![
            (Stage::Vertex, tail_a.clone()),
            (Stage::Pixel, tail_b.clone()),
            (Stage::Vertex, tail_a.clone()),
        ];

        let text = preset.to_text();
        assert!(text.contains("tail 0 01020304"));
        assert!(text.contains("tail 1 05060708"));
        assert!(text.contains("program Vertex #0"));
        assert!(text.contains("program Pixel #1"));
        assert_eq!(text.matches("program Vertex #0").count(), 2);

        let parsed = Preset::from_text(&text).unwrap();
        assert_eq!(parsed.programs.len(), 3);
        assert_eq!(parsed.programs[0].1, tail_a);
        assert_eq!(parsed.programs[1].1, tail_b);
        assert_eq!(parsed.programs[2].1, tail_a);
    }

    /// Builds a minimal block channel record: `{hash, kind, count = 1}` plus
    /// filler bytes of the kind's length.
    fn channel_record(hash: u32, kind: u32) -> Vec<u8> {
        let len = channel_record_len(kind).unwrap();
        let mut record = vec![0u8; len];
        record[0..4].copy_from_slice(&hash.to_le_bytes());
        record[4..8].copy_from_slice(&kind.to_le_bytes());
        record[8..12].copy_from_slice(&1u32.to_le_bytes());
        record
    }

    #[test]
    fn channel_records_are_found_by_kind_length() {
        let engine = hash_token("fog_volume");
        let mut preamble = vec![0u8; 32];
        preamble.extend_from_slice(&channel_record(engine, 4));
        preamble.extend_from_slice(&channel_record(0x1111_1111, 5));
        preamble.extend_from_slice(&channel_record(0x2222_2222, 4));

        assert_eq!(find_channel_record(&preamble, engine), Some((32, 4, 60)));
        assert_eq!(
            find_channel_record(&preamble, 0x1111_1111),
            Some((92, 5, 73))
        );
        assert_eq!(
            find_channel_record(&preamble, 0x2222_2222),
            Some((165, 4, 60))
        );
        assert_eq!(find_channel_record(&preamble, 0xDEAD_BEEF), None);
    }

    #[test]
    fn channel_clone_inserts_a_record() {
        let template = hash_token("texture_map");
        let name = hash_token("mod_map");
        let record = channel_record(template, 4);
        let mut replacement = record.clone();
        replacement[0..4].copy_from_slice(&name.to_le_bytes());

        let mut preamble = vec![0u8; 24];
        preamble.extend_from_slice(&record);
        preamble.extend_from_slice(&channel_record(0x3333_3333, 5));
        assert_eq!(clone_record(&mut preamble, &record, &replacement), 1);
        assert_eq!(u32_at(&preamble, 24), template);
        assert_eq!(u32_at(&preamble, 84), name);
        assert_eq!(u32_at(&preamble, 88), 4);
        assert_eq!(u32_at(&preamble, 60 + 84 + 4), 5, "record intact");

        // A tail whose block repeats the record is patched the same way.
        let mut tail = record.clone();
        tail.extend_from_slice(&channel_record(0x4444_4444, 4));
        assert_eq!(clone_record(&mut tail, &record, &replacement), 1);
        assert_eq!(u32_at(&tail, 60), name);
    }

    #[test]
    fn channel_clone_line_round_trips_and_generates() {
        let mut preset = empty_preset();
        let template = hash_token("texture_map");
        let mut preamble = vec![0u8; 16];
        preamble.extend_from_slice(&channel_record(template, 4));
        preset.device_preamble = preamble;
        preset.channel_clones.push(ChannelClone {
            template: "texture_map".to_string(),
            name: "mod_map".to_string(),
        });

        let text = preset.to_text();
        assert!(text.contains("clone_channel texture_map mod_map"), "{text}");
        let parsed = Preset::from_text(&text).unwrap();
        assert_eq!(parsed.channel_clones.len(), 1);

        let (section, rewritten, cloned) = parsed.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!((rewritten, cloned), (0, 1));
        // The device starts at 48 with the other sections empty.
        let device = &section[48..];
        assert_eq!(u32_at(device, 16), template);
        assert_eq!(u32_at(device, 16 + 60), hash_token("mod_map"));
        assert_eq!(u32_at(device, 16 + 60 + 4), 4);
    }

    #[test]
    fn channel_clone_bumps_the_stream_count() {
        let mut preset = empty_preset();
        let template = hash_token("texture_map");
        let name = hash_token("mod_map");

        // The record stream is preceded by its record count (the shipped
        // families have it right before the first record).
        let mut preamble = Vec::new();
        preamble.extend_from_slice(&1u32.to_le_bytes());
        preamble.extend_from_slice(&channel_record(template, 4));
        preset.device_preamble = preamble;
        preset.channel_clones.push(ChannelClone {
            template: "texture_map".to_string(),
            name: "mod_map".to_string(),
        });

        let (section, _, cloned) = preset.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!(cloned, 1);
        let device = &section[48..];
        assert_eq!(u32_at(device, 0), 2, "stream count grew by one");
        assert_eq!(u32_at(device, 4), template);
        assert_eq!(u32_at(device, 64), name);
    }

    #[test]
    fn channel_clone_copies_both_group_data_framings() {
        let mut preset = empty_preset();
        let template = hash_token("texture_map");
        let name = hash_token("mod_map");
        let cbuffer = 0xB5639618;

        // A canonical run of two records with its count, then a packed run of
        // two records (cbuffer hash and zero at +20/+24) with its count.
        let mut group = Vec::new();
        group.extend_from_slice(&2u32.to_le_bytes());
        for (kind, offset, size) in [(5u32, 0u32, 4u32), (1, 4, 8)] {
            for word in [kind, 0, template, offset, size] {
                group.extend_from_slice(&word.to_le_bytes());
            }
        }
        group.extend_from_slice(&2u32.to_le_bytes());
        for a in [0u32, 8] {
            for word in [template, a, 0, 4, 1, cbuffer, 0] {
                group.extend_from_slice(&word.to_le_bytes());
            }
        }
        preset.group_data = group;
        preset.channel_clones.push(ChannelClone {
            template: "texture_map".to_string(),
            name: "mod_map".to_string(),
        });

        let (section, _, cloned) = preset.generate_with_report(&HashMap::new()).unwrap();
        assert_eq!(cloned, 4, "two canonical and two packed records");
        let group = &section[48..];
        // Canonical run: count 2 -> 4, one clone after each record.
        assert_eq!(u32_at(group, 0), 4, "canonical count");
        assert_eq!(u32_at(group, 32), name, "first canonical clone");
        // The packed run follows the grown canonical run: 4 + 4 * 20.
        assert_eq!(u32_at(group, 84), 4, "packed count");
        assert_eq!(u32_at(group, 116), name, "first packed clone");
        assert_eq!(
            u32_at(group, 136),
            cbuffer,
            "packed clone keeps the cbuffer"
        );
    }
}
