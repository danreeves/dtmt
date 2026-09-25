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
fn patch_variable(data: &mut [u8], slot: u32, name: u32, offset: u32, size: u32) -> (usize, Option<(u32, u32)>) {
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

/// Appends a copy of the `template` record to every run of consecutive records
/// that contains it, renamed to `name` with the given `offset` and `size`. The
/// run's count word is bumped when one can be found (the word just before the
/// run or, in copies that use a `{kind, count}` header, the word before that).
/// Returns the number of records added.
fn clone_variable(data: &mut Vec<u8>, template: u32, name: u32, offset: u32, size: u32) -> usize {
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
                let (Some(slot), Some(name), Some(offset), Some(size)) = (
                    fields.next(),
                    fields.next(),
                    fields.next(),
                    fields.next(),
                ) else {
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
                let (Some(template), Some(name), Some(offset), Some(size)) = (
                    fields.next(),
                    fields.next(),
                    fields.next(),
                    fields.next(),
                ) else {
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

        text
    }

    /// Builds the device data: the preset's preamble, then one framed program
    /// record per preset program, using `containers[stage]` and that program's
    /// tail.
    pub fn build_device(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        self.build_device_with(&self.programs, containers)
    }

    /// Like [`Preset::build_device`], but with the program tails to use (the
    /// generator may have rewritten them).
    fn build_device_with(
        &self,
        programs: &[(Stage, Vec<u8>)],
        containers: &HashMap<Stage, Vec<u8>>,
    ) -> Result<Vec<u8>> {
        let mut device = self.device_preamble.clone();

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
            let Some((old_offset, old_size)) = old else {
                continue;
            };
            let needed = variable.offset + variable.size;
            let old_end = old_offset + old_size;
            for (_, tail) in &mut programs {
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

        for clone in &self.clones {
            if !matches!(clone.size, 4 | 8 | 12 | 16 | 64) {
                bail!(
                    "clone '{}' has size {}, expected one of 4, 8, 12, 16, 64",
                    clone.name,
                    clone.size
                );
            }
            cloned += clone_variable(
                &mut group_data,
                hash_token(&clone.template),
                hash_token(&clone.name),
                clone.offset,
                clone.size,
            );
        }

        let device = self.build_device_with(&programs, containers)?;

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
}