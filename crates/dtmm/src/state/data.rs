use std::{path::PathBuf, sync::Arc};

use druid::{
    im::{HashMap, Vector},
    Data, Lens, WindowHandle, WindowId,
};
use dtmt_shared::ModConfig;

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

#[derive(Clone, Data, Debug, Lens, PartialEq)]
pub(crate) struct ModInfo {
    pub id: String,
    pub name: String,
    pub description: Arc<String>,
    pub enabled: bool,
    #[lens(ignore)]
    #[data(ignore)]
    pub packages: Vector<Arc<PackageInfo>>,
    #[lens(ignore)]
    #[data(ignore)]
    pub resources: ModResourceInfo,
}

impl ModInfo {
    pub fn new(cfg: ModConfig, packages: Vector<Arc<PackageInfo>>) -> Self {
        Self {
            id: cfg.id,
            name: cfg.name,
            description: Arc::new(cfg.description),
            enabled: false,
            packages,
            resources: ModResourceInfo {
                init: cfg.resources.init,
                data: cfg.resources.data,
                localization: cfg.resources.localization,
            },
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
    pub game_dir: Arc<PathBuf>,
    pub data_dir: Arc<PathBuf>,
    pub log: Arc<String>,

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

    pub fn new(config_path: PathBuf, game_dir: PathBuf, data_dir: PathBuf) -> Self {
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
            config_path: Arc::new(config_path),
            game_dir: Arc::new(game_dir),
            data_dir: Arc::new(data_dir),
            log: Arc::new(String::new()),
            windows: HashMap::new(),
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

    pub(crate) fn get_mod_dir(&self) -> PathBuf {
        self.data_dir.join("mods")
    }

    pub(crate) fn add_log_line(&mut self, line: String) {
        let log = Arc::make_mut(&mut self.log);
        log.push_str(&line);
    }
}
