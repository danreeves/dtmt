//! The `theme` resource: a UI theme.
//!
//! ```text
//! 0                u32      how many words follow (8 in every shipped theme)
//! 4                [u32]    those words; the last of them is the record count
//! 4 + 4 * count    [N B]    the records: a u64 marker and a run of u32 values
//! after the records u32     0
//!                  u32      the name length, including its NUL
//!                  [u8]     the name, NUL-terminated, ending on the last byte
//! ```
//!
//! Two things pin the shape, and neither is enough alone:
//!
//! - the **last header word is the record count** - 18 for `darkness`, and every
//!   theme where both readings agree;
//! - the **name is the last NUL-terminated printable run**, and the two words
//!   before it must be a zero and the name's own length.
//!
//! Together they give the record width, `(records_end - header_end) / count`, which
//! is **not** a constant: `darkness` has 24-byte records, and the themes whose gap
//! is not a multiple of 24 are the ones an earlier draft rejected. Each step is an
//! equality - the word after the records is zero, the word after that is the name
//! length, and the gap divides exactly by the record count - so a file that does not
//! satisfy all of them is rejected rather than guessed at.
//!
//! # What is carried, not claimed
//!
//! A record's marker is identical across every record of the sampled themes, so it
//! reads as a type tag rather than a key, and its values read byte-wise like
//! colours; the seven words under the count are opaque. All three are carried
//! verbatim rather than named after a guess.
//!
//! # A trap in the evidence
//!
//! A first draft derived the record count from the gap alone and required 24-byte
//! records. It decoded 25 themes - and those 25 were exactly the ones where a
//! 24-byte width happened to hold, so the agreement between the header word and the
//! count was measured *on a sample selected by the assumption being tested*. Reading
//! the count from the header and the width from the gap removed the bias and is what
//! the 191-theme sweep in `examples/theme_probe.rs` measures.

//! # It is deliberately not wired into the bundle dispatch
//!
//! It decodes a minority of the shipped themes, and before it existed this type was
//! written out as an opaque blob, which round-trips. Registering a reader that fails
//! on most of them would turn working extractions into errors, so the type stays
//! unregistered until the layout is understood.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type id, for callers that need to recognise the payload.
pub const THEME_TYPE: u64 = 0x38bb_9442_048a_7fbd;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// One theme record.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeRecord {
    /// The record's first eight bytes.
    pub marker: u64,
    /// The values that follow it; how many is the theme's record width.
    pub values: Vec<u32>,
}

/// A UI theme.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    /// The words under the count word, without the record count that closes them.
    pub words: Vec<u32>,
    /// The records.
    pub records: Vec<ThemeRecord>,
    /// The theme's name, without its terminator.
    pub name: String,
}

impl Theme {
    /// Reads a theme from its cooked bytes.
    pub fn from_binary(b: &[u8]) -> Result<Self> {
        if b.len() < 8 {
            bail!("a theme is shorter than its count word and one word");
        }
        let count = u32_at(b, 0) as usize;
        if count == 0 {
            bail!("a theme's count word is zero, so it names no record count");
        }
        let header_end = 4 + count * 4;
        if header_end + 8 > b.len() {
            bail!("a theme's header of {count} words runs past its body");
        }
        let words: Vec<u32> = (0..count - 1).map(|i| u32_at(b, 4 + i * 4)).collect();
        let records_n = u32_at(b, header_end - 4) as usize;
        if records_n == 0 {
            bail!("a theme's record count is zero");
        }

        // The name: the last NUL-terminated printable run, whose length word sits
        // directly before it behind a zero word.
        let last = b.len() - 1;
        if b[last] != 0 {
            bail!("a theme does not end on a NUL-terminated name");
        }
        let mut start = last;
        while start > 0 && (0x20..0x7f).contains(&b[start - 1]) {
            start -= 1;
        }
        if start < header_end + 8 {
            bail!("a theme's name overlaps its records or their tail");
        }
        let name_len = last - start + 1;
        if u32_at(b, start - 4) as usize != name_len {
            bail!(
                "a theme's name is {name_len} bytes but the word before it says {}",
                u32_at(b, start - 4)
            );
        }
        if u32_at(b, start - 8) != 0 {
            bail!(
                "the word before a theme's name length is {}, not zero",
                u32_at(b, start - 8)
            );
        }
        let name = String::from_utf8(b[start..last].to_vec())
            .map_err(|e| color_eyre::eyre::eyre!("a theme's name is not text: {e}"))?;

        // The width follows from the two anchors, and the division is the check that
        // the header word really is a record count.
        let records_end = start - 8;
        let span = records_end - header_end;
        if span % records_n != 0 {
            bail!(
                "a theme's {span} record bytes do not divide into its {records_n} records"
            );
        }
        let width = span / records_n;
        if width < 8 || (width - 8) % 4 != 0 {
            bail!("a theme's record width {width} is not a marker plus whole words");
        }
        let values = (width - 8) / 4;
        let mut records = Vec::with_capacity(records_n);
        for at in (header_end..records_end).step_by(width) {
            records.push(ThemeRecord {
                marker: u64_at(b, at),
                values: (0..values).map(|i| u32_at(b, at + 8 + i * 4)).collect(),
            });
        }

        Ok(Self {
            words,
            records,
            name,
        })
    }

    /// Writes the theme back, deriving the count word and the record count.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let count = self.words.len() + 1;
        if count > u32::MAX as usize {
            bail!("a theme carries more words than its count word can hold");
        }
        if self.records.len() > u32::MAX as usize {
            bail!("a theme carries more records than its count word can hold");
        }
        let width = match self.records.first() {
            Some(record) => record.values.len(),
            None => 0,
        };
        if self.records.iter().any(|record| record.values.len() != width) {
            bail!("a theme's records disagree on their width");
        }

        let mut out = Vec::new();
        out.extend_from_slice(&(count as u32).to_le_bytes());
        for word in &self.words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for record in &self.records {
            out.extend_from_slice(&record.marker.to_le_bytes());
            for value in &record.values {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&((self.name.len() + 1) as u32).to_le_bytes());
        out.extend_from_slice(self.name.as_bytes());
        out.push(0);
        Ok(out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the resource from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

/// Decompiles a `theme` variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a theme resource needs a variant");
    };
    let theme = Theme::from_binary(variant.data())?;
    Ok(vec![UserFile::new(theme.to_sjson()?.into_bytes())])
}

/// Compiles a `theme` SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let theme = Theme::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(theme.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::Theme);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A theme with a given record width, shaped like the shipped ones: `count = 8`,
    /// eight words whose last is the record count, then the records, then the name.
    fn shaped(name: &str, records: usize, values: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for word in [0xfd6754bfu32, 0xf3969268, 0x1063b31a, 0x004a5fa2, 0x4b3c77cb, 0xf47995b4, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&(records as u32).to_le_bytes());
        for i in 0..records {
            bytes.extend_from_slice(&(0xf479_95b4_4b3c_77cbu64 + i as u64).to_le_bytes());
            for k in 0..values {
                bytes.extend_from_slice(&(0xe10f948eu32 + k as u32).to_le_bytes());
            }
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&((name.len() + 1) as u32).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes
    }

    #[test]
    fn a_shipped_shaped_theme_round_trips() {
        let bytes = shaped("darkness", 18, 4);
        assert_eq!(bytes.len(), 485, "the shipped size for a 24-byte width");
        let theme = Theme::from_binary(&bytes).expect("the sample parses");
        assert_eq!(theme.words.len(), 7, "the words under the count");
        assert_eq!(theme.records.len(), 18, "the header word is the record count");
        assert_eq!(theme.records[0].values.len(), 4, "the derived width");
        assert_eq!(theme.records[0].marker, 0xf479_95b4_4b3c_77cb);
        assert_eq!(theme.records[0].values[0], 0xe10f948e);
        assert_eq!(theme.name, "darkness");
        assert_eq!(theme.to_binary().unwrap(), bytes, "byte-for-byte");
        let sjson = theme.to_sjson().unwrap();
        assert_eq!(Theme::from_sjson(&sjson).unwrap(), theme);
    }

    #[test]
    fn a_narrower_record_width_round_trips() {
        // The width is derived, so a theme whose records are not 24 bytes works too -
        // which is the case an earlier draft rejected.
        let bytes = shaped("performance_forge", 105, 2);
        let theme = Theme::from_binary(&bytes).expect("a 16-byte width parses");
        assert_eq!(theme.records.len(), 105);
        assert_eq!(theme.records[0].values.len(), 2);
        assert_eq!(theme.to_binary().unwrap(), bytes, "byte-for-byte");
    }

    #[test]
    fn a_count_that_does_not_divide_the_gap_is_rejected() {
        let mut bytes = shaped("darkness", 18, 4);
        let at = 4 + 7 * 4;
        bytes[at..at + 4].copy_from_slice(&17u32.to_le_bytes());
        assert!(Theme::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_non_zero_word_before_the_name_is_rejected() {
        let mut bytes = shaped("darkness", 18, 4);
        let at = bytes.len() - 9 - 8;
        bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
        assert!(Theme::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_name_length_that_lies_is_rejected() {
        let mut bytes = shaped("darkness", 18, 4);
        let at = bytes.len() - 9 - 4;
        bytes[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
        assert!(Theme::from_binary(&bytes).is_err());
    }
}
