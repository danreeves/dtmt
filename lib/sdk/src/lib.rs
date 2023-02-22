mod binary;
mod bundle;
mod context;
pub mod filetype;
pub mod murmur;

pub use binary::{FromBinary, ToBinary};
pub use bundle::database::BundleDatabase;
pub use bundle::decompress;
pub use bundle::{Bundle, BundleFile, BundleFileType, BundleFileVariant};
pub use context::Context;

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct ModConfigResources {
    pub init: String,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub localization: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct ModConfig {
    #[serde(skip)]
    pub dir: std::path::PathBuf,
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub packages: Vec<std::path::PathBuf>,
    pub resources: ModConfigResources,
    #[serde(default)]
    pub depends: Vec<String>,
}
