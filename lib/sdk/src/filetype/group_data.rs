//! Reading and writing a declaration's group data.
//!
//! The group data is what tells the engine, per group, which material variables
//! live where. It is a 32-byte global header, then per group three 16-byte
//! descriptors, then the variable tables those descriptors point at. A canonical
//! variable record is 20 bytes:
//!
//! ```text
//! u32 type            // 0 scalar, 1 float2, 2 float3, 3 float4, 4 matrix
//! u32 flags
//! u32 name_hash       // murmur32 of the variable name
//! u32 cbuffer_offset  // byte offset in the cbuffer the descriptor names
//! u32 size            // 4, 8, 12, 16 or 64, matching the type
//! ```
//!
//! Two things make this awkward to write from scratch, and both are why the
//! emitter is a *rebuilder* rather than a constructor:
//!
//! - The tables are **byte-identical across groups** in a shipped section, and
//!   only the descriptors' `Y` and the per-group headers vary. So the bytes for
//!   a group are not laid out one after another in a way a fresh writer would
//!   choose; they are shared.
//! - Besides the canonical records, the same variables appear as **packed
//!   28-byte copies** in a run that the engine also reads, keyed by a cbuffer
//!   hash. Writing only the canonical records leaves the old values in effect.
//!
//! So [`GroupData::rebuild`] takes the template's bytes, finds every framing of
//! the variables it knows about, and rewrites them from a [`VariableTable`]. A
//! template that already has the right variables round-trips byte for byte,
//! which is what the tests pin.
//!
//! The cbuffer offset of each variable comes from the compiled program's
//! reflection (`shader43 --slots` reports the slot of every variable), not from
//! the declaration: the engine reads the slot, and the compiled DXBC is what
//! says where a name ended up.

use color_eyre::eyre;
use color_eyre::eyre::{Result, bail};

use crate::murmur::Murmur32;

/// The record type codes the engine uses, with the byte size each implies.
const TYPE_SIZES: [(u32, u32); 5] = [(0, 4), (1, 8), (2, 12), (3, 16), (4, 64)];

/// The record length of the canonical 20-byte variable record.
const RECORD_LEN: usize = 20;

/// The record length of the packed 28-byte copy.
const PACKED_LEN: usize = 28;

/// The cbuffer hash that keys the packed copies of the per-object table. The
/// shipped sections use this value for their per-object cbuffer.
const PACKED_KEY: u32 = 0xB563_9618;

/// The first record of the engine's `global_viewport` table: `6BC91D73` on all
/// seven sections measured - the six small families and the UI base - and one
/// per group. The engine table is otherwise indistinguishable from the
/// material's (both are runs of canonical records with a count word), so this
/// measured word is what tells them apart. A section whose engine table starts
/// elsewhere would be misidentified; `engine_records` exists so a caller can
/// check.
const ENGINE_FIRST_HASH: u32 = 0x6BC9_1D73;

/// Whether a validated run is an engine table, by the one measured word that
/// identifies its first record.
fn is_engine_run(run: &[Record]) -> bool {
    run.first()
        .is_some_and(|record| record.hash == ENGINE_FIRST_HASH)
}

/// The largest cbuffer offset a record may carry, used to recognise a record by
/// scanning.
const MAX_OFFSET: u32 = 8192;

/// The three 16-byte descriptors of one group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptors {
    /// The engine's `global_viewport` table: its offset, a type count, its
    /// cbuffer hash and its flags.
    pub engine: Descriptor,
    /// The material's own per-object table.
    pub object: Descriptor,
    /// The packed-copy run.
    pub packed: Descriptor,
}

/// One 16-byte descriptor: where a table starts, how many kinds it has, which
/// cbuffer it fills, and the flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Descriptor {
    /// Byte offset of the table within the group data.
    pub offset: u32,
    /// A count of kinds, as the engine writes it.
    pub count: u32,
    /// The cbuffer the table's offsets refer to.
    pub cbuffer: u32,
    /// The descriptor's flags.
    pub flags: u32,
}

/// One canonical variable record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// The record type, and through it the size.
    pub kind: u32,
    /// The record's flags.
    pub flags: u32,
    /// murmur32 of the variable's name.
    pub hash: u32,
    /// The variable's byte offset in its cbuffer.
    pub offset: u32,
    /// The variable's size in bytes.
    pub size: u32,
}

impl Record {
    /// Reads a record, if the bytes at `at` are one.
    fn read(data: &[u8], at: usize) -> Option<Self> {
        if at + RECORD_LEN > data.len() {
            return None;
        }
        let word = |i: usize| -> Option<u32> {
            Some(u32::from_le_bytes(
                data[at + i..at + i + 4].try_into().ok()?,
            ))
        };
        let record = Self {
            kind: word(0)?,
            flags: word(4)?,
            hash: word(8)?,
            offset: word(12)?,
            size: word(16)?,
        };
        record.looks_valid().then_some(record)
    }

    /// Whether the record's fields are in range. Scanning for records at every
    /// byte offset needs a strong filter, and the size has to match the type.
    fn looks_valid(&self) -> bool {
        if self.hash == 0 || self.kind > 12 || self.flags > 3 || self.offset > MAX_OFFSET {
            return false;
        }
        match TYPE_SIZES.iter().find(|(code, ..)| *code == self.kind) {
            Some((_, size)) => *size == self.size,
            // A kind the table does not size: the engine's own extensions.
            None => true,
        }
    }

    /// The record's bytes.
    fn write(&self, out: &mut Vec<u8>) {
        for word in [self.kind, self.flags, self.hash, self.offset, self.size] {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }

    /// The record's bytes, over four words of `data` at an offset.
    fn write_at(&self, data: &mut [u8], at: usize) {
        if at + RECORD_LEN > data.len() {
            return;
        }
        for (index, word) in [self.kind, self.flags, self.hash, self.offset, self.size]
            .into_iter()
            .enumerate()
        {
            data[at + index * 4..at + index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
    }
}

/// The record length of a dependencies entry.
pub const DEPENDENCY_LEN: usize = 8;

/// One entry of a section's dependencies table: the library a declaration was built
/// against.
///
/// The entry is **8 bytes**, and it is one 64-bit MurmurHash of the dependency's
/// path stored little-endian. The earlier readings of these bytes - first a
/// 16-byte record, then a `{tag, name}` pair - were both wrong in the same way:
/// the two words are the two halves of one hash, not two fields. The dictionary
/// confirms it: `209FB8C3C0A8C3A4` is the long hash of
/// `core/stingray_renderer/renderer`, and its two words in little-endian order
/// are exactly the entry's words.
///
/// The count and hash that an earlier reading gave this entry are the group
/// data's own first two words, read through a window twice as wide as the entry.
/// `group_data_offset - dependency_offset` is 8 on all seven sections measured,
/// which is the check that settles it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Murmur64 of the dependency's path, [`Dependency::RENDERER`] on every
    /// shipped section.
    pub id: u64,
}

impl Dependency {
    /// `core/stingray_renderer/renderer`, the library every shipped section names
    /// as its one dependency.
    pub const RENDERER: u64 = 0x209F_B8C3_C0A8_C3A4;

    /// Reads the entry at an offset within a section.
    pub fn parse(data: &[u8], at: usize) -> Option<Self> {
        let entry = data.get(at..at + DEPENDENCY_LEN)?;
        Some(Self {
            id: u64::from_le_bytes(entry.try_into().ok()?),
        })
    }

    /// Reads every entry of a section's dependencies table.
    pub fn read(data: &[u8], at: usize, count: usize) -> Vec<Self> {
        (0..count)
            .filter_map(|index| Self::parse(data, at + index * DEPENDENCY_LEN))
            .collect()
    }

    /// The entry a generated section writes: the one every shipped section has.
    pub fn of() -> Self {
        Self {
            id: Self::RENDERER,
        }
    }

    /// The bytes of the entry.
    pub fn write(&self) -> [u8; DEPENDENCY_LEN] {
        self.id.to_le_bytes()
    }
}

/// One channel a group's channel table declares: a texture binding and the
/// parameters bound to it, all sharing one name hash.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// murmur32 of the channel's name, the key the records group by.
    pub hash: u32,
    /// The records, in the order the table lists them: the type 5 binding first,
    /// then its parameters.
    pub records: Vec<Record>,
}

impl Channel {
    /// The byte offset the texture binds at, which is the first record's.
    pub fn offset(&self) -> u32 {
        self.records.first().map_or(0, |record| record.offset)
    }
}

/// One variable a generated group data declares: its name, and where the
/// compiled program put it.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    /// The variable's name, or - for a variable read out of a shipped table whose
    /// name the dictionary does not have - the murmur32 of it, as a string.
    pub name: String,
    /// The variable's byte offset in its cbuffer, from the program's reflection.
    pub offset: u32,
    /// The record type, 0 through 4.
    pub kind: u32,
}

impl Variable {
    /// A variable the declaration names.
    pub fn new(name: impl Into<String>, offset: u32, kind: u32) -> Self {
        Self {
            name: name.into(),
            offset,
            kind,
        }
    }

    /// A variable known only by its hash, which is how a shipped table reads
    /// when the dictionary has no name for it. The name is the hash in hex, so
    /// hashing it again gives the hash back and a table read this way rewrites
    /// byte for byte.
    pub fn from_hash(hash: u32, offset: u32, kind: u32) -> Self {
        Self {
            name: format!("{hash:#010x}"),
            offset,
            kind,
        }
    }

    /// murmur32 of the name, the key every framing uses.
    pub fn hash(&self) -> u32 {
        // A name that is a hash already - `0xE503152C` - is taken as it is, so a
        // table read by hash round-trips instead of hashing the hex digits.
        let trimmed = self.name.strip_prefix('#').unwrap_or(&self.name);
        if trimmed.len() == 10 && trimmed.starts_with("0x") {
            let digits = &trimmed[2..];
            if digits.chars().all(|char| char.is_ascii_hexdigit())
                && let Ok(hash) = u32::from_str_radix(digits, 16)
            {
                return hash;
            }
        }
        u32::from(Murmur32::hash(self.name.as_bytes()))
    }

    /// The variable's size, from its type.
    pub fn size(&self) -> u32 {
        TYPE_SIZES
            .iter()
            .find(|(code, ..)| *code == self.kind)
            .map(|(_, size)| *size)
            .unwrap_or(0)
    }
}

/// A declaration's group data, kept as the bytes it is.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupData {
    data: Vec<u8>,
}

impl GroupData {
    /// Wraps the group data of a section.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// The bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// The bytes, owned, for a caller that has to modify them.
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// The number of groups the data carries.
    pub fn group_count(&self) -> u32 {
        self.word(0).unwrap_or(0)
    }

    /// The hash in the header. Nothing has established what it is a hash *of*:
    /// it is not the murmur32 of the group data's own bytes for any seed tried,
    /// so a generated group data takes it from its template. It is also not the
    /// dependency entry's content - that entry is eight bytes and is the
    /// dependency's own 64-bit hash; the count and hash here are the group data's
    /// own first two words.
    pub fn hash(&self) -> u32 {
        self.word(4).unwrap_or(0)
    }

    /// One word at an offset within the group data.
    fn word(&self, at: usize) -> Option<u32> {
        let end = at.checked_add(4)?;
        (end <= self.data.len()).then(|| u32::from_le_bytes(self.data[at..end].try_into().unwrap()))
    }

    /// The canonical records of a table, read from a descriptor.
    ///
    /// The record count is not in the descriptor, so a table is read until its
    /// records stop being records. That only works when a table is followed by
    /// something else, which is why a caller that must not read into the next
    /// table uses [`GroupData::run_at`] instead.
    pub fn table(&self, descriptor: Descriptor) -> Vec<Record> {
        let mut records = Vec::new();
        let mut at = descriptor.offset as usize;
        while let Some(record) = Record::read(&self.data, at) {
            records.push(record);
            at += RECORD_LEN;
        }
        records
    }

    /// A table at `at`: the consecutive canonical records there, if the word
    /// four bytes before them is their count.
    ///
    /// The count word is the rule that was measured and documented before the
    /// reader implemented it, and without it a table's 12-byte header tail reads
    /// as a one-record table of its own. The UI base's material table was read at
    /// +76 for exactly that reason; the real table is at +88.
    pub fn run_at(&self, at: usize) -> Vec<Record> {
        // A run inside the descriptor table is not a table: the descriptors are
        // metadata, so header bytes that happen to read as records are skipped.
        if at < self.descriptor_bytes() || at < 4 {
            return Vec::new();
        }
        let mut records = Vec::new();
        let mut cursor = at;
        while let Some(record) = Record::read(&self.data, cursor) {
            records.push(record);
            cursor += RECORD_LEN;
        }
        if self.word(at - 4) == Some(records.len() as u32) {
            records
        } else {
            Vec::new()
        }
    }

    /// How far the descriptor table reaches: the 8-byte global header plus one
    /// set of three 16-byte descriptors.
    ///
    /// There is one set, not one per group, even when the header's group count is
    /// higher: a shipped three-group section has its three descriptors at +8, +24
    /// and +40 and its first table at +136.
    pub fn descriptor_bytes(&self) -> usize {
        8 + 48
    }

    /// The three descriptors: the engine's `global_viewport` table, the
    /// material's own table, and the packed-copy run.
    ///
    /// The group count is in the header but the descriptor set is not per group,
    /// so this takes no group. What a *second* group adds is a table of its own,
    /// laid out after the first group's, which is what makes the per-group
    /// interface lengths differ.
    pub fn descriptors(&self) -> Option<Descriptors> {
        let at = 8;
        let read = |i: usize| -> Option<Descriptor> {
            Some(Descriptor {
                offset: self.word(at + i * 16)?,
                count: self.word(at + i * 16 + 4)?,
                cbuffer: self.word(at + i * 16 + 8)?,
                flags: self.word(at + i * 16 + 12)?,
            })
        };
        Some(Descriptors {
            engine: read(0)?,
            object: read(1)?,
            packed: read(2)?,
        })
    }

    /// Every run of two or more canonical records, in the order it appears.
    pub fn runs(&self) -> Vec<Vec<Record>> {
        let mut runs = Vec::new();
        let mut at = 0;
        while at + 2 * RECORD_LEN <= self.data.len() {
            if Record::read(&self.data, at).is_some() {
                let run = self.run_at(at);
                let len = run.len();
                if len >= 2 {
                    runs.push(run);
                    // Skip past the run and start looking for the next one. The
                    // byte after a run may still read as a record, so the search
                    // resumes one record further on rather than right after it.
                    at += len * RECORD_LEN;
                    continue;
                }
            }
            at += 1;
        }
        runs
    }

    /// Every run of packed copies, keyed by the run's cbuffer hash. A run of one
    /// is real: a declaration may have a single material variable.
    pub fn packed_runs(&self) -> Vec<(u32, Vec<u32>)> {
        let mut runs: Vec<(u32, Vec<u32>)> = Vec::new();
        let mut at = 0;
        while at + PACKED_LEN <= self.data.len() {
            if is_packed(&self.data, at) {
                let key = u32::from_le_bytes(self.data[at + 20..at + 24].try_into().unwrap());
                let mut hashes = Vec::new();
                let mut cursor = at;
                while is_packed(&self.data, cursor) {
                    hashes.push(u32::from_le_bytes(
                        self.data[cursor..cursor + 4].try_into().unwrap(),
                    ));
                    cursor += PACKED_LEN;
                }
                if hashes.len() >= 1 {
                    runs.push((key, hashes));
                }
                at = cursor.max(at + PACKED_LEN);
                continue;
            }
            at += 1;
        }
        runs
    }

    /// The material's own variable table: the first validated table that is not
    /// the engine's.
    ///
    /// This is a heuristic, and the reason it is a heuristic is the one thing
    /// that would settle it: the per-group headers name each table's offset, and
    /// they are not decoded yet. The engine's table is identified by its first
    /// record ([`ENGINE_FIRST_HASH`], measured on seven sections); everything
    /// else that passes the count rule is a candidate, and the first one is the
    /// material's table in every section measured - at +88 on the UI base, after
    /// the engine's 69 records on the six small families. A caller that knows the
    /// offset from the headers should use [`GroupData::rebuild_at`].
    pub fn object_table(&self) -> Option<(usize, Vec<Record>)> {
        let mut at = 0;
        while at + 2 * RECORD_LEN <= self.data.len() {
            let run = self.run_at(at);
            if run.is_empty() {
                at += 1;
                continue;
            }
            if is_engine_run(&run) {
                at += run.len() * RECORD_LEN;
                continue;
            }
            return Some((at, run));
        }
        None
    }
    /// The channels table: the third table of a group, the one after the
    /// material's own variables.
    ///
    /// The stride is measured, not fitted. A table's record count is the word
    /// four bytes *before* its first record, and the next table's 12-byte header
    /// begins where this table's records end - so the header after a table of
    /// `len` records starts at `start + 20 x len`, its count is at `+ 8`, and its
    /// records at `+ 12`. On `427B5E6E` that is `1528 + 20 x 58 = 2688`, whose
    /// header is `{400, 0, 19}` and whose records are the nineteen channels.
    ///
    /// A channel is however many records it is: a *texture* channel is three - a
    /// type 5 binding followed by two type 1 parameters, the UV scale and offset
    /// the sampler takes - and a scalar channel is one. So the record count is
    /// not the channel count, and [`GroupData::channels`] is what to read for
    /// that. `427B5E6E` is 19 records for 15 channels, which is the mix.
    ///
    /// The stride is only sound when the channel table follows the material's
    /// immediately. On the UI base the engine table sits between them, so the
    /// stride lands on the engine run; that is refused rather than read as 69
    /// channels, and the UI base's channel table stays undecoded until the
    /// per-group headers are.
    pub fn channel_table(&self) -> Option<(usize, Vec<Record>)> {
        let (at, table) = self.object_table()?;
        let header = at + table.len() * RECORD_LEN;
        let count = self.word(header + 8)? as usize;
        let start = header + 12;
        if count == 0 {
            return None;
        }
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            records.push(Record::read(&self.data, start + index * RECORD_LEN)?);
        }
        if is_engine_run(&records) {
            return None;
        }
        Some((start, records))
    }

    /// Rewrites the channel table from a list of channels.
    ///
    /// This is the one table a from-scratch group data has to *write* rather than
    /// copy: the channels are what the declaration declares, where the engine's
    /// `global_viewport` table and the material's own variables are carried. A
    /// channel keeps the records the template gave it and takes only the new name
    /// and offset, for the same reason the variable table does - a type 5 binding
    /// and its two type 1 parameters are engine-side widths, not something a
    /// declaration's type can say.
    ///
    /// So a table read from a section and rewritten with the channels it was read
    /// as comes back byte for byte, which is what the tests pin. The record count
    /// must be the template's: adding or dropping a channel changes the count and
    /// moves the next table, and a new channel needs a cbuffer offset from the
    /// compiled program's reflection, which is not this call's input.
    pub fn rebuild_channels(&self, channels: &[Channel]) -> Result<Vec<u8>> {
        let (at, old) = self
            .channel_table()
            .ok_or_else(|| eyre::eyre!("the material's own channel table was not found"))?;
        // A channel is however many records it is, not a fixed three: a *texture*
        // channel is three - the type 5 binding and its two type 1 parameters -
        // and a scalar channel is one.
        let wanted: usize = channels.iter().map(|channel| channel.records.len()).sum();
        if wanted != old.len() {
            bail!(
                "the template has {} channel records and {wanted} were asked for; adding or \
                 dropping channels needs the count word to move and the offsets from the \
                 compiled program's reflection",
                old.len()
            );
        }
        let mut data = self.data.clone();
        let mut slot = 0;
        for channel in channels {
            for record in &channel.records {
                // The slot's kind, flags and size are the engine's: a type 5
                // binding is four bytes wide and no declaration's type can say
                // it. Only the name and the offset change.
                let template = &old[slot];
                Record {
                    kind: template.kind,
                    flags: template.flags,
                    hash: channel.hash,
                    offset: record.offset,
                    size: template.size,
                }
                .write_at(&mut data, at + slot * RECORD_LEN);
                slot += 1;
            }
        }
        Ok(data)
    }

    /// The channels a group's channel table declares, one per texture binding.
    ///
    /// A channel is a run of records that share a name hash - the type 5 binding
    /// and the parameters bound to it - so the channels are the distinct names in
    /// the table, in the order the table lists them. That order is the order the
    /// engine reads, and it is what a generated channel table has to keep.
    pub fn channels(&self) -> Vec<Channel> {
        let Some((_, records)) = self.channel_table() else {
            return Vec::new();
        };
        let mut channels: Vec<Channel> = Vec::new();
        for record in records {
            match channels
                .iter_mut()
                .find(|channel| channel.hash == record.hash)
            {
                Some(channel) => channel.records.push(record),
                None => channels.push(Channel {
                    hash: record.hash,
                    records: vec![record],
                }),
            }
        }
        channels
    }

    /// The engine's `global_viewport` records: the run whose first record is
    /// [`ENGINE_FIRST_HASH`], measured on all seven sections. There is one per
    /// group and they are byte-identical, which is what the engine's table being
    /// engine-side means; this returns the first.
    pub fn engine_records(&self) -> Option<Vec<Record>> {
        self.runs().into_iter().find(|run| is_engine_run(run))
    }

    /// Rebuilds the group data for a material's own variables: the canonical
    /// records and the packed copies are both rewritten, and every other table -
    /// the engine's `global_viewport` and the channels - is left as it is.
    ///
    /// A template whose table already matches comes back byte for byte, which is
    /// what makes this usable as the emitter's own test.
    ///
    /// The variables must fit the run that is already there. Growing one is a
    /// separate job, because every offset pointing into it moves with it.
    pub fn rebuild(&self, variables: &[Variable]) -> Result<Vec<u8>> {
        let (at, table) = self
            .object_table()
            .ok_or_else(|| eyre::eyre!("the material's own variable table was not found"))?;
        self.rebuild_at(at, &table, variables)
    }

    /// Rewrites the packed copies of the run keyed by [`PACKED_KEY`], and nothing
    /// else. This is a separate step because the packed framing is the one part of
    /// the group data whose shape is not confirmed against a shipped section: the
    /// key alone does not prove a run, and a false match rewrites bytes that were
    /// not a table. So it is opt-in, and a caller that has confirmed the framing
    /// for its template calls this after [`GroupData::rebuild`].
    pub fn rebuild_packed(&self, variables: &[Variable]) -> Result<Vec<u8>> {
        let mut data = self.data.clone();
        let mut rewritten = 0;
        let mut at = 0;
        while at + PACKED_LEN <= data.len() {
            if !is_packed(&data, at) {
                at += 1;
                continue;
            }
            // A whole run at a time: every record of it satisfies the shape, so
            // stepping a byte would match the run's own second record and write
            // over the cbuffer keys.
            let mut end = at;
            while is_packed(&data, end) {
                end += PACKED_LEN;
            }
            let key = u32::from_le_bytes(data[at + 20..at + 24].try_into().unwrap());
            if key == PACKED_KEY {
                // Only as many copies as the run holds. A packed run is keyed by
                // its cbuffer and is not the same length as the canonical table,
                // so it is never grown or shrunk here.
                let room = (end - at) / PACKED_LEN;
                for (index, variable) in variables.iter().enumerate().take(room) {
                    let slot = at + index * PACKED_LEN;
                    data[slot..slot + 4].copy_from_slice(&variable.hash().to_le_bytes());
                    rewritten += 1;
                }
            }
            at = end;
        }
        if rewritten == 0 && !variables.is_empty() {
            bail!("the packed run was not found");
        }
        Ok(data)
    }

    /// Rebuilds the run of canonical records at `table_at`, given the run's
    /// current length in records. This is the entry point for a caller that read
    /// the offset out of the per-group headers, and the one that does not depend
    /// on [`GroupData::object_table`]'s heuristic.
    pub fn rebuild_at(
        &self,
        table_at: usize,
        table: &[Record],
        variables: &[Variable],
    ) -> Result<Vec<u8>> {
        if self.group_count() == 0 {
            bail!("the group data carries no groups");
        }
        let mut data = self.data.clone();
        rewrite_table(&mut data, table_at, &table, variables)?;

        Ok(data)
    }
}

/// Rewrites a run of canonical records in place, shrinking the run to fit and
/// zeroing the tail a shorter table leaves behind, so a stale record cannot be
/// read back.
fn rewrite_table(data: &mut [u8], at: usize, old: &[Record], variables: &[Variable]) -> Result<()> {
    let old_bytes = old.len() * RECORD_LEN;
    if old.is_empty() {
        bail!("the table at {at} holds no records");
    }
    if at + old_bytes > data.len() {
        bail!("the table at {at} runs past the end of the group data");
    }
    let new = variables.len() * RECORD_LEN;
    if new > old_bytes {
        bail!(
            "the table at {at} holds {old_bytes} bytes, which does not fit {new} bytes of records"
        );
    }
    let mut bytes = Vec::with_capacity(new);
    for (index, variable) in variables.iter().enumerate() {
        // The kind, the flags and the size are the engine's, not the
        // declaration's: a slot keeps what the template gave it and takes only
        // the new name and offset, so renaming a variable is otherwise byte for
        // byte what it was. A kind above 4 is one of the engine's own - a texture
        // binding is kind 5 and four bytes wide - and its size is nothing the
        // declaration's type can say, so a slot that already exists must not have
        // its size recomputed.
        let (kind, flags, size) = old
            .get(index)
            .map_or((variable.kind, 0, variable.size()), |record| {
                (record.kind, record.flags, record.size)
            });
        Record {
            kind,
            flags,
            hash: variable.hash(),
            offset: variable.offset,
            size,
        }
        .write(&mut bytes);
    }
    data[at..at + new].copy_from_slice(&bytes);
    // Any tail the shorter table leaves behind is zeroed, so it cannot be read
    // as a stale record.
    data[at + new..at + old_bytes].fill(0);
    // The count word four bytes before the first record is the table's length,
    // so a shrink moves it too: a stale count makes the table unreadable to the
    // count-validated walk that found it.
    if at >= 4 {
        data[at - 4..at].copy_from_slice(&(variables.len() as u32).to_le_bytes());
    }
    Ok(())
}

/// Rewrites a packed run: 28-byte records keyed by the run's cbuffer hash.
/// Returns how many records were rewritten.
fn rewrite_packed(
    data: &mut [u8],
    descriptor: Descriptor,
    variables: &[Variable],
) -> Result<usize> {
    let start = descriptor.offset as usize;
    if start + PACKED_LEN > data.len() {
        bail!("the packed run at {start} is out of range");
    }
    // The run starts with its own count and key; find where the records begin by
    // walking the 28-byte shape from the descriptor's offset.
    let mut at = start;
    let mut count = 0;
    while at + PACKED_LEN <= data.len() && is_packed(data, at) {
        count += 1;
        at += PACKED_LEN;
    }
    if count == 0 {
        return Ok(0);
    }
    let mut written = 0;
    for (index, variable) in variables.iter().enumerate() {
        if index >= count {
            break;
        }
        let at = start + index * PACKED_LEN;
        // Only the name hash is the variable's own; the rest of the packed record
        // is the cbuffer binding the template laid down, which stays.
        data[at..at + 4].copy_from_slice(&variable.hash().to_le_bytes());
        written += 1;
    }
    Ok(written)
}

/// Whether the bytes at `at` are a packed record: a hash, then the cbuffer
/// binding, then the run's key and a zero.
fn is_packed(data: &[u8], at: usize) -> bool {
    if at + PACKED_LEN > data.len() {
        return false;
    }
    let word = |i: usize| u32::from_le_bytes(data[at + i..at + i + 4].try_into().unwrap());
    // `{name hash, a, b, byte offset, kind, cbuffer hash, type}`: the fourth word
    // is the variable's byte offset in the cbuffer the fifth names, the fifth is
    // c_per_object's hash, and the seventh is 2. Checking for a zero in the
    // seventh, as this did, found a run of one in data that holds dozens.
    word(0) != 0 && word(16) == 1 && word(20) == PACKED_KEY && word(24) == 2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A group data in the shipped shape: a 32-byte global header, one group of
    /// three 16-byte descriptors, then the engine's own table, a gap, the
    /// material's own table, and the packed run.
    ///
    /// The descriptors deliberately do *not* name where the tables are, because
    /// the shipped ones do not either: the emitter has to find them.
    fn template() -> GroupData {
        // 32 bytes of header, then room for the three descriptors at +8..+56.
        let mut data = vec![0u8; 56];
        data[0..4].copy_from_slice(&1u32.to_le_bytes()); // one group
        let descriptors = [
            Descriptor {
                offset: 928,
                count: 7,
                cbuffer: 0x516D5CCD,
                flags: 0x101,
            },
            Descriptor {
                offset: 0,
                count: 0,
                cbuffer: 0xB3A2EB88,
                flags: 0x101,
            },
            Descriptor {
                offset: 24,
                count: 0,
                cbuffer: PACKED_KEY,
                flags: 0,
            },
        ];
        for (index, descriptor) in descriptors.iter().enumerate() {
            let at = 8 + index * 16;
            for (word_index, word) in [
                descriptor.offset,
                descriptor.count,
                descriptor.cbuffer,
                descriptor.flags,
            ]
            .into_iter()
            .enumerate()
            {
                data[at + word_index * 4..at + word_index * 4 + 4]
                    .copy_from_slice(&word.to_le_bytes());
            }
        }
        // The engine's own table: a 12-byte header whose count is 2, then two
        // records. Its first record is the measured engine marker.
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(&2u32.to_le_bytes());
        for record in [
            Record {
                kind: 3,
                flags: 0,
                hash: ENGINE_FIRST_HASH,
                offset: 0,
                size: 16,
            },
            Record {
                kind: 0,
                flags: 0,
                hash: var("time", 16, 0).hash(),
                offset: 16,
                size: 4,
            },
        ] {
            record.write(&mut data);
        }
        // The material's own table: its header, then three records.
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(&3u32.to_le_bytes());
        for (name, offset, kind) in [("texture_map", 0, 3), ("world", 16, 4), ("mod_tint", 80, 3)] {
            Record {
                kind,
                flags: 0,
                hash: var(name, offset, kind).hash(),
                offset,
                size: size_of_kind(kind),
            }
            .write(&mut data);
        }
        // The packed run: three 28-byte copies of the same names.
        for name in ["texture_map", "world", "mod_tint"] {
            data.extend_from_slice(&var(name, 0, 3).hash().to_le_bytes());
            data.extend_from_slice(&[0xCD; 12]); // a, b and the byte offset
            data.extend_from_slice(&1u32.to_le_bytes()); // the kind
            data.extend_from_slice(&PACKED_KEY.to_le_bytes());
            data.extend_from_slice(&2u32.to_le_bytes()); // the type
        }
        GroupData::new(data)
    }

    fn var(name: &str, offset: u32, kind: u32) -> Variable {
        Variable {
            name: name.to_string(),
            offset,
            kind,
        }
    }

    /// The size a record type implies.
    fn size_of_kind(kind: u32) -> u32 {
        TYPE_SIZES
            .iter()
            .find(|(code, ..)| *code == kind)
            .map(|(_, size)| *size)
            .unwrap_or(0)
    }

    /// The three variables the template's own table carries.
    fn template_variables() -> Vec<Variable> {
        vec![
            var("texture_map", 0, 3),
            var("world", 16, 4),
            var("mod_tint", 80, 3),
        ]
    }

    #[test]
    fn reads_the_descriptors_of_a_group() {
        let data = template();
        assert_eq!(data.group_count(), 1);
        let descriptors = data.descriptors().expect("descriptors");
        // The descriptor names the cbuffer, not the table.
        assert_eq!(descriptors.engine.cbuffer, 0x516D5CCD);
        assert_eq!(descriptors.engine.flags, 0x101);
        assert_eq!(descriptors.object.cbuffer, 0xB3A2EB88);
        assert_eq!(descriptors.packed.cbuffer, PACKED_KEY);
    }

    #[test]
    fn finds_the_material_table_and_its_packed_copies() {
        let data = template();
        let (at, records) = data.object_table().expect("the material's own table");
        // Found by scanning, not by a descriptor: the offset is nowhere near the
        // 0 the object descriptor carries.
        assert!(at > 96, "the table sits after the engine's own, at {at}");
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].hash, var("texture_map", 0, 3).hash());
        assert_eq!(records[2].hash, var("mod_tint", 80, 3).hash());
        // Both tables are found, and the engine's is not the material's.
        let runs = data.runs();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 2);
        assert_eq!(data.packed_runs().len(), 1);
        assert_eq!(data.packed_runs()[0].1.len(), 3);
    }

    #[test]
    fn the_engine_table_is_the_one_with_the_measured_first_record() {
        let data = template();
        let engine = data.engine_records().expect("the engine table");
        assert_eq!(engine.len(), 2);
        assert_eq!(engine[0].hash, ENGINE_FIRST_HASH);
        let (_, object) = data.object_table().expect("the material table");
        assert_ne!(object[0].hash, ENGINE_FIRST_HASH);
    }

    #[test]
    fn a_run_without_its_count_word_is_not_a_table() {
        // The UI base's material table was read eight bytes into its header,
        // because the header's tail happened to read as one record. The count
        // word four bytes before a table is what rejects it.
        let data = template();
        let (at, _) = data.object_table().expect("the material table");
        let mut bytes = data.into_bytes();
        bytes[at - 4..at].copy_from_slice(&0u32.to_le_bytes());
        let without = GroupData::new(bytes);
        assert!(
            without.object_table().is_none(),
            "a table whose count word does not match is not identified"
        );
        assert!(without.engine_records().is_some(), "the engine table is still there");
    }

    #[test]
    fn a_table_that_matches_round_trips_byte_for_byte() {
        let data = template();
        let rebuilt = data.rebuild(&template_variables()).expect("rebuild");
        assert_eq!(
            rebuilt,
            data.bytes(),
            "a template whose table already matches must not change"
        );
    }

    #[test]
    fn a_renamed_variable_reaches_every_framing() {
        let data = template();
        // The same three variables, one renamed and moved to a new slot.
        let variables = [
            var("texture_map", 0, 3),
            var("world", 16, 4),
            var("mod_gradient", 96, 3),
        ];
        let rebuilt = data.rebuild(&variables).expect("rebuild");
        let read = GroupData::new(rebuilt);
        let (at, records) = read.object_table().expect("the table");
        // The canonical record carries the new name and the new offset.
        assert_eq!(records[2].hash, var("mod_gradient", 96, 3).hash());
        assert_eq!(records[2].offset, 96);
        // The packed copy carries the new name too, or the engine would read the
        // old one and the new slot would never be written.
        let packed = read.packed_runs();
        assert_eq!(packed.len(), 1);
        // The canonical table carries the new name; the packed copies are a
        // separate step, because their framing is not confirmed against a
        // shipped section and a false match would rewrite bytes that are not a
        // table.
        assert_eq!(packed[0].1[2], var("mod_tint", 80, 3).hash());
        let both = read.rebuild_packed(&variables).expect("packed");
        assert_eq!(
            GroupData::new(both).packed_runs()[0].1[2],
            var("mod_gradient", 96, 3).hash(),
            "the opt-in packed rewrite reaches the new name"
        );
        // The engine's own table is untouched.
        assert_eq!(read.runs()[0][0].hash, ENGINE_FIRST_HASH);
        assert_eq!(read.bytes()[at], read.bytes()[at]);
    }

    #[test]
    fn a_shorter_table_zeroes_its_tail() {
        let data = template();
        // One variable fewer: the records that are left must not still be there.
        let rebuilt = data.rebuild(&[var("texture_map", 0, 3)]).expect("rebuild");
        let read = GroupData::new(rebuilt);
        // The table is found by its packed copies, and holds one record.
        let (at, records) = read.object_table().expect("the table");
        assert_eq!(records.len(), 1);
        // What is left of the two dropped records is zeroed, not stale.
        assert!(
            read.bytes()[at + RECORD_LEN..at + 3 * RECORD_LEN]
                .iter()
                .all(|byte| *byte == 0)
        );
    }

    #[test]
    fn a_table_that_does_not_fit_is_reported() {
        let data = template();
        let mut variables = vec![var("texture_map", 0, 3)];
        // More variables than the template's run has room for.
        for index in 0..8 {
            variables.push(var(&format!("extra_{index}"), 128 + index * 16, 3));
        }
        let err = data.rebuild(&variables).expect_err("does not fit");
        assert!(err.to_string().contains("does not fit"), "{err}");
    }

    #[test]
    fn a_packed_rewrite_without_a_run_is_reported() {
        // The same template with the packed run cut off: header, engine header
        // and table, material header and table.
        let mut data = template().into_bytes();
        data.truncate(56 + 12 + 2 * RECORD_LEN + 12 + 3 * RECORD_LEN);
        let data = GroupData::new(data);
        assert!(data.packed_runs().is_empty());
        // The canonical table still rebuilds; the packed step says it found
        // nothing rather than writing one framing and leaving the other stale.
        assert!(data.rebuild(&template_variables()).is_ok());
        let err = data
            .rebuild_packed(&template_variables())
            .expect_err("no packed run");
        assert!(err.to_string().contains("was not found"), "{err}");
    }

    #[test]
    fn a_slot_keeps_the_size_the_engine_gave_it() {
        // A kind above 4 is the engine's own: a texture binding is kind 5 and
        // four bytes wide, and nothing in the declaration's type can say so. A
        // rewrite that recomputed the size would write a zero and the engine
        // would read a different slot.
        let mut data = template().into_bytes();
        let (at, table) = GroupData::new(data.clone())
            .object_table()
            .expect("the table");
        let kinds = [5u32, 5, 3];
        // Rebuild the table with kinds the size table has never heard of.
        let mut records = table.clone();
        for (record, kind) in records.iter_mut().zip(kinds) {
            record.kind = kind;
        }
        for (index, record) in records.iter().enumerate() {
            let at = at + index * RECORD_LEN;
            data[at..at + 4].copy_from_slice(&record.kind.to_le_bytes());
        }
        let data = GroupData::new(data);
        let variables: Vec<Variable> = data
            .object_table()
            .expect("the table")
            .1
            .iter()
            .map(|record| Variable::from_hash(record.hash, record.offset, record.kind))
            .collect();
        let rebuilt = data.rebuild(&variables).expect("rebuild");
        assert_eq!(
            rebuilt,
            data.bytes(),
            "a slot keeps the size the template gave it, kind 5 or not"
        );
    }

    /// A group data with a material table and the channel table that follows it,
    /// which is the pair the stride rule is about.
    fn with_channels() -> GroupData {
        // The 8-byte global header and the three descriptors, which a run inside
        // is not a table - the tables start after them, as they do in a section.
        let mut data = vec![0u8; 56];
        data[0..4].copy_from_slice(&1u32.to_le_bytes()); // one group
        data[4..8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        // The engine's table (its header, then two records with the measured
        // marker first), then the material's.
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(&2u32.to_le_bytes());
        for hash in [ENGINE_FIRST_HASH, var("time", 0, 3).hash()] {
            Record {
                kind: 3,
                flags: 0,
                hash,
                offset: 0,
                size: 16,
            }
            .write(&mut data);
        }
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(&2u32.to_le_bytes());
        for name in ["texture_map", "world"] {
            Record {
                kind: 3,
                flags: 0,
                hash: var(name, 0, 3).hash(),
                offset: 0,
                size: 16,
            }
            .write(&mut data);
        }
        // The channel table's 12-byte header, at the end of the material's
        // records, then its records: one channel of three - the type 5 binding
        // and two parameters.
        let header = data.len();
        data.resize(header + 12, 0);
        for (index, word) in [0x190u32, 0xFFFF_FFFF, 3].into_iter().enumerate() {
            let at = header + index * 4;
            data[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        for (kind, offset) in [(5u32, 0u32), (1, 4), (1, 16)] {
            Record {
                kind,
                flags: 0,
                hash: var("orm", offset, kind).hash(),
                offset,
                size: size_of_kind(kind),
            }
            .write(&mut data);
        }
        GroupData::new(data)
    }

    #[test]
    fn the_channel_table_is_the_one_after_the_material_variables() {
        // The stride is measured: a table's count is the word four bytes before
        // its first record, and the next table's header starts where this table's
        // records end.
        let group_data = with_channels();
        let (material_at, material) = group_data.object_table().expect("the material table");
        assert_eq!(material.len(), 2);
        let (at, records) = group_data.channel_table().expect("the channel table");
        assert_eq!(at, material_at + 2 * RECORD_LEN + 12);
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[0].kind, 5,
            "a channel leads with its texture binding"
        );

        // And a channel is the records sharing a name, not one record.
        let channels = group_data.channels();
        assert_eq!(channels.len(), 1, "three records, one channel");
        assert_eq!(channels[0].records.len(), 3);
        assert_eq!(channels[0].offset(), 0);
    }

    #[test]
    fn a_channel_table_rewrites_from_the_channels_it_was_read_as() {
        // The channel table is the one a from-scratch group data has to write, so
        // rewriting it with the channels it was read as must change nothing.
        let group_data = with_channels();
        let channels = group_data.channels();
        assert_eq!(channels.len(), 1);
        let rebuilt = group_data.rebuild_channels(&channels).expect("rewrite");
        assert_eq!(
            rebuilt,
            group_data.bytes(),
            "a channel table round trips through its own channels"
        );

        // A rename reaches every record of the channel - a texture channel's
        // binding and both of its parameters carry the name - and nothing else.
        let mut renamed = channels.clone();
        renamed[0].hash = var("bca", 0, 5).hash();
        let rebuilt = group_data.rebuild_channels(&renamed).expect("rewrite");
        let changed = group_data
            .bytes()
            .iter()
            .zip(&rebuilt)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            changed,
            3 * 4,
            "a renamed channel moves its name hash in each of its three records"
        );
    }

    #[test]
    fn a_channel_table_that_does_not_fit_is_refused() {
        // A channel the template has no slot for needs a cbuffer offset from the
        // compiled program's reflection, so asking for more than fits is an error
        // rather than a guess at where the next one goes.
        let group_data = with_channels();
        let extra = Channel {
            hash: var("noise_texture", 0, 5).hash(),
            records: vec![Record {
                kind: 5,
                flags: 0,
                hash: 0,
                offset: 32,
                size: 4,
            }],
        };
        assert!(
            group_data
                .rebuild_channels(&[extra.clone(), extra.clone(), extra.clone(), extra])
                .is_err(),
            "more channels than the table has room for is refused"
        );
    }

    #[test]
    fn a_dependency_entry_is_the_library_hash_and_nothing_else() {
        // Eight bytes: one 64-bit hash of `core/stingray_renderer/renderer`, the
        // library every shipped section names. The `{tag, name}` pair an earlier
        // reading saw in these bytes is the hash's low and high words, and the
        // group count and hash an even earlier one found past them are the group
        // data's own first two words, read through a sixteen byte window.
        let dependency = Dependency::of();
        assert_eq!(dependency.id, Dependency::RENDERER);
        let bytes = dependency.write();
        assert_eq!(bytes.len(), 8, "the entry is eight bytes");
        assert_eq!(
            bytes,
            [0xA4, 0xC3, 0xA8, 0xC0, 0xC3, 0xB8, 0x9F, 0x20],
            "stored little-endian"
        );
        assert_eq!(Dependency::parse(&bytes, 0), Some(dependency));
        assert_eq!(Dependency::read(&bytes, 0, 1), vec![dependency]);
    }
    #[test]
    fn hashes_names_the_way_the_engine_does() {
        assert_eq!(var("texture_map", 0, 3).hash(), 0xE503152C);
    }
}
