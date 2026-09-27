//! EngineDatas: generating a `shader43` section from our own programs.
//!
//! A shader (the `gui` UI shader, the entity shader, ...) shares an
//! engine-side wrapper: the contexts the engine queries, the conditions tree
//! that selects a group, the group data with its variable tables, the packed
//! device preamble and one metadata tail per program. A [`EngineData`] captures
//! that wrapper once from a shipped base material, so a mod can generate a
//! complete section from its own compiled programs instead of shipping a
//! shipped shader blob.
//!
//! ```text
//! let engine_data = EngineData::from_material(&data)?;          // extract once per shader
//! std::fs::write("ui.engine_data", engine_data.to_text())?;
//!
//! let engine_data = EngineData::from_text(&text)?;              // generate from it
//! let section = engine_data.generate(&containers)?;         // Stage -> DXBC container
//! ```

use std::collections::HashMap;
use std::fs;

use color_eyre::eyre::{Context, Result, bail};

use super::shader::{self, Stage};
use crate::murmur;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
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

/// The engine-side data of a shipped section: the parts the generator cannot
/// currently derive, captured once from a shipped material.
#[derive(Debug)]
pub struct EngineData {
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
}

impl EngineData {
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
        })
    }

    /// Parses the engine data from its text form.
    pub fn from_text(text: &str) -> Result<Self> {
        let mut engine_data = Self {
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
                    Some(index) => engine_data
                        .tails
                        .get(index.parse::<usize>()?)
                        .cloned()
                        .ok_or_else(|| color_eyre::eyre::eyre!("unknown tail #{index}"))?,
                    None => from_hex(tail)?,
                };
                engine_data.programs.push((stage, tail));
                continue;
            }

            // Deduplicated tails, referenced by `program <stage> #n`.
            if let Some(rest) = line.strip_prefix("tail ") {
                let (index, hex) = rest
                    .split_once(' ')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed tail line"))?;
                let index = index.parse::<usize>()?;
                if engine_data.tails.len() <= index {
                    engine_data.tails.resize(index + 1, Vec::new());
                }
                engine_data.tails[index] = from_hex(hex)?;
                continue;
            }

            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "opaque" => engine_data.opaque = value.parse()?,
                "context_count" => engine_data.context_count = value.parse()?,
                "dependency_count" => engine_data.dependency_count = value.parse()?,
                "contexts" => engine_data.contexts = from_hex(value)?,
                "conditions" => engine_data.conditions = from_hex(value)?,
                "dependencies" => engine_data.dependencies = from_hex(value)?,
                "group_data" => engine_data.group_data = from_hex(value)?,
                "device_preamble" => engine_data.device_preamble = from_hex(value)?,
                other => bail!("unknown engine data key '{other}'"),
            }
        }

        Ok(engine_data)
    }

    /// Serializes the engine data to its text form.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
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
        // `tail` line, which shrinks engine data files a lot (the UI shader has 96
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

        text
    }

    /// Builds the device data: the engine data's preamble, then one framed program
    /// record per engine data program, using `containers[stage]` and that program's
    /// tail.
    pub fn build_device(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        let mut device = self.device_preamble.clone();

        for (index, (stage, tail)) in self.programs.iter().enumerate() {
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

    /// Assembles a complete shader section from the engine data and our programs.
    pub fn generate(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        let device = self.build_device(containers)?;

        let contexts_offset = 48usize;
        let conditions_offset = contexts_offset + self.contexts.len();
        let dependencies_offset = conditions_offset + self.conditions.len();
        let group_offset = dependencies_offset + self.dependencies.len();
        let device_offset = group_offset + self.group_data.len();
        let default_offset = device_offset + device.len();

        let header = [
            shader::VERSION,
            self.opaque,
            contexts_offset as u32,
            self.context_count,
            conditions_offset as u32,
            default_offset as u32,
            dependencies_offset as u32,
            self.dependency_count,
            group_offset as u32,
            self.group_data.len() as u32,
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
        section.extend_from_slice(&self.group_data);
        section.extend_from_slice(&device);
        section.extend_from_slice(&[0u8; 16]);
        while section.len() % 16 != 0 {
            section.push(0);
        }

        Ok(section)
    }

    /// Extracts the engine data straight from a material data file path.
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let data = fs::read(path)?;
        Self::from_material(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_engine_data() -> EngineData {
        EngineData {

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
        }
    }

    #[test]
    fn tails_round_trip_through_dedup() {
        let mut engine_data = empty_engine_data();
        let tail_a = vec![1u8, 2, 3, 4];
        let tail_b = vec![5u8, 6, 7, 8];
        engine_data.programs = vec![
            (Stage::Vertex, tail_a.clone()),
            (Stage::Pixel, tail_b.clone()),
            (Stage::Vertex, tail_a.clone()),
        ];

        let text = engine_data.to_text();
        assert!(text.contains("tail 0 01020304"));
        assert!(text.contains("tail 1 05060708"));
        assert!(text.contains("program Vertex #0"));
        assert!(text.contains("program Pixel #1"));
        assert_eq!(text.matches("program Vertex #0").count(), 2);

        let parsed = EngineData::from_text(&text).unwrap();
        assert_eq!(parsed.programs.len(), 3);
        assert_eq!(parsed.programs[0].1, tail_a);
        assert_eq!(parsed.programs[1].1, tail_b);
        assert_eq!(parsed.programs[2].1, tail_a);
    }

    #[test]
    fn a_rewrite_line_is_refused() {
        // The engine data used to carry DTMT-only `variable`/`clone` lines; they are
        // gone, and a file that still has one must fail rather than silently
        // ignore it.
        let err = EngineData::from_text("variable dev_wireframe_color mod_tint 224 16\n")
            .expect_err("a rewrite line is not an engine data key");
        assert!(err.to_string().contains("unknown engine data key"), "{err}");
    }
}