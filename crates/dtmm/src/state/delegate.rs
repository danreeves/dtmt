use std::{path::PathBuf, sync::Arc};

use color_eyre::Report;
use druid::{
    im::Vector, AppDelegate, Command, DelegateCtx, Env, FileInfo, Handled, Selector, SingleUse,
    Target, WindowHandle, WindowId,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::window;

use super::{ModInfo, State};

pub(crate) const ACTION_SELECT_MOD: Selector<usize> = Selector::new("dtmm.action.select-mod");
pub(crate) const ACTION_SELECTED_MOD_UP: Selector = Selector::new("dtmm.action.selected-mod-up");
pub(crate) const ACTION_SELECTED_MOD_DOWN: Selector =
    Selector::new("dtmm.action.selected-mod-down");
pub(crate) const ACTION_START_DELETE_SELECTED_MOD: Selector<SingleUse<Arc<ModInfo>>> =
    Selector::new("dtmm.action.srart-delete-selected-mod");
pub(crate) const ACTION_FINISH_DELETE_SELECTED_MOD: Selector<SingleUse<Arc<ModInfo>>> =
    Selector::new("dtmm.action.finish-delete-selected-mod");

pub(crate) const ACTION_START_DEPLOY: Selector = Selector::new("dtmm.action.start-deploy");
pub(crate) const ACTION_FINISH_DEPLOY: Selector = Selector::new("dtmm.action.finish-deploy");

pub(crate) const ACTION_START_RESET_DEPLOYMENT: Selector =
    Selector::new("dtmm.action.start-reset-deployment");
pub(crate) const ACTION_FINISH_RESET_DEPLOYMENT: Selector =
    Selector::new("dtmm.action.finish-reset-deployment");

pub(crate) const ACTION_ADD_MOD: Selector<FileInfo> = Selector::new("dtmm.action.add-mod");
pub(crate) const ACTION_FINISH_ADD_MOD: Selector<SingleUse<Arc<ModInfo>>> =
    Selector::new("dtmm.action.finish-add-mod");

pub(crate) const ACTION_LOG: Selector<SingleUse<String>> = Selector::new("dtmm.action.log");

pub(crate) const ACTION_START_SAVE_SETTINGS: Selector =
    Selector::new("dtmm.action.start-save-settings");
pub(crate) const ACTION_FINISH_SAVE_SETTINGS: Selector =
    Selector::new("dtmm.action.finish-save-settings");

pub(crate) const ACTION_SET_DIRTY: Selector = Selector::new("dtmm.action.set-dirty");

pub(crate) const ACTION_SHOW_ERROR_DIALOG: Selector<SingleUse<Report>> =
    Selector::new("dtmm.action.show-error-dialog");

pub(crate) const ACTION_SET_WINDOW_HANDLE: Selector<SingleUse<(WindowId, WindowHandle)>> =
    Selector::new("dtmm.action.set-window-handle");

// A sub-selection of `State`'s fields that are required in `AsyncAction`s and that are
// `Send + Sync`
pub(crate) struct ActionState {
    pub mods: Vector<Arc<ModInfo>>,
    pub game_dir: Arc<PathBuf>,
    pub data_dir: Arc<PathBuf>,
    pub mod_dir: Arc<PathBuf>,
    pub config_path: Arc<PathBuf>,
    pub ctx: Arc<sdk::Context>,
}

impl From<State> for ActionState {
    fn from(state: State) -> Self {
        Self {
            mods: state.mods,
            game_dir: state.game_dir,
            mod_dir: Arc::new(state.data_dir.join("mods")),
            data_dir: state.data_dir,
            config_path: state.config_path,
            ctx: state.ctx,
        }
    }
}

pub(crate) enum AsyncAction {
    DeployMods(ActionState),
    ResetDeployment(ActionState),
    AddMod(ActionState, FileInfo),
    DeleteMod(ActionState, Arc<ModInfo>),
    SaveSettings(ActionState),
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
        ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        state: &mut State,
        _env: &Env,
    ) -> Handled {
        if cfg!(debug_assertions) && !cmd.is(ACTION_LOG) {
            tracing::trace!(?cmd);
        }

        match cmd {
            cmd if cmd.is(ACTION_START_DEPLOY) => {
                if self
                    .sender
                    .send(AsyncAction::DeployMods(state.clone().into()))
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
                state.dirty = false;
                Handled::Yes
            }
            cmd if cmd.is(ACTION_START_RESET_DEPLOYMENT) => {
                if self
                    .sender
                    .send(AsyncAction::ResetDeployment(state.clone().into()))
                    .is_ok()
                {
                    state.is_reset_in_progress = true;
                } else {
                    tracing::error!("Failed to queue action to reset mod deployment");
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_RESET_DEPLOYMENT) => {
                state.is_reset_in_progress = false;
                Handled::Yes
            }
            cmd if cmd.is(ACTION_SELECT_MOD) => {
                let index = cmd
                    .get(ACTION_SELECT_MOD)
                    .expect("command type matched but didn't contain the expected value");

                state.select_mod(*index);
                // ctx.submit_command(ACTION_START_SAVE_SETTINGS);
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
                // ctx.submit_command(ACTION_START_SAVE_SETTINGS);
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
                // ctx.submit_command(ACTION_START_SAVE_SETTINGS);
                Handled::Yes
            }
            cmd if cmd.is(ACTION_START_DELETE_SELECTED_MOD) => {
                let info = cmd
                    .get(ACTION_START_DELETE_SELECTED_MOD)
                    .and_then(SingleUse::take)
                    .expect("command type matched but didn't contain the expected value");

                if self
                    .sender
                    .send(AsyncAction::DeleteMod(state.clone().into(), info))
                    .is_err()
                {
                    tracing::error!("Failed to queue action to deploy mods");
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_DELETE_SELECTED_MOD) => {
                let info = cmd
                    .get(ACTION_FINISH_DELETE_SELECTED_MOD)
                    .and_then(SingleUse::take)
                    .expect("command type matched but didn't contain the expected value");

                let found = state.mods.iter().enumerate().find(|(_, i)| i.id == info.id);
                let Some((index, _)) = found else {
                    return Handled::No;
                };

                state.mods.remove(index);

                Handled::Yes
            }
            cmd if cmd.is(ACTION_ADD_MOD) => {
                let info = cmd
                    .get(ACTION_ADD_MOD)
                    .expect("command type matched but didn't contain the expected value");

                if self
                    .sender
                    .send(AsyncAction::AddMod(state.clone().into(), info.clone()))
                    .is_err()
                {
                    tracing::error!("Failed to queue action to add mod");
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
            cmd if cmd.is(ACTION_LOG) => {
                let line = cmd
                    .get(ACTION_LOG)
                    .expect("command type matched but didn't contain the expected value");
                if let Some(line) = line.take() {
                    state.add_log_line(line);
                }
                Handled::Yes
            }
            cmd if cmd.is(ACTION_START_SAVE_SETTINGS) => {
                if state.is_save_in_progress {
                    state.is_next_save_pending = true;
                } else if self
                    .sender
                    .send(AsyncAction::SaveSettings(state.clone().into()))
                    .is_ok()
                {
                    state.is_save_in_progress = true;
                } else {
                    tracing::error!("Failed to queue action to save settings");
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_SAVE_SETTINGS) => {
                tracing::trace!(
                    in_progress = state.is_save_in_progress,
                    next_pending = state.is_next_save_pending,
                    "Finished saving settings",
                );
                state.is_save_in_progress = false;

                if state.is_next_save_pending {
                    state.is_next_save_pending = false;
                    ctx.submit_command(ACTION_START_SAVE_SETTINGS);
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_SET_DIRTY) => {
                state.dirty = true;
                Handled::Yes
            }
            cmd if cmd.is(ACTION_SHOW_ERROR_DIALOG) => {
                let err = cmd
                    .get(ACTION_SHOW_ERROR_DIALOG)
                    .and_then(SingleUse::take)
                    .expect("command type matched but didn't contain the expected value");

                let window = state
                    .windows
                    .get(&window::main::WINDOW_ID)
                    .expect("root window does not exist");

                let dialog = window::dialog::error::<State>(err, window.clone());
                ctx.new_window(dialog);

                Handled::Yes
            }
            cmd if cmd.is(ACTION_SET_WINDOW_HANDLE) => {
                let (id, handle) = cmd
                    .get(ACTION_SET_WINDOW_HANDLE)
                    .and_then(SingleUse::take)
                    .expect("command type matched but didn't contain the expected value");

                state.windows.insert(id, handle);
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

    fn window_added(
        &mut self,
        id: WindowId,
        handle: WindowHandle,
        data: &mut State,
        _: &Env,
        _: &mut DelegateCtx,
    ) {
        data.windows.insert(id, handle);
    }

    fn window_removed(&mut self, id: WindowId, data: &mut State, _: &Env, _: &mut DelegateCtx) {
        data.windows.remove(&id);
    }
}
