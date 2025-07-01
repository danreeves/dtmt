use color_eyre::eyre;
use color_eyre::Result;
use serde::Serialize;

use crate::murmur::Murmur64;

macro_rules! make_enum {
    (
        $( $variant:ident, $hash:expr, $ext:expr $(, $decompiled:expr)? ; )+
    ) => {
        #[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
        pub enum BundleFileType {
            $(
                $variant,
            )+
            Unknown(Murmur64),
        }

        impl BundleFileType {
            pub fn ext_name(&self) -> String {
                match self {
                    $(
                        Self::$variant => String::from($ext),
                    )+
                    Self::Unknown(s) => format!("{s:016X}"),
                }
            }

            pub fn decompiled_ext_name(&self) -> String {
                match self {
                    $(
                        $( Self::$variant => String::from($decompiled), )?
                    )+
                    _ => self.ext_name(),
                }
            }
        }

        impl std::str::FromStr for BundleFileType {
            type Err = color_eyre::Report;
            fn from_str(s: &str) -> Result<Self> {
                match s {
                    $(
                        $ext => Ok(Self::$variant),
                    )+
                    s => eyre::bail!("Unknown type string '{}'", s),
                }
            }
        }

        impl From<u64> for BundleFileType {
            fn from(h: u64) -> Self {
                match h {
                    $(
                        $hash => Self::$variant,
                    )+
                    hash => Self::Unknown(hash.into()),
                }
            }
        }

        impl From<BundleFileType> for u64 {
            fn from(t: BundleFileType) -> u64 {
                match t {
                    $(
                        BundleFileType::$variant => $hash,
                    )+
                    BundleFileType::Unknown(hash) => hash.into(),
                }
            }
        }
    }
}

make_enum! {
    AnimationCurves,          0xdcfb9e18fff13984, "animation_curves";
    Animation,                0x931e336d7646cc26, "animation";
    Apb,                      0x3eed05ba83af5090, "apb";
    BakedLighting,            0x7ffdb779b04e4ed1, "baked_lighting";
    Bik,                      0xaa5965f03029fa18, "bik";
    BlendSet,                 0xe301e8af94e3b5a3, "blend_set";
    Bones,                    0x18dead01056b72e9, "bones";
    Chroma,                   0xb7893adf7567506a, "chroma";
    CommonPackage,            0xfe9754bd19814a47, "common_package";
    Config,                   0x82645835e6b73232, "config";
    Crypto,                   0x69108ded1e3e634b, "crypto";
    Data,                     0x8fd0d44d20650b68, "data";
    Entity,                   0x9831ca893b0d087d, "entity";
    Flow,                     0x92d3ee038eeb610d, "flow";
    Font,                     0x9efe0a916aae7880, "font";
    Ies,                      0x8f7d5a2c0f967655, "ies";
    Ini,                      0xd526a27da14f1dc5, "ini";
    Input,                    0x2bbcabe5074ade9e, "input";
    Ivf,                      0xfa4a8e091a91201e, "ivf";
    Keys,                     0xa62f9297dc969e85, "keys";
    Level,                    0x2a690fd348fe9ac5, "level";
    Lua,                      0xa14e8dfa2cd117e2, "lua";
    Material,                 0xeac0b497876adedf, "material";
    Mod,                      0x3fcdd69156a46417, "mod";
    MouseCursor,              0xb277b11fe4a61d37, "mouse_cursor";
    NavData,                  0x169de9566953d264, "nav_data";
    NetworkConfig,            0x3b1fa9e8f6bac374, "network_config";
    OddleNet,                 0xb0f2c12eb107f4d8, "oodle_net";
    Package,                  0xad9c6d9ed1e5e77a, "package";
    Particles,                0xa8193123526fad64, "particles";
    PhysicsProperties,        0xbf21403a3ab0bbb1, "physics_properties";
    RenderConfig,             0x27862fe24795319c, "render_config";
    RtPipeline,               0x9ca183c2d0e76dee, "rt_pipeline";
    Scene,                    0x9d0a795bfe818d19, "scene";
    Shader,                   0xcce8d5b5f5ae333f, "shader";
    ShaderLibrary,            0xe5ee32a477239a93, "shader_library";
    ShaderLibraryGroup,       0x9e5c3cc74575aeb5, "shader_library_group";
    ShadingEnvionmentMapping, 0x250e0a11ac8e26f8, "shading_envionment_mapping";
    ShadingEnvironment,       0xfe73c7dcff8a7ca5, "shading_environment";
    Slug,                     0xa27b4d04a9ba6f9e, "slug";
    SlugAlbum,                0xe9fc9ea7042e5ec0, "slug_album";
    SoundEnvironment,         0xd8b27864a97ffdd7, "sound_environment";
    SpuJob,                   0xf97af9983c05b950, "spu_job";
    StateMachine,             0xa486d4045106165c, "state_machine";
    StaticPVS,                0xe3f0baa17d620321, "static_pvs";
    Strings,                  0x0d972bab10b40fd3, "strings";
    SurfaceProperties,        0xad2d3fa30d9ab394, "surface_properties";
    Texture,                  0xcd4238c6a0c69e32, "texture",      "dds";
    TimpaniBank,              0x99736be1fff739a4, "timpani_bank";
    TimpaniMaster,            0x00a3e6c59a2b9c6c, "timpani_master";
    Tome,                     0x19c792357c99f49b, "tome";
    Ugg,                      0x712d6e3dd1024c9c, "ugg";
    Unit,                     0xe0a48d0be9a7453f, "unit";
    Upb,                      0xa99510c6e86dd3c2, "upb";
    VectorField,              0xf7505933166d6755, "vector_field";
    Wav,                      0x786f65c00a816b19, "wav";
    WwiseBank,                0x535a7bd3e650d799, "wwise_bank",   "bnk";
    WwiseDep,                 0xaf32095c82f2b070, "wwise_dep";
    WwiseEvent,               0xaabdd317b58dfc8a, "wwise_event";
    WwiseMetadata,            0xd50a8b7e1c82b110, "wwise_metadata";
    WwiseStream,              0x504b55235d21440e, "wwise_stream", "ogg";
    Xml,                      0x76015845a6003765, "xml";
    Theme,                    0x38BB9442048A7FBD, "theme";
    MissionThemes,            0x80F2DE893657F83A, "mission_themes";
}

impl BundleFileType {
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

impl From<Murmur64> for BundleFileType {
    fn from(value: Murmur64) -> Self {
        Self::from(Into::<u64>::into(value))
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
