use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use bitflags::bitflags;
use color_eyre::eyre::Context;
use color_eyre::{Result, eyre};
use futures::future::join_all;

use crate::binary::sync::*;
use crate::filetype::*;
use crate::murmur::{HashGroup, IdString64, Murmur64};

use super::filetype::BundleFileType;

#[derive(Debug)]
struct BundleFileHeader {
    variant: u32,
    external: bool,
    size: usize,
    unknown_1: u8,
    len_data_file_name: usize,
}

#[derive(Clone)]
pub struct BundleFileVariant {
    property: u32,
    data: Vec<u8>,
    data_file_name: Option<String>,
    /// Declared byte length of the data file name field in the bundle. The game
    /// stores the name in a slot that can be longer than the string itself,
    /// padding it with NUL bytes. `read_string_len` strips that padding, so the
    /// original length is kept here to write the field back byte-for-byte.
    data_file_name_len: usize,
    /// Contents of the external data file referenced by `data_file_name`.
    /// Not part of the serialized bundle; used to carry streamed data files
    /// from compilation to the build/deploy step.
    external_data: Option<Vec<u8>>,
    external: bool,
    unknown_1: u8,
}

impl BundleFileVariant {
    // We will need a parameter for `property` eventually, so the `Default` impl would need to go
    // eventually anyways.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            // TODO: Hard coded for, as long as we don't support bundle properties
            property: 0,
            data: Vec::new(),
            data_file_name: None,
            data_file_name_len: 0,
            external_data: None,
            external: false,
            unknown_1: 0,
        }
    }

    /// The data file name padded with NUL bytes up to the length the bundle
    /// declares for the field. Writing only the trimmed string would shift all
    /// following file data and corrupt the bundle.
    fn data_file_name_bytes(&self) -> Vec<u8> {
        let Some(name) = &self.data_file_name else {
            return Vec::new();
        };

        let mut bytes = name.as_bytes().to_vec();
        if bytes.len() < self.data_file_name_len {
            bytes.resize(self.data_file_name_len, 0);
        }
        bytes
    }

    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn property(&self) -> u32 {
        self.property
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_file_name(&self) -> Option<&String> {
        self.data_file_name.as_ref()
    }

    /// Byte length the bundle declares for the data file name field, including
    /// any NUL padding.
    pub fn data_file_name_len(&self) -> usize {
        if self.data_file_name.is_some() {
            self.data_file_name_len
        } else {
            0
        }
    }

    /// Sets the name and contents of the external data file this variant
    /// references. The contents are written out separately from the bundle.
    pub fn set_external_data_file(&mut self, name: String, data: Vec<u8>) {
        self.data_file_name_len = name.len();
        self.data_file_name = Some(name);
        self.external_data = Some(data);
    }

    pub fn set_external(&mut self, external: bool) {
        self.external = external;
    }

    /// The contents of the external data file, if this variant carries one.
    pub fn external_data(&self) -> Option<&Vec<u8>> {
        self.external_data.as_ref()
    }

    pub fn external(&self) -> bool {
        self.external
    }

    pub fn unknown_1(&self) -> u8 {
        self.unknown_1
    }

    #[tracing::instrument(skip_all)]
    fn read_header<R>(r: &mut R) -> Result<BundleFileHeader>
    where
        R: Read + Seek,
    {
        let variant = r.read_u32()?;
        let external = r.read_bool()?;
        let size = r.read_u32()? as usize;
        let unknown_1 = r.read_u8()?;
        let len_data_file_name = r.read_u32()? as usize;

        Ok(BundleFileHeader {
            size,
            external,
            variant,
            unknown_1,
            len_data_file_name,
        })
    }

    #[tracing::instrument(skip_all)]
    fn write_header<W>(&self, w: &mut W, props: Properties) -> Result<()>
    where
        W: Write + Seek,
    {
        w.write_u32(self.property)?;
        w.write_bool(self.external)?;

        let len_data_file_name = if self.data_file_name.is_some() {
            self.data_file_name_len
        } else {
            0
        };

        if props.contains(Properties::DATA) {
            w.write_u32(len_data_file_name as u32)?;
            w.write_u8(1)?;
            w.write_u32(0)?;
        } else {
            w.write_u32(self.data.len() as u32)?;
            w.write_u8(1)?;
            w.write_u32(len_data_file_name as u32)?;
        }

        Ok(())
    }
}

impl std::fmt::Debug for BundleFileVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("BundleFileVariant");
        out.field("property", &self.property);

        if self.data.len() <= 5 {
            out.field("data", &format!("{:x?}", &self.data));
        } else {
            out.field(
                "data",
                &format!("{:x?}.. ({} bytes)", &self.data[..5], &self.data.len()),
            );
        }

        out.field("data_file_name", &self.data_file_name)
            .field("external", &self.external)
            .finish()
    }
}

bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    pub struct Properties: u32 {
        const DATA = 0b100;
        // A custom flag used by DTMT to signify a file altered by mods.
        const MODDED = 1 << 31;
    }
}

#[derive(Clone, Debug)]
pub struct BundleFile {
    file_type: BundleFileType,
    name: IdString64,
    variants: Vec<BundleFileVariant>,
    props: Properties,
}

impl BundleFile {
    pub fn new(name: impl Into<IdString64>, file_type: BundleFileType) -> Self {
        Self {
            file_type,
            name: name.into(),
            variants: Vec::new(),
            props: Properties::empty(),
        }
    }

    pub fn add_variant(&mut self, variant: BundleFileVariant) {
        self.variants.push(variant)
    }

    pub fn set_variants(&mut self, variants: Vec<BundleFileVariant>) {
        self.variants = variants;
    }

    pub fn set_props(&mut self, props: Properties) {
        self.props = props;
    }

    pub fn set_modded(&mut self, is_modded: bool) {
        self.props.set(Properties::MODDED, is_modded);
    }

    #[tracing::instrument(name = "File::read", skip(ctx, r))]
    pub fn from_reader<R>(ctx: &crate::Context, r: &mut R, props: Properties) -> Result<Self>
    where
        R: Read + Seek,
    {
        let file_type = BundleFileType::from(r.read_u64()?);
        let hash = Murmur64::from(r.read_u64()?);
        let name = ctx.lookup_hash(hash, HashGroup::Filename);

        let header_count = r.read_u32()? as usize;
        tracing::trace!(header_count);
        let mut headers = Vec::with_capacity(header_count);
        r.skip_u32(0)?;

        for i in 0..header_count {
            let span = tracing::debug_span!("Read file header", i);
            let _enter = span.enter();

            let header = BundleFileVariant::read_header(r)
                .wrap_err_with(|| format!("Failed to read header {i}"))?;

            // TODO: Figure out how `header.unknown_1` correlates to `properties::DATA`
            // if props.contains(Properties::DATA) {
            //     tracing::debug!("props: {props:?} | unknown_1: {}", header.unknown_1)
            // }

            headers.push(header);
        }

        let mut variants = Vec::with_capacity(header_count);
        for (i, header) in headers.into_iter().enumerate() {
            let span = tracing::debug_span!(
                "Read file data {}",
                i,
                size = header.size,
                len_data_file_name = header.len_data_file_name
            );
            let _enter = span.enter();

            let (data, data_file_name, data_file_name_len) = if props.contains(Properties::DATA) {
                let data = vec![];
                let s = r
                    .read_string_len(header.size)
                    .wrap_err("Failed to read data file name")?;

                (data, Some(s), header.size)
            } else {
                let mut data = vec![0; header.size];
                r.read_exact(&mut data)
                    .wrap_err_with(|| format!("Failed to read file {i}"))?;

                let data_file_name = if header.len_data_file_name > 0 {
                    let s = r
                        .read_string_len(header.len_data_file_name)
                        .wrap_err("Failed to read data file name")?;

                    Some(s)
                } else {
                    None
                };

                (data, data_file_name, header.len_data_file_name)
            };

            let variant = BundleFileVariant {
                property: header.variant,
                data,
                data_file_name,
                data_file_name_len,
                external_data: None,
                external: header.external,
                unknown_1: header.unknown_1,
            };

            variants.push(variant);
        }

        Ok(Self {
            variants,
            file_type,
            name,
            props,
        })
    }

    #[tracing::instrument(name = "File::to_binary", skip_all)]
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut w = Cursor::new(Vec::new());

        w.write_u64(self.file_type.hash().into())?;
        w.write_u64(self.name.to_murmur64().into())?;
        w.write_u32(self.variants.len() as u32)?;

        // TODO: Figure out what this is
        w.write_u32(0x0)?;

        for variant in self.variants.iter() {
            w.write_u32(variant.property())?;
            w.write_bool(variant.external)?;

            let len_data_file_name = variant.data_file_name_len();

            if self.props.contains(Properties::DATA) {
                w.write_u32(len_data_file_name as u32)?;
                w.write_u8(1)?;
                w.write_u32(0)?;
            } else {
                w.write_u32(variant.size() as u32)?;
                w.write_u8(1)?;
                w.write_u32(len_data_file_name as u32)?;
            }
        }

        for variant in self.variants.iter() {
            w.write_all(&variant.data)?;
            w.write_all(&variant.data_file_name_bytes())?;
        }

        Ok(w.into_inner())
    }

    #[tracing::instrument("File::from_sjson", skip(sjson, name), fields(name = %name.display()))]
    pub async fn from_sjson(
        name: IdString64,
        file_type: BundleFileType,
        sjson: impl AsRef<str>,
        root: impl AsRef<Path> + std::fmt::Debug,
    ) -> Result<Self> {
        match file_type {
            BundleFileType::Lua => lua::compile(name, sjson).wrap_err("Failed to compile Lua file"),
            BundleFileType::Texture => texture::compile(name, sjson, root)
                .await
                .wrap_err("Failed to compile Texture file"),
            BundleFileType::Unknown(_) => {
                eyre::bail!("Unknown file type. Cannot compile from SJSON");
            }
            _ => {
                eyre::bail!(
                    "Compiling file type {} is not yet supported",
                    file_type.ext_name()
                )
            }
        }
    }

    pub fn props(&self) -> Properties {
        self.props
    }

    pub fn base_name(&self) -> &IdString64 {
        &self.name
    }

    pub fn name(&self, decompiled: bool, variant: Option<u32>) -> String {
        let mut s = self.name.display().to_string();
        s.push('.');

        if let Some(variant) = variant {
            s.push_str(&variant.to_string());
            s.push('.');
        }

        if decompiled {
            s.push_str(&self.file_type.decompiled_ext_name());
        } else {
            s.push_str(&self.file_type.ext_name());
        }

        s
    }

    pub fn matches_name(&self, name: &IdString64) -> bool {
        if self.name == *name {
            return true;
        }

        if let IdString64::String(name) = name {
            self.name(false, None) == *name || self.name(true, None) == *name
        } else {
            false
        }
    }

    pub fn file_type(&self) -> BundleFileType {
        self.file_type
    }

    pub fn variants(&self) -> &Vec<BundleFileVariant> {
        &self.variants
    }

    pub fn variants_mut(&mut self) -> impl Iterator<Item = &mut BundleFileVariant> {
        self.variants.iter_mut()
    }

    pub fn raw(&self) -> Result<Vec<UserFile>> {
        let files = self
            .variants
            .iter()
            .map(|variant| {
                let name = if self.variants.len() > 1 {
                    self.name(false, Some(variant.property()))
                } else {
                    self.name(false, None)
                };
                UserFile {
                    data: variant.data().to_vec(),
                    name: Some(name),
                }
            })
            .collect();

        Ok(files)
    }

    #[tracing::instrument(
        name = "File::decompiled",
        skip_all,
        fields(file = self.name(false, None), file_type = self.file_type().ext_name(), variants = self.variants.len())
    )]
    pub async fn decompiled(&self, ctx: &crate::Context) -> Result<Vec<UserFile>> {
        let file_type = self.file_type();

        // The `Strings` type handles all variants combined.
        // For the other ones, each variant will be its own file.
        if file_type == BundleFileType::Strings {
            return strings::decompile(ctx, &self.variants);
        }

        let tasks = self.variants.iter().map(|variant| async move {
            let data = variant.data();
            let name = if self.variants.len() > 1 {
                self.name(true, Some(variant.property()))
            } else {
                self.name(true, None)
            };

            let res = match file_type {
                BundleFileType::Lua => lua::decompile(ctx, data).await,
                BundleFileType::Package => package::decompile(ctx, name.clone(), data),
                BundleFileType::Texture => texture::decompile(ctx, name.clone(), variant).await,
                _ => {
                    tracing::debug!("Can't decompile, unknown file type");
                    Ok(vec![UserFile::with_name(data.to_vec(), name.clone())])
                }
            };

            let res = res.wrap_err_with(|| format!("Failed to decompile file {name}"));
            match res {
                Ok(files) => files,
                Err(err) => {
                    tracing::error!("{:?}", err);
                    vec![]
                }
            }
        });

        let results = join_all(tasks).await;

        Ok(results.into_iter().flatten().collect())
    }
}

impl PartialEq for BundleFile {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.file_type == other.file_type
    }
}

pub struct UserFile {
    // TODO: Might be able to avoid some allocations with a Cow here
    data: Vec<u8>,
    name: Option<String>,
}

impl UserFile {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, name: None }
    }

    pub fn with_name(data: Vec<u8>, name: String) -> Self {
        Self {
            data,
            name: Some(name),
        }
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }
}
