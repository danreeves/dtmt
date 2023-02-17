use std::path::PathBuf;
use std::sync::Arc;

use druid::im::Vector;
use druid::text::Formatter;
use druid::widget::Controller;
use druid::{
    AppDelegate, Data, DelegateCtx, Env, Event, EventCtx, Handled, Lens, Selector, Target,
    Widget,
};
use tokio::sync::mpsc::UnboundedSender;

pub const ACTION_SELECT_MOD: Selector<usize> = Selector::new("dtmm.action..select-mod");
pub const ACTION_SELECTED_MOD_UP: Selector = Selector::new("dtmm.action.selected-mod-up");
pub const ACTION_SELECTED_MOD_DOWN: Selector = Selector::new("dtmm.action.selected-mod-down");
pub const ACTION_DELETE_SELECTED_MOD: Selector = Selector::new("dtmm.action.delete-selected-mod");

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
    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_files(&self) -> &Vector<String> {
        &self.files
    }
}

#[derive(Clone, Data, Debug, Lens)]
pub(crate) struct ModInfo {
    name: String,
    description: Arc<String>,
    enabled: bool,
    #[lens(ignore)]
    packages: Vector<PackageInfo>,
}

impl ModInfo {
    pub fn new() -> Self {
        Self {
            name: format!("Test Mod: {:?}", std::time::SystemTime::now()),
            description: Arc::new(String::from("A test dummy")),
            enabled: false,
            packages: Vector::new(),
        }
    }

    pub fn get_packages(&self) -> &Vector<PackageInfo> {
        &self.packages
    }

    pub(crate) fn get_name(&self) -> &String {
        &self.name
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

    pub fn new() -> Self {
        let ctx = sdk::Context::new();

        let (game_dir, data_dir) = if cfg!(debug_assertions) {
            (
                std::env::current_dir().expect("PWD is borked").join("data"),
                PathBuf::from("/tmp/dtmm"),
            )
        } else {
            (PathBuf::new(), PathBuf::new())
        };

        Self {
            ctx: Arc::new(ctx),
            current_view: View::default(),
            mods: Vector::new(),
            selected_mod_index: None,
            is_deployment_in_progress: false,
            game_dir: Arc::new(game_dir),
            data_dir: Arc::new(data_dir),
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

pub struct StateController {}

impl StateController {
    pub fn new() -> Self {
        Self {}
    }
}

// TODO: Turn notifications into commands on the AppDelegate
impl<W: Widget<State>> Controller<State, W> for StateController {
    #[tracing::instrument(name = "StateController::event", skip_all)]
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        state: &mut State,
        env: &Env,
    ) {
        match event {
            Event::Notification(notif) if notif.is(ACTION_SELECT_MOD) => {
                ctx.set_handled();
                let index = notif
                    .get(ACTION_SELECT_MOD)
                    .expect("notification type didn't match after check");

                state.select_mod(*index);
            }
            Event::Notification(notif) if notif.is(ACTION_SELECTED_MOD_UP) => {
                ctx.set_handled();
                let Some(i) = state.selected_mod_index else {
                    return;
                };

                let len = state.mods.len();
                if len == 0 || i == 0 {
                    return;
                }

                state.mods.swap(i, i - 1);
                state.selected_mod_index = Some(i - 1);
            }
            Event::Notification(notif) if notif.is(ACTION_SELECTED_MOD_DOWN) => {
                ctx.set_handled();
                let Some(i) = state.selected_mod_index else {
                    return;
                };

                let len = state.mods.len();
                if len == 0 || i == usize::MAX || i >= len - 1 {
                    return;
                }

                state.mods.swap(i, i + 1);
                state.selected_mod_index = Some(i + 1);
            }
            Event::Notification(notif) if notif.is(ACTION_DELETE_SELECTED_MOD) => {
                ctx.set_handled();
                let Some(index) = state.selected_mod_index else {
                    return;
                };

                state.mods.remove(index);
            }
            _ => child.event(ctx, event, state, env),
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
