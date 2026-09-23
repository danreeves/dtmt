use color_eyre::{Help, Result, SectionExt, eyre::Context};
use csv_async::{AsyncDeserializer, AsyncSerializer};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_stream::StreamExt;

use super::{Murmur32, Murmur64, SEED, murmurhash64};

#[derive(Copy, Clone, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum HashGroup {
    Filename,
    Filetype,
    Strings,
    TextureCategory,
    #[default]
    Other,
}

impl HashGroup {
    pub fn all() -> [Self; 5] {
        [
            Self::Filename,
            Self::Filetype,
            Self::Strings,
            Self::TextureCategory,
            Self::Other,
        ]
    }
}

impl std::fmt::Display for HashGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashGroup::Filename => write!(f, "filename"),
            HashGroup::Filetype => write!(f, "filetype"),
            HashGroup::Strings => write!(f, "strings"),
            HashGroup::TextureCategory => write!(f, "texture-category"),
            HashGroup::Other => write!(f, "other"),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Row {
    // NOTE: The order of fields is important, as the CSV serializer copies that.
    value: String,
    long: Option<Murmur64>,
    short: Option<Murmur32>,
    #[serde(default)]
    group: HashGroup,
}

#[derive(Clone)]
pub struct Entry {
    value: String,
    long: Murmur64,
    short: Murmur32,
    group: HashGroup,
}

impl Entry {
    pub fn value(&self) -> &String {
        &self.value
    }

    pub fn long(&self) -> Murmur64 {
        self.long
    }

    pub fn short(&self) -> Murmur32 {
        self.short
    }

    pub fn group(&self) -> HashGroup {
        self.group
    }
}

#[derive(Clone)]
pub struct Dictionary {
    entries: Vec<Entry>,
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary {
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    pub async fn from_csv<R>(r: R) -> Result<Self>
    where
        R: AsyncRead + std::marker::Unpin + std::marker::Send,
    {
        let mut entries = vec![];

        let r = AsyncDeserializer::from_reader(r);
        let mut records = r.into_deserialize::<Row>();

        while let Some(row) = records.next().await {
            let record = row?;
            let value = record.value;
            let long = record.long.unwrap_or_else(|| {
                Murmur64::from(murmurhash64::hash(value.as_bytes(), SEED as u64))
            });
            let short = record
                .short
                .unwrap_or_else(|| Murmur32::from(murmurhash64::hash32(value.as_bytes(), SEED)));

            let entry = Entry {
                value,
                long,
                short,
                group: record.group,
            };

            entries.push(entry);
        }

        Ok(Self { entries })
    }

    pub async fn to_csv<W>(&self, w: W) -> Result<()>
    where
        W: AsyncWrite + std::marker::Unpin,
    {
        let mut w = AsyncSerializer::from_writer(w);
        for (i, entry) in self.entries.iter().enumerate() {
            let row = Row {
                long: Some(entry.long),
                short: Some(entry.short),
                value: entry.value.clone(),
                group: entry.group,
            };

            w.serialize(row)
                .await
                .wrap_err("Failed to serialize entry")
                .with_section(|| {
                    let s = format!(
                        "Index: {}\nValue: {}\n64bit: {:#016X}\n32bit: {:#08X}\nGroup: {}",
                        i, entry.value, entry.long, entry.short, entry.group
                    );

                    s.header("Entry")
                })?;
        }

        Ok(())
    }

    pub fn add(&mut self, value: impl AsRef<[u8]>, group: HashGroup) {
        let long = Murmur64::from(murmurhash64::hash(value.as_ref(), SEED as u64));
        let short = Murmur32::from(murmurhash64::hash32(value.as_ref(), SEED));

        let entry = Entry {
            long,
            short,
            value: String::from_utf8_lossy(value.as_ref()).to_string(),
            group,
        };

        self.entries.push(entry);
    }

    pub fn find(&self, value: &String, group: HashGroup) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.value == *value && e.group == group)
    }

    pub fn lookup(&self, hash: Murmur64, group: HashGroup) -> Option<&String> {
        self.entries
            .iter()
            .filter(|e| e.group == group)
            .find(|e| e.long == hash)
            .map(|e| &e.value)
    }

    pub fn lookup_short(&self, hash: Murmur32, group: HashGroup) -> Option<&String> {
        self.entries
            .iter()
            .filter(|e| e.group == group)
            .find(|e| e.short == hash)
            .map(|e| &e.value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &Vec<Entry> {
        &self.entries
    }
}
