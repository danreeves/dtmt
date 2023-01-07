use std::collections::HashMap;
use std::io::{Cursor, Read};

use color_eyre::{Report, Result};

use crate::binary::sync::ReadExt;
use crate::bundle::file::{BundleFileVariant, UserFile};
use crate::murmur::HashGroup;

#[derive(Copy, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(untagged)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(serialize_with = "Language::serialize_unnamed")]
    Unnamed(u32),
}

impl Language {
    fn serialize_unnamed<S>(field: &u32, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ser.serialize_str(&format!("lang_{field}"))
    }
}

#[derive(serde::Serialize)]
pub struct Strings(HashMap<String, HashMap<Language, String>>);

fn read_string<R>(r: R) -> Result<String>
where
    R: Read,
{
    r.bytes()
        .take_while(|b| b.as_ref().map(|b| *b != 0).unwrap_or(false))
        .map(|b| b.map_err(Report::new))
        .collect::<Result<_>>()
        .and_then(|bytes| String::from_utf8(bytes).map_err(Report::new))
}

impl Strings {
    #[tracing::instrument(skip_all, fields(languages = variants.len()))]
    pub fn from_variants(ctx: &crate::Context, variants: &Vec<BundleFileVariant>) -> Result<Self> {
        let mut map: HashMap<String, HashMap<Language, String>> = HashMap::new();

        for (i, variant) in variants.iter().enumerate() {
            let _span = tracing::trace_span!("variant {}", i);
            let mut r = Cursor::new(variant.data());
            let _header = r.read_u32()?;
            let count = r.read_u32()? as usize;

            for _ in 0..count {
                let name = ctx.lookup_hash_short(r.read_u32()?, HashGroup::Strings);
                let address = r.read_u32()? as u64;

                let pos = r.position();

                r.set_position(address);
                let s = read_string(&mut r)?;
                r.set_position(pos);

                map.entry(name)
                    .or_default()
                    .insert(Language::Unnamed(variant.property()), s);
            }
        }

        Ok(Self(map))
    }

    #[tracing::instrument(skip_all)]
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(&self.0).map_err(Report::new)
    }
}

#[tracing::instrument(skip_all)]
pub fn decompile(ctx: &crate::Context, variants: &Vec<BundleFileVariant>) -> Result<Vec<UserFile>> {
    let strings = Strings::from_variants(ctx, variants)?;
    let content = strings.to_sjson()?;

    Ok(vec![UserFile::new(content.into_bytes())])
}
