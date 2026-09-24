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
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Program lines carry two fields (stage plus tail hex).
            if let Some(rest) = line.strip_prefix("program ") {
                let (stage, tail) = rest
                    .split_once(' ')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed program line"))?;
                let stage = match stage {
                    "Vertex" => Stage::Vertex,
                    "Pixel" => Stage::Pixel,
                    other => bail!("unsupported program stage '{other}'"),
                };
                preset.programs.push((stage, from_hex(tail)?));
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

        for (stage, tail) in &self.programs {
            text.push_str(&format!("program {stage:?} {}\n", to_hex(tail)));
        }

        text
    }

    /// Builds the device data: the preset's preamble, then one framed program
    /// record per preset program, using `containers[stage]` and that program's
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

    /// Assembles a complete shader section from the preset and our programs.
    pub fn generate(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        let device = self.build_device(containers)?;

        let contexts_offset = 48usize;
        let conditions_offset = contexts_offset + self.contexts.len();
        let dependencies_offset = conditions_offset + self.conditions.len();
        let group_offset = dependencies_offset + self.dependencies.len();
        let device_offset = group_offset + self.group_data.len();
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

    /// Extracts a preset straight from a material data file path.
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let data = fs::read(path)?;
        Self::from_material(&data)
    }
}
