use std::path::PathBuf;
use std::sync::Arc;

use druid::im::{HashMap, Vector};
use druid::text::RichText;
use druid::{Data, ImageBuf, Lens, WindowHandle, WindowId};
use dtmt_shared::ModConfig;
use nexusmods::Mod as NexusMod;

use super::SelectedModLens;

#[derive(Copy, Clone, Data, Debug, PartialEq)]
pub(crate) enum View {
    Mods,
    Settings,
}

impl Default for View {
    fn default() -> Self {
        Self::Mods
    }
}

#[derive(Clone, Data, Debug, PartialEq)]
pub struct PackageInfo {
    pub name: String,
    pub files: Vector<String>,
}

impl PackageInfo {
    pub fn new(name: String, files: Vector<String>) -> Self {
        Self { name, files }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModResourceInfo {
    pub init: PathBuf,
    pub data: Option<PathBuf>,
    pub localization: Option<PathBuf>,
}

#[derive(Clone, Data, Debug, PartialEq)]
pub(crate) enum ModOrder {
    Before,
    After,
}

#[derive(Clone, Data, Debug, PartialEq)]
pub(crate) struct ModDependency {
    pub id: String,
    pub order: ModOrder,
}

impl From<dtmt_shared::ModDependency> for ModDependency {
    fn from(value: dtmt_shared::ModDependency) -> Self {
        match value {
            dtmt_shared::ModDependency::ID(id) => ModDependency {
                id,
                order: ModOrder::Before,
            },
            dtmt_shared::ModDependency::Config { id, order } => ModDependency {
                id,
                order: match order {
                    dtmt_shared::ModOrder::Before => ModOrder::Before,
                    dtmt_shared::ModOrder::After => ModOrder::After,
                },
            },
        }
    }
}

#[derive(Clone, Data, Debug, Lens, serde::Serialize, serde::Deserialize)]
pub(crate) struct NexusInfo {
    pub id: u64,
    pub name: String,
    pub version: String,
    pub author: String,
    pub summary: Arc<String>,
    pub description: Arc<String>,
    pub picture_url: Arc<String>,
}

impl From<NexusMod> for NexusInfo {
    fn from(value: NexusMod) -> Self {
        Self {
            id: value.mod_id,
            name: value.name,
            version: value.version,
            author: value.author,
            summary: Arc::new(value.summary),
            description: Arc::new(value.description),
            picture_url: Arc::new(value.picture_url.into()),
        }
    }
}

#[derive(Clone, Data, Lens)]
pub(crate) struct ModInfo {
    pub id: String,
    pub name: String,
    pub summary: Arc<String>,
    pub description: Option<Arc<String>>,
    pub categories: Vector<String>,
    pub author: Option<String>,
    pub image: Option<ImageBuf>,
    pub version: String,
    pub enabled: bool,
    #[lens(ignore)]
    #[data(ignore)]
    pub packages: Vector<Arc<PackageInfo>>,
    #[lens(ignore)]
    #[data(ignore)]
    pub resources: ModResourceInfo,
    pub depends: Vector<ModDependency>,
    pub bundled: bool,
    #[data(ignore)]
    pub nexus: Option<NexusInfo>,
}

impl std::fmt::Debug for ModInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModInfo")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("summary", &self.summary)
            .field(
                "description",
                &(match &self.description {
                    Some(desc) => format!("Some(String[0..{}])", desc.len()),
                    None => "None".to_string(),
                }),
            )
            .field("categories", &self.categories)
            .field("author", &self.author)
            .field(
                "image",
                &(match &self.image {
                    Some(image) => format!("Some(ImageBuf[{}x{}])", image.width(), image.height()),
                    None => "None".to_string(),
                }),
            )
            .field("version", &self.version)
            .field("enabled", &self.enabled)
            .field("packages", &format!("Vec[0..{}]", self.packages.len()))
            .field("resources", &self.resources)
            .field("depends", &self.depends)
            .field("bundled", &self.bundled)
            .field("nexus", &self.nexus)
            .finish()
    }
}

impl ModInfo {
    pub fn new(
        cfg: ModConfig,
        packages: Vector<Arc<PackageInfo>>,
        image: Option<ImageBuf>,
        nexus: Option<NexusInfo>,
    ) -> Self {
        Self {
            id: cfg.id,
            name: cfg.name,
            summary: Arc::new(cfg.summary),
            description: cfg.description.map(Arc::new),
            author: cfg.author,
            version: cfg.version,
            enabled: false,
            packages,
            bundled: cfg.bundled,
            image,
            categories: cfg.categories.into_iter().collect(),
            resources: ModResourceInfo {
                init: cfg.resources.init,
                data: cfg.resources.data,
                localization: cfg.resources.localization,
            },
            depends: cfg.depends.into_iter().map(ModDependency::from).collect(),
            nexus,
        }
    }
}

#[derive(Clone, Data, Lens)]
pub(crate) struct State {
    pub current_view: View,
    pub mods: Vector<Arc<ModInfo>>,
    pub selected_mod_index: Option<usize>,
    pub dirty: bool,
    pub is_deployment_in_progress: bool,
    pub is_reset_in_progress: bool,
    pub is_save_in_progress: bool,
    pub is_next_save_pending: bool,
    pub is_update_in_progress: bool,
    pub is_io_enabled: bool,
    pub game_dir: Arc<PathBuf>,
    pub data_dir: Arc<PathBuf>,
    pub nexus_api_key: Arc<String>,
    pub log: Vector<RichText>,
    // True, when the initial loading of configuration and mods is still in progress
    pub loading: bool,

    #[lens(ignore)]
    #[data(ignore)]
    pub config_path: Arc<PathBuf>,
    #[lens(ignore)]
    #[data(ignore)]
    pub windows: HashMap<WindowId, WindowHandle>,
    #[lens(ignore)]
    #[data(ignore)]
    pub ctx: Arc<sdk::Context>,
}

impl State {
    #[allow(non_upper_case_globals)]
    pub const selected_mod: SelectedModLens = SelectedModLens;

    pub fn new() -> Self {
        let ctx = sdk::Context::new();

        Self {
            ctx: Arc::new(ctx),
            current_view: View::default(),
            mods: Vector::new(),
            selected_mod_index: None,
            dirty: false,
            is_deployment_in_progress: false,
            is_reset_in_progress: false,
            is_save_in_progress: false,
            is_next_save_pending: false,
            is_update_in_progress: false,
            is_io_enabled: false,
            config_path: Arc::new(PathBuf::new()),
            game_dir: Arc::new(PathBuf::new()),
            data_dir: Arc::new(PathBuf::new()),
            nexus_api_key: Arc::new(String::new()),
            log: Vector::new(),
            windows: HashMap::new(),
            loading: true,
        }
    }

    pub fn select_mod(&mut self, index: usize) {
        self.selected_mod_index = Some(index);
    }

    pub fn add_mod(&mut self, info: Arc<ModInfo>) {
        if let Some(pos) = self.mods.iter().position(|i| i.id == info.id) {
            self.mods.set(pos, info);
            self.selected_mod_index = Some(pos);
        } else {
            self.mods.push_back(info);
            self.selected_mod_index = Some(self.mods.len() - 1);
        }
    }

    pub fn can_move_mod_down(&self) -> bool {
        self.selected_mod_index
            .map(|i| i < (self.mods.len().saturating_sub(1)))
            .unwrap_or(false)
    }

    pub fn can_move_mod_up(&self) -> bool {
        self.selected_mod_index.map(|i| i > 0).unwrap_or(false)
    }
}
