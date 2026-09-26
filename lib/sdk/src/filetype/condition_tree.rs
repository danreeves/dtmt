//! The conditions region of a `shader43` section: a decision tree over a
//! material's channels.
//!
//! The framing is decoded and self-delimiting; the payload's opcodes are only
//! partly mapped, so the reader keeps each payload whole:
//!
//! ```text
//! u16 tag            // 1
//! u16 payload_words  // the number of u16 payload words that follow the hashes
//! u16 payload_offset // 8 + 4 x count, the payload's byte offset in the record
//! u16 count          // the number of condition hashes
//! u32 hashes[count]  // condition names, murmur32
//! u16 payload[payload_words]
//! ```
//!
//! Measured on the UI base's 1436 bytes, 35 records, whose starts are exactly
//! the 29 + 6 conditions offsets of its two contexts: sizes 24, 30, 36, 42, 48,
//! 54 and 60 bytes, and `payload_offset == 8 + 4 x count` on every one. The
//! payload is a bytecode over the hashes: `0x20xx` reads as a test of hash `xx`,
//! `0x10xx` as a jump, `0x70xx` as a count and `0x90xx` as the end, but that is
//! a reading, not a decode, and a writer that emits it must know the semantics.
//! The tree is carried until then.
//!
//! The UI base's roots resolve through the dictionary: `gui` (`9FCFE126`),
//! `red` (`9B8DE7E4`), `green` (`4BA4BD58`), `blue` (`0977913D`) and `alpha`
//! (`3F697354`); `BDF72706`, `B5F45768`, `8FB860CF`, `E2C8865F` and `BC4EE226`
//! are unnamed. Records are subsets of their parent (7 -> 5 -> 4 -> 2).

use color_eyre::eyre::{Result, bail};

/// The only record tag the region uses.
pub const TAG: u16 = 1;

/// One record of the conditions region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The condition hashes, in record order.
    pub hashes: Vec<u32>,
    /// The payload words, kept whole until the opcodes are decoded.
    pub payload: Vec<u16>,
}

impl Node {
    /// The record's byte length: `8 + 4 x count + 2 x payload_words`.
    pub fn len(&self) -> usize {
        8 + self.hashes.len() * 4 + self.payload.len() * 2
    }

    /// Whether the record carries nothing.
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty() && self.payload.is_empty()
    }

    /// The record's bytes.
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&TAG.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&((8 + self.hashes.len() * 4) as u16).to_le_bytes());
        out.extend_from_slice(&(self.hashes.len() as u16).to_le_bytes());
        for hash in &self.hashes {
            out.extend_from_slice(&hash.to_le_bytes());
        }
        for word in &self.payload {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }
}

/// A section's conditions region, read as its records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConditionTree {
    nodes: Vec<Node>,
}

impl ConditionTree {
    /// Reads the region. The last record may be followed by up to three zero
    /// bytes of alignment; anything else that does not parse is an error.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut nodes = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if bytes.len() - at < 8 {
                if bytes[at..].iter().all(|byte| *byte == 0) {
                    break;
                }
                bail!("{} trailing bytes do not start a conditions record", bytes.len() - at);
            }
            let word = |i: usize| u16::from_le_bytes(bytes[at + i * 2..at + i * 2 + 2].try_into().unwrap());
            let tag = word(0);
            let payload_words = word(1) as usize;
            let payload_offset = word(2) as usize;
            let count = word(3) as usize;
            if tag != TAG {
                bail!("the record at {at} has tag {tag}, not {TAG}");
            }
            if payload_offset != 8 + count * 4 {
                bail!(
                    "the record at {at} says its payload starts at {payload_offset}, not {}",
                    8 + count * 4
                );
            }
            let len = 8 + count * 4 + payload_words * 2;
            if at + len > bytes.len() {
                bail!("the record at {at} runs past the region");
            }
            let mut hashes = Vec::with_capacity(count);
            for index in 0..count {
                let hash_at = at + 8 + index * 4;
                hashes.push(u32::from_le_bytes(
                    bytes[hash_at..hash_at + 4].try_into().unwrap(),
                ));
            }
            let mut payload = Vec::with_capacity(payload_words);
            for index in 0..payload_words {
                let word_at = at + payload_offset + index * 2;
                payload.push(u16::from_le_bytes(
                    bytes[word_at..word_at + 2].try_into().unwrap(),
                ));
            }
            nodes.push(Node { hashes, payload });
            at += len;
        }
        Ok(Self { nodes })
    }

    /// The region's bytes, recomputed from the records.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for node in &self.nodes {
            node.write(&mut out);
        }
        out
    }

    /// The records, in region order.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// How many records the region carries.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the region is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI base's first record, 60 bytes: seven condition hashes and a
    /// twelve-word payload. It is the fixture the framing was measured on.
    const FIRST: [u8; 60] = [
        0x01, 0x00, 0x0C, 0x00, 0x24, 0x00, 0x07, 0x00, 0x26, 0xE1, 0xCF, 0x9F, 0x06, 0x27, 0xF7,
        0xBD, 0x68, 0x57, 0xF4, 0xB5, 0xE4, 0xE7, 0x8D, 0x9B, 0x58, 0xBD, 0xA4, 0x4B, 0x3D, 0x91,
        0x77, 0x09, 0xCF, 0x60, 0xB8, 0x8F, 0x00, 0x20, 0x01, 0x20, 0x02, 0x20, 0x02, 0x10, 0x0B,
        0x70, 0x03, 0x20, 0x04, 0x20, 0x05, 0x20, 0x06, 0x20, 0x03, 0x10, 0x07, 0x50, 0x00, 0x90,
    ];

    #[test]
    fn a_record_round_trips() {
        let tree = ConditionTree::parse(&FIRST).expect("parse");
        assert_eq!(tree.len(), 1);
        let node = &tree.nodes()[0];
        assert_eq!(node.hashes.len(), 7);
        assert_eq!(node.payload.len(), 12);
        assert_eq!(node.len(), 60);
        assert_eq!(tree.bytes(), FIRST, "the framing is self-delimiting");
    }

    #[test]
    fn records_are_read_back_to_back() {
        // The first record and the second (48 bytes, five hashes) as one region.
        let mut bytes = FIRST.to_vec();
        let second = [
            0x01, 0x00, 0x0A, 0x00, 0x1C, 0x00, 0x05, 0x00, 0x26, 0xE1, 0xCF, 0x9F, 0x06, 0x27,
            0xF7, 0xBD, 0x68, 0x57, 0xF4, 0xB5, 0xE4, 0xE7, 0x8D, 0x9B, 0xCF, 0x60, 0xB8, 0x8F,
            0x00, 0x20, 0x01, 0x20, 0x02, 0x20, 0x02, 0x10, 0x09, 0x70, 0x03, 0x20, 0x04, 0x20,
            0x01, 0x10, 0x07, 0x50, 0x00, 0x90,
        ];
        bytes.extend_from_slice(&second);
        let tree = ConditionTree::parse(&bytes).expect("parse");
        assert_eq!(tree.len(), 2);
        assert_eq!(tree.nodes()[0].len(), 60);
        assert_eq!(tree.nodes()[1].len(), 48);
        assert_eq!(tree.bytes(), bytes);
    }

    #[test]
    fn a_payload_offset_that_is_not_the_formula_is_refused() {
        let mut wrong = FIRST;
        wrong[4..6].copy_from_slice(&28u16.to_le_bytes());
        assert!(ConditionTree::parse(&wrong).is_err());
    }

    #[test]
    fn a_foreign_tag_is_refused() {
        let mut wrong = FIRST;
        wrong[0..2].copy_from_slice(&2u16.to_le_bytes());
        assert!(ConditionTree::parse(&wrong).is_err());
    }

    #[test]
    fn trailing_alignment_is_accepted() {
        let mut bytes = FIRST.to_vec();
        bytes.extend_from_slice(&[0, 0]);
        let tree = ConditionTree::parse(&bytes).expect("parse");
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.bytes(), FIRST);
    }
}
