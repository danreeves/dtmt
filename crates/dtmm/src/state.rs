use std::path::PathBuf;
use std::sync::Arc;

use druid::im::Vector;
use druid::text::Formatter;
use druid::{
    AppDelegate, Command, Data, DelegateCtx, Env, FileInfo, Handled, Lens, Selector, SingleUse,
    Target,
};
use sdk::ModConfig;
use tokio::sync::mpsc::UnboundedSender;

use crate::Config;

pub(crate) const ACTION_SELECT_MOD: Selector<usize> = Selector::new("dtmm.action.select-mod");
pub(crate) const ACTION_SELECTED_MOD_UP: Selector = Selector::new("dtmm.action.selected-mod-up");
pub(crate) const ACTION_SELECTED_MOD_DOWN: Selector =
    Selector::new("dtmm.action.selected-mod-down");
pub(crate) const ACTION_DELETE_SELECTED_MOD: Selector =
    Selector::new("dtmm.action.delete-selected-mod");

pub(crate) const ACTION_START_DEPLOY: Selector = Selector::new("dtmm.action.start-deploy");
pub(crate) const ACTION_FINISH_DEPLOY: Selector = Selector::new("dtmm.action.finish-deploy");

pub(crate) const ACTION_ADD_MOD: Selector<FileInfo> = Selector::new("dtmm.action.add-mod");
pub(crate) const ACTION_FINISH_ADD_MOD: Selector<SingleUse<ModInfo>> =
    Selector::new("dtmm.action.finish-add-mod");

#[derive(Copy, Clone, Data, Debug, PartialEq)]
pub(crate) enum View {
    Mods,
    Settings,
    About,
}

impl Default for View {
    fn default() -> Self {
        Self::Mods
    }
}

#[derive(Clone, Data, Debug)]
pub struct PackageInfo {
    name: String,
    files: Vector<String>,
}

impl PackageInfo {
    pub fn new(name: String, files: Vector<String>) -> Self {
        Self { name, files }
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_files(&self) -> &Vector<String> {
        &self.files
    }
}

#[derive(Clone, Data, Debug)]
pub(crate) struct ModResourceInfo {
    init: String,
    data: Option<String>,
    localization: Option<String>,
}

impl ModResourceInfo {
    pub(crate) fn get_init(&self) -> &String {
        &self.init
    }

    pub(crate) fn get_data(&self) -> Option<&String> {
        self.data.as_ref()
    }

    pub(crate) fn get_localization(&self) -> Option<&String> {
        self.localization.as_ref()
    }
}

#[derive(Clone, Data, Debug, Lens)]
pub(crate) struct ModInfo {
    id: String,
    name: String,
    description: Arc<String>,
    enabled: bool,
    #[lens(ignore)]
    packages: Vector<PackageInfo>,
    #[lens(ignore)]
    resources: ModResourceInfo,
}

impl ModInfo {
    pub fn new(cfg: ModConfig, packages: Vector<PackageInfo>) -> Self {
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

    pub fn get_packages(&self) -> &Vector<PackageInfo> {
        &self.packages
    }

    pub(crate) fn get_name(&self) -> &String {
        &self.name
    }

    pub(crate) fn get_id(&self) -> &String {
        &self.id
    }

    pub(crate) fn get_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn get_resources(&self) -> &ModResourceInfo {
        &self.resources
    }
}

impl PartialEq for ModInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

#[derive(Clone, Data, Lens)]
pub(crate) struct State {
    current_view: View,
    mods: Vector<ModInfo>,
    selected_mod_index: Option<usize>,
    is_deployment_in_progress: bool,
    game_dir: Arc<PathBuf>,
    data_dir: Arc<PathBuf>,
    ctx: Arc<sdk::Context>,
}

impl State {
    #[allow(non_upper_case_globals)]
    pub const selected_mod: SelectedModLens = SelectedModLens;

    pub fn new(config: Config) -> Self {
        let ctx = sdk::Context::new();

        Self {
            ctx: Arc::new(ctx),
            current_view: View::default(),
            mods: Vector::new(),
            selected_mod_index: None,
            is_deployment_in_progress: false,
            game_dir: Arc::new(config.game_dir.unwrap_or_default()),
            data_dir: Arc::new(config.data_dir.unwrap_or_default()),
        }
    }

    pub fn get_current_view(&self) -> View {
        self.current_view
    }

    pub fn set_current_view(&mut self, view: View) {
        self.current_view = view;
    }

    pub fn get_mods(&self) -> Vector<ModInfo> {
        self.mods.clone()
    }

    pub fn select_mod(&mut self, index: usize) {
        self.selected_mod_index = Some(index);
    }

    pub fn add_mod(&mut self, info: ModInfo) {
        self.mods.push_back(info);
        self.selected_mod_index = Some(self.mods.len() - 1);
    }

    pub fn can_move_mod_down(&self) -> bool {
        self.selected_mod_index
            .map(|i| i < (self.mods.len().saturating_sub(1)))
            .unwrap_or(false)
    }

    pub fn can_move_mod_up(&self) -> bool {
        self.selected_mod_index.map(|i| i > 0).unwrap_or(false)
    }

    pub fn can_deploy_mods(&self) -> bool {
        !self.is_deployment_in_progress
    }

    pub(crate) fn get_game_dir(&self) -> &PathBuf {
        &self.game_dir
    }

    pub(crate) fn get_mod_dir(&self) -> PathBuf {
        self.data_dir.join("mods")
    }

    pub(crate) fn get_ctx(&self) -> Arc<sdk::Context> {
        self.ctx.clone()
    }
}

pub(crate) struct SelectedModLens;

impl Lens<State, Option<ModInfo>> for SelectedModLens {
    #[tracing::instrument(name = "SelectedModLens::with", skip_all)]
    fn with<V, F: FnOnce(&Option<ModInfo>) -> V>(&self, data: &State, f: F) -> V {
        let info = data
            .selected_mod_index
            .and_then(|i| data.mods.get(i).cloned());

        f(&info)
    }

    #[tracing::instrument(name = "SelectedModLens::with_mut", skip_all)]
    fn with_mut<V, F: FnOnce(&mut Option<ModInfo>) -> V>(&self, data: &mut State, f: F) -> V {
        match data.selected_mod_index {
            Some(i) => {
                let mut info = data.mods.get_mut(i).cloned();
                let ret = f(&mut info);

                if let Some(info) = info {
                    // TODO: Figure out a way to check for equality and
                    // only update when needed
                    data.mods.set(i, info);
                } else {
                    data.selected_mod_index = None;
                }

                ret
            }
            None => f(&mut None),
        }
    }
}

/// A Lens that maps an `im::Vector<T>` to `im::Vector<(usize, T)>`,
/// where each element in the destination vector includes its index in the
/// source vector.
pub(crate) struct IndexedVectorLens;

impl<T: Data> Lens<Vector<T>, Vector<(usize, T)>> for IndexedVectorLens {
    #[tracing::instrument(name = "IndexedVectorLens::with", skip_all)]
    fn with<V, F: FnOnce(&Vector<(usize, T)>) -> V>(&self, values: &Vector<T>, f: F) -> V {
        let indexed = values
            .iter()
            .enumerate()
            .map(|(i, val)| (i, val.clone()))
            .collect();
        f(&indexed)
    }

    #[tracing::instrument(name = "IndexedVectorLens::with_mut", skip_all)]
    fn with_mut<V, F: FnOnce(&mut Vector<(usize, T)>) -> V>(
        &self,
        values: &mut Vector<T>,
        f: F,
    ) -> V {
        let mut indexed = values
            .iter()
            .enumerate()
            .map(|(i, val)| (i, val.clone()))
            .collect();
        let ret = f(&mut indexed);

        *values = indexed.into_iter().map(|(_i, val)| val).collect();

        ret
    }
}

pub(crate) enum AsyncAction {
    DeployMods(State),
    AddMod((State, FileInfo)),
}

pub(crate) struct Delegate {
    sender: UnboundedSender<AsyncAction>,
}

impl Delegate {
    pub fn new(sender: UnboundedSender<AsyncAction>) -> Self {
        Self { sender }
    }
}

impl AppDelegate<State> for Delegate {
    #[tracing::instrument(name = "Delegate", skip_all)]
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        state: &mut State,
        _env: &Env,
    ) -> Handled {
        match cmd {
            cmd if cmd.is(ACTION_START_DEPLOY) => {
                if self
                    .sender
                    .send(AsyncAction::DeployMods(state.clone()))
                    .is_ok()
                {
                    state.is_deployment_in_progress = true;
                } else {
                    tracing::error!("Failed to queue action to deploy mods");
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_DEPLOY) => {
                state.is_deployment_in_progress = false;
                Handled::Yes
            }
            cmd if cmd.is(ACTION_SELECT_MOD) => {
                let index = cmd
                    .get(ACTION_SELECT_MOD)
                    .expect("command type matched but didn't contain the expected value");

                state.select_mod(*index);
                Handled::Yes
            }
            cmd if cmd.is(ACTION_SELECTED_MOD_UP) => {
                let Some(i) = state.selected_mod_index else {
                    return Handled::No;
                };

                let len = state.mods.len();
                if len == 0 || i == 0 {
                    return Handled::No;
                }

                state.mods.swap(i, i - 1);
                state.selected_mod_index = Some(i - 1);
                Handled::Yes
            }
            cmd if cmd.is(ACTION_SELECTED_MOD_DOWN) => {
                let Some(i) = state.selected_mod_index else {
                    return Handled::No;
                };

                let len = state.mods.len();
                if len == 0 || i == usize::MAX || i >= len - 1 {
                    return Handled::No;
                }

                state.mods.swap(i, i + 1);
                state.selected_mod_index = Some(i + 1);
                Handled::Yes
            }
            cmd if cmd.is(ACTION_DELETE_SELECTED_MOD) => {
                let Some(index) = state.selected_mod_index else {
                    return Handled::No;
                };

                state.mods.remove(index);
                Handled::Yes
            }
            cmd if cmd.is(ACTION_ADD_MOD) => {
                let info = cmd
                    .get(ACTION_ADD_MOD)
                    .expect("command type matched but didn't contain the expected value");
                if let Err(err) = self
                    .sender
                    .send(AsyncAction::AddMod((state.clone(), info.clone())))
                {
                    tracing::error!("Failed to add mod: {}", err);
                }
                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_ADD_MOD) => {
                let info = cmd
                    .get(ACTION_FINISH_ADD_MOD)
                    .expect("command type matched but didn't contain the expected value");
                if let Some(info) = info.take() {
                    state.add_mod(info);
                }
                Handled::Yes
            }
            cmd => {
                if cfg!(debug_assertions) {
                    tracing::warn!("Unknown command: {:?}", cmd);
                }
                Handled::No
            }
        }
    }
}

pub(crate) struct PathBufFormatter;

impl PathBufFormatter {
    pub fn new() -> Self {
        Self {}
    }
}

impl Formatter<Arc<PathBuf>> for PathBufFormatter {
    fn format(&self, value: &Arc<PathBuf>) -> String {
        value.display().to_string()
    }

    fn validate_partial_input(
        &self,
        _input: &str,
        _sel: &druid::text::Selection,
    ) -> druid::text::Validation {
        druid::text::Validation::success()
    }

    fn value(&self, input: &str) -> Result<Arc<PathBuf>, druid::text::ValidationError> {
        let p = PathBuf::from(input);
        Ok(Arc::new(p))
    }
}
