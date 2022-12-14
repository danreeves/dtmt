use std::collections::HashMap;
use std::io::Cursor;
use std::ops::{Deref, DerefMut};

use color_eyre::eyre::Context;
use color_eyre::Result;
use serde::Serialize;

use crate::binary::sync::ReadExt;
use crate::bundle::file::{BundleFileType, UserFile};
use crate::murmur::{HashGroup, Murmur64};

#[derive(Serialize)]
struct Package(HashMap<BundleFileType, Vec<String>>);

impl Deref for Package {
    type Target = HashMap<BundleFileType, Vec<String>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Package {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Package {
    fn new() -> Self {
        Self(HashMap::new())
    }
}

#[tracing::instrument(skip_all)]
pub fn decompile<B>(ctx: &crate::Context, binary: B) -> Result<Vec<UserFile>>
where
    B: AsRef<[u8]>,
{
    let mut r = Cursor::new(binary.as_ref());
    // TODO: Figure out what this is
    let unknown = r.read_u32()?;
    if unknown != 0x2b {
        tracing::warn!("Unknown u32 header. Expected 0x2b, got: {unknown:#08X} ({unknown})");
    }

    let file_count = r.read_u32()? as usize;
    let mut package = Package::new();

    for i in 0..file_count {
        let t = BundleFileType::from(r.read_u64()?);
        let hash = Murmur64::from(r.read_u64()?);
        let name = ctx.lookup_hash(hash, HashGroup::Filename);

        tracing::trace!(index = i, r"type" = ?t, %hash, name, "Package entry");

        package.entry(t).or_insert_with(Vec::new).push(name);
    }

    let s = serde_sjson::to_string(&package.0).wrap_err("failed to serialize Package to SJSON")?;
    Ok(vec![UserFile::new(s.into_bytes())])
}
