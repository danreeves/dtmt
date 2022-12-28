use std::io::Cursor;
use std::sync::Arc;

use color_eyre::{Help, Result, SectionExt};
use futures::future::join_all;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncWrite, AsyncWriteExt};
use tokio::sync::RwLock;

use crate::binary::*;
use crate::context::lookup_hash;
use crate::filetype::*;
use crate::murmur::{HashGroup, Murmur64};

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub enum BundleFileType {
    Animation,
    AnimationCurves,
    Apb,
    BakedLighting,
    Bik,
    BlendSet,
    Bones,
    Chroma,
    CommonPackage,
    Config,
    Crypto,
    Data,
    Entity,
    Flow,
    Font,
    Ies,
    Ini,
    Input,
    Ivf,
    Keys,
    Level,
    Lua,
    Material,
    Mod,
    MouseCursor,
    NavData,
    NetworkConfig,
    OddleNet,
    Package,
    Particles,
    PhysicsProperties,
    RenderConfig,
    RtPipeline,
    Scene,
    Shader,
    ShaderLibrary,
    ShaderLibraryGroup,
    ShadingEnvionmentMapping,
    ShadingEnvironment,
    Slug,
    SlugAlbum,
    SoundEnvironment,
    SpuJob,
    StateMachine,
    StaticPVS,
    Strings,
    SurfaceProperties,
    Texture,
    TimpaniBank,
    TimpaniMaster,
    Tome,
    Ugg,
    Unit,
    Upb,
    VectorField,
    Wav,
    WwiseBank,
    WwiseDep,
    WwiseEvent,
    WwiseMetadata,
    WwiseStream,
    Xml,

    Unknown(Murmur64),
}

impl BundleFileType {
    pub fn ext_name(&self) -> String {
        match self {
            BundleFileType::AnimationCurves => String::from("animation_curves"),
            BundleFileType::Animation => String::from("animation"),
            BundleFileType::Apb => String::from("apb"),
            BundleFileType::BakedLighting => String::from("baked_lighting"),
            BundleFileType::Bik => String::from("bik"),
            BundleFileType::BlendSet => String::from("blend_set"),
            BundleFileType::Bones => String::from("bones"),
            BundleFileType::Chroma => String::from("chroma"),
            BundleFileType::CommonPackage => String::from("common_package"),
            BundleFileType::Config => String::from("config"),
            BundleFileType::Crypto => String::from("crypto"),
            BundleFileType::Data => String::from("data"),
            BundleFileType::Entity => String::from("entity"),
            BundleFileType::Flow => String::from("flow"),
            BundleFileType::Font => String::from("font"),
            BundleFileType::Ies => String::from("ies"),
            BundleFileType::Ini => String::from("ini"),
            BundleFileType::Input => String::from("input"),
            BundleFileType::Ivf => String::from("ivf"),
            BundleFileType::Keys => String::from("keys"),
            BundleFileType::Level => String::from("level"),
            BundleFileType::Lua => String::from("lua"),
            BundleFileType::Material => String::from("material"),
            BundleFileType::Mod => String::from("mod"),
            BundleFileType::MouseCursor => String::from("mouse_cursor"),
            BundleFileType::NavData => String::from("nav_data"),
            BundleFileType::NetworkConfig => String::from("network_config"),
            BundleFileType::OddleNet => String::from("oodle_net"),
            BundleFileType::Package => String::from("package"),
            BundleFileType::Particles => String::from("particles"),
            BundleFileType::PhysicsProperties => String::from("physics_properties"),
            BundleFileType::RenderConfig => String::from("render_config"),
            BundleFileType::RtPipeline => String::from("rt_pipeline"),
            BundleFileType::Scene => String::from("scene"),
            BundleFileType::ShaderLibraryGroup => String::from("shader_library_group"),
            BundleFileType::ShaderLibrary => String::from("shader_library"),
            BundleFileType::Shader => String::from("shader"),
            BundleFileType::ShadingEnvionmentMapping => String::from("shading_environment_mapping"),
            BundleFileType::ShadingEnvironment => String::from("shading_environment"),
            BundleFileType::SlugAlbum => String::from("slug_album"),
            BundleFileType::Slug => String::from("slug"),
            BundleFileType::SoundEnvironment => String::from("sound_environment"),
            BundleFileType::SpuJob => String::from("spu_job"),
            BundleFileType::StateMachine => String::from("state_machine"),
            BundleFileType::StaticPVS => String::from("static_pvs"),
            BundleFileType::Strings => String::from("strings"),
            BundleFileType::SurfaceProperties => String::from("surface_properties"),
            BundleFileType::Texture => String::from("texture"),
            BundleFileType::TimpaniBank => String::from("timpani_bank"),
            BundleFileType::TimpaniMaster => String::from("timpani_master"),
            BundleFileType::Tome => String::from("tome"),
            BundleFileType::Ugg => String::from("ugg"),
            BundleFileType::Unit => String::from("unit"),
            BundleFileType::Upb => String::from("upb"),
            BundleFileType::VectorField => String::from("vector_field"),
            BundleFileType::Wav => String::from("wav"),
            BundleFileType::WwiseBank => String::from("wwise_bank"),
            BundleFileType::WwiseDep => String::from("wwise_dep"),
            BundleFileType::WwiseEvent => String::from("wwise_event"),
            BundleFileType::WwiseMetadata => String::from("wwise_metadata"),
            BundleFileType::WwiseStream => String::from("wwise_stream"),
            BundleFileType::Xml => String::from("xml"),

            BundleFileType::Unknown(s) => format!("{s:016X}"),
        }
    }

    pub fn decompiled_ext_name(&self) -> String {
        match self {
            BundleFileType::Texture => String::from("dds"),
            BundleFileType::WwiseBank => String::from("bnk"),
            BundleFileType::WwiseStream => String::from("ogg"),
            _ => self.ext_name(),
        }
    }

    pub fn hash(&self) -> Murmur64 {
        Murmur64::from(*self)
    }
}

impl Serialize for BundleFileType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.ext_name();
        value.serialize(serializer)
    }
}

impl From<u64> for BundleFileType {
    fn from(value: u64) -> Self {
        Self::from(Murmur64::from(value))
    }
}

impl From<Murmur64> for BundleFileType {
    fn from(hash: Murmur64) -> BundleFileType {
        match *hash {
            0x931e336d7646cc26 => BundleFileType::Animation,
            0xdcfb9e18fff13984 => BundleFileType::AnimationCurves,
            0x3eed05ba83af5090 => BundleFileType::Apb,
            0x7ffdb779b04e4ed1 => BundleFileType::BakedLighting,
            0xaa5965f03029fa18 => BundleFileType::Bik,
            0xe301e8af94e3b5a3 => BundleFileType::BlendSet,
            0x18dead01056b72e9 => BundleFileType::Bones,
            0xb7893adf7567506a => BundleFileType::Chroma,
            0xfe9754bd19814a47 => BundleFileType::CommonPackage,
            0x82645835e6b73232 => BundleFileType::Config,
            0x69108ded1e3e634b => BundleFileType::Crypto,
            0x8fd0d44d20650b68 => BundleFileType::Data,
            0x9831ca893b0d087d => BundleFileType::Entity,
            0x92d3ee038eeb610d => BundleFileType::Flow,
            0x9efe0a916aae7880 => BundleFileType::Font,
            0x8f7d5a2c0f967655 => BundleFileType::Ies,
            0xd526a27da14f1dc5 => BundleFileType::Ini,
            0x2bbcabe5074ade9e => BundleFileType::Input,
            0xfa4a8e091a91201e => BundleFileType::Ivf,
            0xa62f9297dc969e85 => BundleFileType::Keys,
            0x2a690fd348fe9ac5 => BundleFileType::Level,
            0xa14e8dfa2cd117e2 => BundleFileType::Lua,
            0xeac0b497876adedf => BundleFileType::Material,
            0x3fcdd69156a46417 => BundleFileType::Mod,
            0xb277b11fe4a61d37 => BundleFileType::MouseCursor,
            0x169de9566953d264 => BundleFileType::NavData,
            0x3b1fa9e8f6bac374 => BundleFileType::NetworkConfig,
            0xb0f2c12eb107f4d8 => BundleFileType::OddleNet,
            0xad9c6d9ed1e5e77a => BundleFileType::Package,
            0xa8193123526fad64 => BundleFileType::Particles,
            0xbf21403a3ab0bbb1 => BundleFileType::PhysicsProperties,
            0x27862fe24795319c => BundleFileType::RenderConfig,
            0x9ca183c2d0e76dee => BundleFileType::RtPipeline,
            0x9d0a795bfe818d19 => BundleFileType::Scene,
            0xcce8d5b5f5ae333f => BundleFileType::Shader,
            0xe5ee32a477239a93 => BundleFileType::ShaderLibrary,
            0x9e5c3cc74575aeb5 => BundleFileType::ShaderLibraryGroup,
            0x250e0a11ac8e26f8 => BundleFileType::ShadingEnvionmentMapping,
            0xfe73c7dcff8a7ca5 => BundleFileType::ShadingEnvironment,
            0xa27b4d04a9ba6f9e => BundleFileType::Slug,
            0xe9fc9ea7042e5ec0 => BundleFileType::SlugAlbum,
            0xd8b27864a97ffdd7 => BundleFileType::SoundEnvironment,
            0xf97af9983c05b950 => BundleFileType::SpuJob,
            0xa486d4045106165c => BundleFileType::StateMachine,
            0xe3f0baa17d620321 => BundleFileType::StaticPVS,
            0x0d972bab10b40fd3 => BundleFileType::Strings,
            0xad2d3fa30d9ab394 => BundleFileType::SurfaceProperties,
            0xcd4238c6a0c69e32 => BundleFileType::Texture,
            0x99736be1fff739a4 => BundleFileType::TimpaniBank,
            0x00a3e6c59a2b9c6c => BundleFileType::TimpaniMaster,
            0x19c792357c99f49b => BundleFileType::Tome,
            0x712d6e3dd1024c9c => BundleFileType::Ugg,
            0xe0a48d0be9a7453f => BundleFileType::Unit,
            0xa99510c6e86dd3c2 => BundleFileType::Upb,
            0xf7505933166d6755 => BundleFileType::VectorField,
            0x786f65c00a816b19 => BundleFileType::Wav,
            0x535a7bd3e650d799 => BundleFileType::WwiseBank,
            0xaf32095c82f2b070 => BundleFileType::WwiseDep,
            0xaabdd317b58dfc8a => BundleFileType::WwiseEvent,
            0xd50a8b7e1c82b110 => BundleFileType::WwiseMetadata,
            0x504b55235d21440e => BundleFileType::WwiseStream,
            0x76015845a6003765 => BundleFileType::Xml,

            _ => BundleFileType::Unknown(hash),
        }
    }
}

impl From<BundleFileType> for Murmur64 {
    fn from(t: BundleFileType) -> Murmur64 {
        match t {
            BundleFileType::Animation => Murmur64::from(0x931e336d7646cc26),
            BundleFileType::AnimationCurves => Murmur64::from(0xdcfb9e18fff13984),
            BundleFileType::Apb => Murmur64::from(0x3eed05ba83af5090),
            BundleFileType::BakedLighting => Murmur64::from(0x7ffdb779b04e4ed1),
            BundleFileType::Bik => Murmur64::from(0xaa5965f03029fa18),
            BundleFileType::BlendSet => Murmur64::from(0xe301e8af94e3b5a3),
            BundleFileType::Bones => Murmur64::from(0x18dead01056b72e9),
            BundleFileType::Chroma => Murmur64::from(0xb7893adf7567506a),
            BundleFileType::CommonPackage => Murmur64::from(0xfe9754bd19814a47),
            BundleFileType::Config => Murmur64::from(0x82645835e6b73232),
            BundleFileType::Crypto => Murmur64::from(0x69108ded1e3e634b),
            BundleFileType::Data => Murmur64::from(0x8fd0d44d20650b68),
            BundleFileType::Entity => Murmur64::from(0x9831ca893b0d087d),
            BundleFileType::Flow => Murmur64::from(0x92d3ee038eeb610d),
            BundleFileType::Font => Murmur64::from(0x9efe0a916aae7880),
            BundleFileType::Ies => Murmur64::from(0x8f7d5a2c0f967655),
            BundleFileType::Ini => Murmur64::from(0xd526a27da14f1dc5),
            BundleFileType::Input => Murmur64::from(0x2bbcabe5074ade9e),
            BundleFileType::Ivf => Murmur64::from(0xfa4a8e091a91201e),
            BundleFileType::Keys => Murmur64::from(0xa62f9297dc969e85),
            BundleFileType::Level => Murmur64::from(0x2a690fd348fe9ac5),
            BundleFileType::Lua => Murmur64::from(0xa14e8dfa2cd117e2),
            BundleFileType::Material => Murmur64::from(0xeac0b497876adedf),
            BundleFileType::Mod => Murmur64::from(0x3fcdd69156a46417),
            BundleFileType::MouseCursor => Murmur64::from(0xb277b11fe4a61d37),
            BundleFileType::NavData => Murmur64::from(0x169de9566953d264),
            BundleFileType::NetworkConfig => Murmur64::from(0x3b1fa9e8f6bac374),
            BundleFileType::OddleNet => Murmur64::from(0xb0f2c12eb107f4d8),
            BundleFileType::Package => Murmur64::from(0xad9c6d9ed1e5e77a),
            BundleFileType::Particles => Murmur64::from(0xa8193123526fad64),
            BundleFileType::PhysicsProperties => Murmur64::from(0xbf21403a3ab0bbb1),
            BundleFileType::RenderConfig => Murmur64::from(0x27862fe24795319c),
            BundleFileType::RtPipeline => Murmur64::from(0x9ca183c2d0e76dee),
            BundleFileType::Scene => Murmur64::from(0x9d0a795bfe818d19),
            BundleFileType::Shader => Murmur64::from(0xcce8d5b5f5ae333f),
            BundleFileType::ShaderLibrary => Murmur64::from(0xe5ee32a477239a93),
            BundleFileType::ShaderLibraryGroup => Murmur64::from(0x9e5c3cc74575aeb5),
            BundleFileType::ShadingEnvionmentMapping => Murmur64::from(0x250e0a11ac8e26f8),
            BundleFileType::ShadingEnvironment => Murmur64::from(0xfe73c7dcff8a7ca5),
            BundleFileType::Slug => Murmur64::from(0xa27b4d04a9ba6f9e),
            BundleFileType::SlugAlbum => Murmur64::from(0xe9fc9ea7042e5ec0),
            BundleFileType::SoundEnvironment => Murmur64::from(0xd8b27864a97ffdd7),
            BundleFileType::SpuJob => Murmur64::from(0xf97af9983c05b950),
            BundleFileType::StateMachine => Murmur64::from(0xa486d4045106165c),
            BundleFileType::StaticPVS => Murmur64::from(0xe3f0baa17d620321),
            BundleFileType::Strings => Murmur64::from(0x0d972bab10b40fd3),
            BundleFileType::SurfaceProperties => Murmur64::from(0xad2d3fa30d9ab394),
            BundleFileType::Texture => Murmur64::from(0xcd4238c6a0c69e32),
            BundleFileType::TimpaniBank => Murmur64::from(0x99736be1fff739a4),
            BundleFileType::TimpaniMaster => Murmur64::from(0x00a3e6c59a2b9c6c),
            BundleFileType::Tome => Murmur64::from(0x19c792357c99f49b),
            BundleFileType::Ugg => Murmur64::from(0x712d6e3dd1024c9c),
            BundleFileType::Unit => Murmur64::from(0xe0a48d0be9a7453f),
            BundleFileType::Upb => Murmur64::from(0xa99510c6e86dd3c2),
            BundleFileType::VectorField => Murmur64::from(0xf7505933166d6755),
            BundleFileType::Wav => Murmur64::from(0x786f65c00a816b19),
            BundleFileType::WwiseBank => Murmur64::from(0x535a7bd3e650d799),
            BundleFileType::WwiseDep => Murmur64::from(0xaf32095c82f2b070),
            BundleFileType::WwiseEvent => Murmur64::from(0xaabdd317b58dfc8a),
            BundleFileType::WwiseMetadata => Murmur64::from(0xd50a8b7e1c82b110),
            BundleFileType::WwiseStream => Murmur64::from(0x504b55235d21440e),
            BundleFileType::Xml => Murmur64::from(0x76015845a6003765),

            BundleFileType::Unknown(hash) => hash,
        }
    }
}

struct BundleFileHeader {
    variant: u32,
    size: usize,
    len_data_file_name: usize,
}

impl BundleFileHeader {
    #[tracing::instrument(name = "FileHeader::read", skip_all)]
    async fn read<R>(r: &mut R) -> Result<Self>
    where
        R: AsyncRead + AsyncSeek + std::marker::Unpin,
    {
        let variant = read_u32(r).await?;
        skip_u8(r, 0).await?;
        let size = read_u32(r).await? as usize;
        skip_u8(r, 1).await?;
        let len_data_file_name = read_u32(r).await? as usize;

        Ok(Self {
            size,
            variant,
            len_data_file_name,
        })
    }
}

pub struct BundleFileVariant {
    header: BundleFileHeader,
    data: Vec<u8>,
    data_file_name: String,
}

impl BundleFileVariant {
    pub fn size(&self) -> usize {
        self.header.size
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn set_data(&mut self, data: Vec<u8>) {
        self.header.size = data.len();
        self.data = data;
    }
}

pub struct BundleFile {
    file_type: BundleFileType,
    hash: Murmur64,
    name: String,
    variants: Vec<BundleFileVariant>,
}

impl BundleFile {
    #[tracing::instrument(name = "File::read", skip_all)]
    pub async fn read<R>(ctx: Arc<RwLock<crate::Context>>, r: &mut R) -> Result<Self>
    where
        R: AsyncRead + AsyncSeek + std::marker::Unpin,
    {
        let file_type = BundleFileType::from(read_u64(r).await?);
        let hash = Murmur64::from(read_u64(r).await?);
        let name = lookup_hash(ctx, hash, HashGroup::Filename).await;

        let header_count = read_u32(r)
            .await
            .with_section(|| format!("{}.{}", name, file_type.ext_name()).header("File:"))?;
        let header_count = header_count as usize;

        let mut headers = Vec::with_capacity(header_count);
        skip_u32(r, 0).await?;

        for _ in 0..header_count {
            let header = BundleFileHeader::read(r)
                .await
                .with_section(|| format!("{}.{}", name, file_type.ext_name()).header("File:"))?;
            headers.push(header);
        }

        let mut variants = Vec::with_capacity(header_count);

        for header in headers.into_iter() {
            let mut data = vec![0; header.size];
            r.read_exact(&mut data).await?;

            let data_file_name = {
                let mut buf = vec![0; header.len_data_file_name];
                r.read_exact(&mut buf).await?;

                String::from_utf8(buf)?
            };

            let variant = BundleFileVariant {
                header,
                data,
                data_file_name,
            };

            variants.push(variant);
        }

        Ok(Self {
            variants,
            file_type,
            hash,
            name,
        })
    }

    #[tracing::instrument(name = "File::write", skip_all)]
    pub async fn write<W>(&self, _ctx: Arc<RwLock<crate::Context>>, w: &mut W) -> Result<()>
    where
        W: AsyncWrite + AsyncSeek + std::marker::Unpin,
    {
        write_u64(w, *self.file_type.hash()).await?;
        write_u64(w, *self.hash).await?;

        let header_count = self.variants.len();
        write_u32(w, header_count as u32).await?;
        // TODO: Unknown what this is
        write_u32(w, 0).await?;

        for variant in self.variants.iter() {
            // TODO: Unknown what these are
            write_u32(w, variant.header.variant).await?;
            // TODO: Unknown what this is
            write_u8(w, 0).await?;
            write_u32(w, variant.data.len() as u32).await?;
            // TODO: Unknown what this is
            write_u8(w, 1).await?;
            // TODO: The previous size value and this one are somehow connected,
            // but so far it is unknown how
            write_u32(w, variant.data_file_name.len() as u32).await?;
        }

        for variant in self.variants.iter() {
            w.write_all(&variant.data).await?;
            w.write_all(variant.data_file_name.as_bytes()).await?;
        }

        Ok(())
    }

    pub fn base_name(&self) -> &String {
        &self.name
    }

    pub fn name(&self, decompiled: bool, variant: Option<u32>) -> String {
        let mut s = self.name.clone();
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

    pub fn matches_name<S>(&self, name: S) -> bool
    where
        S: AsRef<str>,
    {
        let name = name.as_ref();
        self.name == name || self.name(false, None) == name || self.name(true, None) == name
    }

    pub fn hash(&self) -> Murmur64 {
        self.hash
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
                    self.name(false, Some(variant.header.variant))
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

    #[tracing::instrument(name = "File::decompiled", skip_all)]
    pub async fn decompiled(&self, ctx: Arc<RwLock<crate::Context>>) -> Result<Vec<UserFile>> {
        let file_type = self.file_type();

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                name = self.name(true, None),
                variants = self.variants.len(),
                "Attempting to decompile"
            );
        }

        let tasks = self.variants.iter().map(|variant| {
            let ctx = ctx.clone();

            async move {
                let data = variant.data();
                let name = if self.variants.len() > 1 {
                    self.name(true, Some(variant.header.variant))
                } else {
                    self.name(true, None)
                };

                let res = match file_type {
                    BundleFileType::Lua => lua::decompile(ctx, data).await,
                    BundleFileType::Package => {
                        let mut c = Cursor::new(data);
                        package::decompile(ctx, &mut c).await
                    }
                    _ => {
                        tracing::debug!("Can't decompile, unknown file type");
                        Ok(vec![UserFile::with_name(data.to_vec(), name.clone())])
                    }
                };

                match res {
                    Ok(files) => files,
                    Err(err) => {
                        let err = err
                            .wrap_err("failed to decompile file")
                            .with_section(|| name.header("File:"));
                        tracing::error!("{:?}", err);
                        vec![]
                    }
                }
            }
        });

        let results = join_all(tasks).await;

        Ok(results.into_iter().flatten().collect())
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
