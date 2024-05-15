use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use bitflags::bitflags;
use color_eyre::eyre::Context;
use color_eyre::{eyre, Result};
use futures::future::join_all;
use serde::Serialize;

use crate::binary::sync::*;
use crate::filetype::*;
use crate::murmur::{HashGroup, IdString64, Murmur64};

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

impl std::str::FromStr for BundleFileType {
    type Err = color_eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let val = match s {
            "animation_curves" => BundleFileType::AnimationCurves,
            "animation" => BundleFileType::Animation,
            "apb" => BundleFileType::Apb,
            "baked_lighting" => BundleFileType::BakedLighting,
            "bik" => BundleFileType::Bik,
            "blend_set" => BundleFileType::BlendSet,
            "bones" => BundleFileType::Bones,
            "chroma" => BundleFileType::Chroma,
            "common_package" => BundleFileType::CommonPackage,
            "config" => BundleFileType::Config,
            "crypto" => BundleFileType::Crypto,
            "data" => BundleFileType::Data,
            "entity" => BundleFileType::Entity,
            "flow" => BundleFileType::Flow,
            "font" => BundleFileType::Font,
            "ies" => BundleFileType::Ies,
            "ini" => BundleFileType::Ini,
            "input" => BundleFileType::Input,
            "ivf" => BundleFileType::Ivf,
            "keys" => BundleFileType::Keys,
            "level" => BundleFileType::Level,
            "lua" => BundleFileType::Lua,
            "material" => BundleFileType::Material,
            "mod" => BundleFileType::Mod,
            "mouse_cursor" => BundleFileType::MouseCursor,
            "nav_data" => BundleFileType::NavData,
            "network_config" => BundleFileType::NetworkConfig,
            "oodle_net" => BundleFileType::OddleNet,
            "package" => BundleFileType::Package,
            "particles" => BundleFileType::Particles,
            "physics_properties" => BundleFileType::PhysicsProperties,
            "render_config" => BundleFileType::RenderConfig,
            "rt_pipeline" => BundleFileType::RtPipeline,
            "scene" => BundleFileType::Scene,
            "shader_library_group" => BundleFileType::ShaderLibraryGroup,
            "shader_library" => BundleFileType::ShaderLibrary,
            "shader" => BundleFileType::Shader,
            "shading_environment_mapping" => BundleFileType::ShadingEnvionmentMapping,
            "shading_environment" => BundleFileType::ShadingEnvironment,
            "slug_album" => BundleFileType::SlugAlbum,
            "slug" => BundleFileType::Slug,
            "sound_environment" => BundleFileType::SoundEnvironment,
            "spu_job" => BundleFileType::SpuJob,
            "state_machine" => BundleFileType::StateMachine,
            "static_pvs" => BundleFileType::StaticPVS,
            "strings" => BundleFileType::Strings,
            "surface_properties" => BundleFileType::SurfaceProperties,
            "texture" => BundleFileType::Texture,
            "timpani_bank" => BundleFileType::TimpaniBank,
            "timpani_master" => BundleFileType::TimpaniMaster,
            "tome" => BundleFileType::Tome,
            "ugg" => BundleFileType::Ugg,
            "unit" => BundleFileType::Unit,
            "upb" => BundleFileType::Upb,
            "vector_field" => BundleFileType::VectorField,
            "wav" => BundleFileType::Wav,
            "wwise_bank" => BundleFileType::WwiseBank,
            "wwise_dep" => BundleFileType::WwiseDep,
            "wwise_event" => BundleFileType::WwiseEvent,
            "wwise_metadata" => BundleFileType::WwiseMetadata,
            "wwise_stream" => BundleFileType::WwiseStream,
            "xml" => BundleFileType::Xml,
            s => eyre::bail!("Unknown type string '{}'", s),
        };

        Ok(val)
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

impl From<Murmur64> for BundleFileType {
    fn from(value: Murmur64) -> Self {
        Self::from(Into::<u64>::into(value))
    }
}

impl From<u64> for BundleFileType {
    fn from(hash: u64) -> BundleFileType {
        match hash {
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

            _ => BundleFileType::Unknown(Murmur64::from(hash)),
        }
    }
}

impl From<BundleFileType> for u64 {
    fn from(t: BundleFileType) -> u64 {
        match t {
            BundleFileType::Animation => 0x931e336d7646cc26,
            BundleFileType::AnimationCurves => 0xdcfb9e18fff13984,
            BundleFileType::Apb => 0x3eed05ba83af5090,
            BundleFileType::BakedLighting => 0x7ffdb779b04e4ed1,
            BundleFileType::Bik => 0xaa5965f03029fa18,
            BundleFileType::BlendSet => 0xe301e8af94e3b5a3,
            BundleFileType::Bones => 0x18dead01056b72e9,
            BundleFileType::Chroma => 0xb7893adf7567506a,
            BundleFileType::CommonPackage => 0xfe9754bd19814a47,
            BundleFileType::Config => 0x82645835e6b73232,
            BundleFileType::Crypto => 0x69108ded1e3e634b,
            BundleFileType::Data => 0x8fd0d44d20650b68,
            BundleFileType::Entity => 0x9831ca893b0d087d,
            BundleFileType::Flow => 0x92d3ee038eeb610d,
            BundleFileType::Font => 0x9efe0a916aae7880,
            BundleFileType::Ies => 0x8f7d5a2c0f967655,
            BundleFileType::Ini => 0xd526a27da14f1dc5,
            BundleFileType::Input => 0x2bbcabe5074ade9e,
            BundleFileType::Ivf => 0xfa4a8e091a91201e,
            BundleFileType::Keys => 0xa62f9297dc969e85,
            BundleFileType::Level => 0x2a690fd348fe9ac5,
            BundleFileType::Lua => 0xa14e8dfa2cd117e2,
            BundleFileType::Material => 0xeac0b497876adedf,
            BundleFileType::Mod => 0x3fcdd69156a46417,
            BundleFileType::MouseCursor => 0xb277b11fe4a61d37,
            BundleFileType::NavData => 0x169de9566953d264,
            BundleFileType::NetworkConfig => 0x3b1fa9e8f6bac374,
            BundleFileType::OddleNet => 0xb0f2c12eb107f4d8,
            BundleFileType::Package => 0xad9c6d9ed1e5e77a,
            BundleFileType::Particles => 0xa8193123526fad64,
            BundleFileType::PhysicsProperties => 0xbf21403a3ab0bbb1,
            BundleFileType::RenderConfig => 0x27862fe24795319c,
            BundleFileType::RtPipeline => 0x9ca183c2d0e76dee,
            BundleFileType::Scene => 0x9d0a795bfe818d19,
            BundleFileType::Shader => 0xcce8d5b5f5ae333f,
            BundleFileType::ShaderLibrary => 0xe5ee32a477239a93,
            BundleFileType::ShaderLibraryGroup => 0x9e5c3cc74575aeb5,
            BundleFileType::ShadingEnvionmentMapping => 0x250e0a11ac8e26f8,
            BundleFileType::ShadingEnvironment => 0xfe73c7dcff8a7ca5,
            BundleFileType::Slug => 0xa27b4d04a9ba6f9e,
            BundleFileType::SlugAlbum => 0xe9fc9ea7042e5ec0,
            BundleFileType::SoundEnvironment => 0xd8b27864a97ffdd7,
            BundleFileType::SpuJob => 0xf97af9983c05b950,
            BundleFileType::StateMachine => 0xa486d4045106165c,
            BundleFileType::StaticPVS => 0xe3f0baa17d620321,
            BundleFileType::Strings => 0x0d972bab10b40fd3,
            BundleFileType::SurfaceProperties => 0xad2d3fa30d9ab394,
            BundleFileType::Texture => 0xcd4238c6a0c69e32,
            BundleFileType::TimpaniBank => 0x99736be1fff739a4,
            BundleFileType::TimpaniMaster => 0x00a3e6c59a2b9c6c,
            BundleFileType::Tome => 0x19c792357c99f49b,
            BundleFileType::Ugg => 0x712d6e3dd1024c9c,
            BundleFileType::Unit => 0xe0a48d0be9a7453f,
            BundleFileType::Upb => 0xa99510c6e86dd3c2,
            BundleFileType::VectorField => 0xf7505933166d6755,
            BundleFileType::Wav => 0x786f65c00a816b19,
            BundleFileType::WwiseBank => 0x535a7bd3e650d799,
            BundleFileType::WwiseDep => 0xaf32095c82f2b070,
            BundleFileType::WwiseEvent => 0xaabdd317b58dfc8a,
            BundleFileType::WwiseMetadata => 0xd50a8b7e1c82b110,
            BundleFileType::WwiseStream => 0x504b55235d21440e,
            BundleFileType::Xml => 0x76015845a6003765,

            BundleFileType::Unknown(hash) => hash.into(),
        }
    }
}
impl From<BundleFileType> for Murmur64 {
    fn from(t: BundleFileType) -> Murmur64 {
        let hash: u64 = t.into();
        Murmur64::from(hash)
    }
}

impl std::fmt::Display for BundleFileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ext_name())
    }
}

#[derive(Debug)]
struct BundleFileHeader {
    variant: u32,
    unknown_1: u8,
    size: usize,
    len_data_file_name: usize,
}

pub struct BundleFileVariant {
    property: u32,
    data: Vec<u8>,
    data_file_name: Option<String>,
    // Seems to be related to whether there is a data path.
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
            unknown_1: 0,
        }
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

    #[tracing::instrument(skip_all)]
    fn read_header<R>(r: &mut R) -> Result<BundleFileHeader>
    where
        R: Read + Seek,
    {
        let variant = r.read_u32()?;
        let unknown_1 = r.read_u8()?;
        let size = r.read_u32()? as usize;
        r.skip_u8(1)?;
        let len_data_file_name = r.read_u32()? as usize;

        Ok(BundleFileHeader {
            size,
            unknown_1,
            variant,
            len_data_file_name,
        })
    }

    #[tracing::instrument(skip_all)]
    fn write_header<W>(&self, w: &mut W, props: Properties) -> Result<()>
    where
        W: Write + Seek,
    {
        w.write_u32(self.property)?;
        w.write_u8(self.unknown_1)?;

        let len_data_file_name = self.data_file_name.as_ref().map(|s| s.len()).unwrap_or(0);

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

bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    pub struct Properties: u32 {
        const DATA = 0b100;
    }
}

pub struct BundleFile {
    file_type: BundleFileType,
    name: IdString64,
    variants: Vec<BundleFileVariant>,
    props: Properties,
}

impl BundleFile {
    pub fn new(name: String, file_type: BundleFileType) -> Self {
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

            let (data, data_file_name) = if props.contains(Properties::DATA) {
                let data = vec![];
                let s = r
                    .read_string_len(header.size)
                    .wrap_err("Failed to read data file name")?;

                (data, Some(s))
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

                (data, data_file_name)
            };

            let variant = BundleFileVariant {
                property: header.variant,
                data,
                data_file_name,
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
            w.write_u8(variant.unknown_1)?;

            let len_data_file_name = variant.data_file_name().map(|s| s.len()).unwrap_or(0);

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
            if let Some(s) = &variant.data_file_name {
                w.write_all(s.as_bytes())?;
            }
        }

        Ok(w.into_inner())
    }

    #[tracing::instrument(name = "File::from_sjson", skip(sjson))]
    pub async fn from_sjson<P, S>(
        name: String,
        file_type: BundleFileType,
        sjson: S,
        root: P,
    ) -> Result<Self>
    where
        P: AsRef<Path> + std::fmt::Debug,
        S: AsRef<str>,
    {
        match file_type {
            BundleFileType::Lua => lua::compile(name.clone(), sjson)
                .wrap_err_with(|| format!("Failed to compile Lua file '{}'", name)),
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

    pub fn matches_name<S>(&self, name: S) -> bool
    where
        S: Into<IdString64>,
    {
        let name = name.into();
        if self.name == name {
            return true;
        }

        if let IdString64::String(name) = name {
            self.name(false, None) == name || self.name(true, None) == name
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

    #[tracing::instrument(name = "File::decompiled", skip_all)]
    pub async fn decompiled(&self, ctx: &crate::Context) -> Result<Vec<UserFile>> {
        let file_type = self.file_type();

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                name = self.name(true, None),
                variants = self.variants.len(),
                "Attempting to decompile"
            );
        }

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
