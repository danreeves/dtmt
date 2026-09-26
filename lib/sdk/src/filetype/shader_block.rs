//! The carried engine block: the compiled preamble a generated section starts
//! from.
//!
//! This is not part of a `.shader_node` declaration. It is the compiled
//! interface of the engine library the section is built against, and it cannot
//! be synthesised - a generated minimal block fails at shader load - so a mod
//! carries a template and rewrites the three varying header words. See
//! `docs/Shader Section Generation Notes.md`.

use super::shader_decl::{ChannelDef, ValueType};

/// The record length the engine uses for a channel record, by its kind.
fn record_len(kind: u32) -> Option<usize> {
    match kind {
        4 => Some(60),
        5 => Some(73),
        _ => None,
    }
}

/// The engine-side constant a generated block starts from: a shipped preamble
/// carrying the header, the engine-variable records and one channel record per
/// kind to clone from. It is the only piece of the block a mod cannot derive.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockTemplate {
    preamble: Vec<u8>,
    /// The engine-variable records, as `[index, value]` pairs.
    records: Vec<(u32, u32)>,
    /// The channel record stream, count word first.
    stream: Vec<u8>,
}

impl BlockTemplate {
    /// Reads a shipped preamble, checking the header/table/stream framing: a
    /// 120-byte header, `count - 8` byte-packed 13-byte records, the stream's
    /// count word, then the records themselves.
    pub fn from_preamble(preamble: &[u8]) -> Result<Self, color_eyre::Report> {
        use color_eyre::eyre::bail;
        if preamble.len() < 0x78 {
            bail!("the block template is too small ({} bytes)", preamble.len());
        }
        let count = u32_at(preamble, 12);
        if count < 8 {
            bail!("the block template's record count {count} is too small");
        }
        let records = (count - 8) as usize;
        let table_end = 0x78 + records * 13;
        if table_end + 4 > preamble.len() {
            bail!(
                "the block template's record table runs past the preamble ({} > {})",
                table_end + 4,
                preamble.len()
            );
        }
        let stream_count = u32_at(preamble, table_end) as usize;
        let mut records_data = Vec::new();
        let mut at = 0x78;
        for _ in 0..records {
            records_data.push((u32_at(preamble, at), u32_at(preamble, at + 5)));
            at += 13;
        }
        // Trim the stream to its records: walk the count and the per-kind lengths
        // so a template with a trailing pad still parses.
        let mut kept = 4;
        at = table_end + 4;
        for _ in 0..stream_count {
            let kind = u32_at(preamble, at + 4);
            let Some(len) = record_len(kind) else {
                bail!("unknown channel record kind {kind}");
            };
            kept += len;
            at += len;
        }
        if at > preamble.len() {
            bail!(
                "the block template's stream runs past the preamble ({} > {})",
                at,
                preamble.len()
            );
        }
        let mut stream = preamble[table_end..].to_vec();
        stream.truncate(kept);
        Ok(Self {
            preamble: preamble.to_vec(),
            records: records_data,
            stream,
        })
    }

    /// The group count the template carries, which a generated section repeats.
    pub fn groups(&self) -> u32 {
        u32_at(&self.preamble, 4)
    }

    /// The cbuffer count the template carries.
    pub fn cbuffers(&self) -> u32 {
        u32_at(&self.preamble, 8)
    }

    /// The engine-variable records, which a generated block copies verbatim.
    pub fn records(&self) -> &[(u32, u32)] {
        &self.records
    }

    /// The channel record stream (count word first).
    pub fn stream(&self) -> &[u8] {
        &self.stream
    }

    /// The channel record of the given kind, for a clone to copy.
    pub fn channel_record(&self, kind: u32) -> Option<Vec<u8>> {
        let count = u32_at(&self.stream, 0) as usize;
        let mut at = 4;
        for _ in 0..count {
            let this = u32_at(&self.stream, at + 4);
            let len = record_len(this)?;
            if this == kind {
                return Some(self.stream[at..at + len].to_vec());
            }
            at += len;
        }
        None
    }

    /// The channel names the template's stream carries.
    pub fn channel_names(&self) -> Vec<u32> {
        let count = u32_at(&self.stream, 0) as usize;
        let mut names = Vec::new();
        let mut at = 4;
        for _ in 0..count {
            names.push(u32_at(&self.stream, at));
            let len = record_len(u32_at(&self.stream, at + 4)).unwrap_or(60);
            at += len;
        }
        names
    }
}

/// Builds a block for `channels`: the template's header with the given counts,
/// its engine-variable records, and one channel record per declared channel -
/// the template's record of the matching kind with the name hash substituted.
/// A channel whose name the template already carries keeps its record.
pub fn build_block(
    template: &BlockTemplate,
    channels: &[(String, ChannelDef)],
    groups: u32,
    cbuffers: u32,
) -> Result<Vec<u8>, color_eyre::Report> {
    use color_eyre::eyre::bail;
    let mut block = Vec::new();
    // The header: the template's bytes with the three varying words rewritten.
    block.extend_from_slice(&template.preamble[..0x78]);
    let records = template.records.len();
    for (index, value) in &template.records {
        block.extend_from_slice(&index.to_le_bytes());
        block.push(0);
        block.extend_from_slice(&value.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut stream = Vec::new();
    let mut written: Vec<u32> = Vec::new();
    for (name, channel) in channels {
        let hash = u32::from(crate::murmur::Murmur32::hash(name.as_bytes()));
        if let Some(at) = template
            .channel_names()
            .iter()
            .position(|existing| *existing == hash)
        {
            // The template already carries it; copy its record unchanged.
            let mut skip = 4;
            for _ in 0..at {
                skip += record_len(u32_at(template.stream(), skip + 4)).unwrap_or(60);
            }
            let len = record_len(u32_at(template.stream(), skip + 4)).unwrap_or(60);
            stream.extend_from_slice(&template.stream()[skip..skip + len]);
            written.push(hash);
            continue;
        }
        // A new channel: clone the template's record of the same kind. Only the
        // two texture kinds have a decoded record length, and the texture record
        // is the shape the verified channel clone used.
        if channel.kind != ValueType::Texture2D {
            bail!(
                "channel {name} is a {:?} and no block record of that kind is known",
                channel.kind
            );
        }
        let Some(record) = template
            .channel_record(4)
            .or_else(|| template.channel_record(5))
        else {
            bail!("the block template has no channel record to clone");
        };
        let mut clone = record;
        clone[0..4].copy_from_slice(&hash.to_le_bytes());
        stream.extend_from_slice(&clone);
        written.push(hash);
    }
    let mut count_bytes = (written.len() as u32).to_le_bytes().to_vec();
    count_bytes.extend_from_slice(&stream);
    block.extend_from_slice(&count_bytes);
    // The three varying header words: groups, cbuffers, records + 8.
    block[4..8].copy_from_slice(&groups.to_le_bytes());
    block[8..12].copy_from_slice(&cbuffers.to_le_bytes());
    block[12..16].copy_from_slice(&((records as u32) + 8).to_le_bytes());
    Ok(block)
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The murmur32 the engine hashes channel names with.
    fn hash(name: &str) -> u32 {
        u32::from(crate::murmur::Murmur32::hash(name.as_bytes()))
    }

    fn texture_channel() -> ChannelDef {
        ChannelDef {
            name: "texture_map".to_string(),
            kind: ValueType::Texture2D,
            required: true,
            ..ChannelDef::default()
        }
    }

    fn template_preamble() -> Vec<u8> {
        let mut block = vec![0u8; 0x78];
        block[4..8].copy_from_slice(&1u32.to_le_bytes());
        block[8..12].copy_from_slice(&2u32.to_le_bytes());
        // One engine record, so the count word is 8 + 1.
        block[12..16].copy_from_slice(&9u32.to_le_bytes());
        // The record table: {index, pad, value, pad}.
        block.extend_from_slice(&7u32.to_le_bytes());
        block.push(0);
        block.extend_from_slice(&4242u32.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
        // The stream: its count word, then the channel records.
        block.extend_from_slice(&1u32.to_le_bytes());
        let mut record = vec![0u8; 60];
        record[0..4].copy_from_slice(&hash("texture_map").to_le_bytes());
        record[4..8].copy_from_slice(&4u32.to_le_bytes());
        record[8..12].copy_from_slice(&1u32.to_le_bytes());
        record[12..16].copy_from_slice(&0x1234u32.to_le_bytes());
        block.extend_from_slice(&record);
        block
    }

    #[test]
    fn reads_a_template_preamble() {
        let preamble = template_preamble();
        let template = BlockTemplate::from_preamble(&preamble).expect("template");
        assert_eq!(template.groups(), 1);
        assert_eq!(template.cbuffers(), 2);
        assert_eq!(template.records(), &[(7, 4242)]);
        assert_eq!(template.channel_names(), vec![hash("texture_map")]);
        let record = template.channel_record(4).expect("kind 4");
        assert_eq!(record.len(), 60);
        assert_eq!(u32_at(&record, 8), 1);
        // The stream is the count word plus the record.
        assert_eq!(template.stream().len(), 4 + 60);
        assert!(template.channel_record(5).is_none());
    }

    #[test]
    fn rejects_a_malformed_template() {
        assert!(BlockTemplate::from_preamble(&[0u8; 16]).is_err());
        let mut preamble = template_preamble();
        preamble[12..16].copy_from_slice(&4u32.to_le_bytes());
        let err = BlockTemplate::from_preamble(&preamble).expect_err("count too small");
        assert!(err.to_string().contains("too small"), "{err}");
        // A record count that outruns the preamble is caught, not a panic.
        preamble.copy_from_slice(&template_preamble());
        preamble[12..16].copy_from_slice(&900u32.to_le_bytes());
        assert!(BlockTemplate::from_preamble(&preamble).is_err());
    }

    #[test]
    fn builds_a_block_from_a_declaration() {
        let template = BlockTemplate::from_preamble(&template_preamble()).expect("template");
        let channels = [
            ("texture_map".to_string(), texture_channel()),
            ("mod_map".to_string(), texture_channel()),
        ];
        let block = build_block(&template, &channels, 4, 3).expect("build");
        // The rebuilt block reads back as a template of its own: one group, three
        // cbuffers, the engine record copied verbatim.
        let built = BlockTemplate::from_preamble(&block).expect("re-read");
        assert_eq!(built.groups(), 4);
        assert_eq!(built.cbuffers(), 3);
        assert_eq!(built.records(), &[(7, 4242)]);
        // Both channels are present, in declaration order, hashed by name.
        assert_eq!(
            built.channel_names(),
            vec![hash("texture_map"), hash("mod_map")]
        );
        // A channel the template already carried keeps its record byte for byte.
        let first = built.channel_record(4).expect("record");
        assert_eq!(u32_at(&first, 0), hash("texture_map"));
        assert_eq!(u32_at(&first, 12), 0x1234);
    }

    #[test]
    fn a_cloned_channel_differs_only_in_its_name() {
        let template = BlockTemplate::from_preamble(&template_preamble()).expect("template");
        let channels = [("mod_map".to_string(), texture_channel())];
        let block = build_block(&template, &channels, 1, 2).expect("build");
        let built = BlockTemplate::from_preamble(&block).expect("re-read");
        assert_eq!(built.channel_names(), vec![hash("mod_map")]);
        let record = built.channel_record(4).expect("record");
        assert_eq!(record.len(), 60);
        // The body is the template's, which is what the verified clone did.
        assert_eq!(u32_at(&record, 4), 4);
        assert_eq!(u32_at(&record, 8), 1);
        assert_eq!(u32_at(&record, 12), 0x1234);
    }

    #[test]
    fn a_block_with_no_channels_keeps_an_empty_stream() {
        let template = BlockTemplate::from_preamble(&template_preamble()).expect("template");
        let block = build_block(&template, &[], 1, 2).expect("build");
        assert_eq!(block.len(), 0x78 + 13 + 4);
        let built = BlockTemplate::from_preamble(&block).expect("re-read");
        assert!(built.channel_names().is_empty());
    }
}
