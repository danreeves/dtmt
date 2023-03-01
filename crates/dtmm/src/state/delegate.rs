use druid::{
    AppDelegate, Command, DelegateCtx, Env, FileInfo, Handled, Selector, SingleUse, Target,
};
use tokio::sync::mpsc::UnboundedSender;

use super::{ModInfo, State};

pub(crate) const ACTION_SELECT_MOD: Selector<usize> = Selector::new("dtmm.action.select-mod");
pub(crate) const ACTION_SELECTED_MOD_UP: Selector = Selector::new("dtmm.action.selected-mod-up");
pub(crate) const ACTION_SELECTED_MOD_DOWN: Selector =
    Selector::new("dtmm.action.selected-mod-down");
pub(crate) const ACTION_START_DELETE_SELECTED_MOD: Selector<SingleUse<ModInfo>> =
    Selector::new("dtmm.action.srart-delete-selected-mod");
pub(crate) const ACTION_FINISH_DELETE_SELECTED_MOD: Selector<SingleUse<ModInfo>> =
    Selector::new("dtmm.action.finish-delete-selected-mod");

pub(crate) const ACTION_START_DEPLOY: Selector = Selector::new("dtmm.action.start-deploy");
pub(crate) const ACTION_FINISH_DEPLOY: Selector = Selector::new("dtmm.action.finish-deploy");

pub(crate) const ACTION_START_RESET_DEPLOYMENT: Selector =
    Selector::new("dtmm.action.start-reset-deployment");
pub(crate) const ACTION_FINISH_RESET_DEPLOYMENT: Selector =
    Selector::new("dtmm.action.finish-reset-deployment");

pub(crate) const ACTION_ADD_MOD: Selector<FileInfo> = Selector::new("dtmm.action.add-mod");
pub(crate) const ACTION_FINISH_ADD_MOD: Selector<SingleUse<ModInfo>> =
    Selector::new("dtmm.action.finish-add-mod");

pub(crate) const ACTION_LOG: Selector<SingleUse<String>> = Selector::new("dtmm.action.log");

pub(crate) const ACTION_START_SAVE_SETTINGS: Selector =
    Selector::new("dtmm.action.start-save-settings");
pub(crate) const ACTION_FINISH_SAVE_SETTINGS: Selector =
    Selector::new("dtmm.action.finish-save-settings");

pub(crate) enum AsyncAction {
    DeployMods(State),
    ResetDeployment(State),
    AddMod((State, FileInfo)),
    DeleteMod((State, ModInfo)),
    SaveSettings(State),
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
            cmd if cmd.is(ACTION_START_RESET_DEPLOYMENT) => {
                if self
                    .sender
                    .send(AsyncAction::ResetDeployment(state.clone()))
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
            cmd if cmd.is(ACTION_START_DELETE_SELECTED_MOD) => {
                let info = cmd
                    .get(ACTION_START_DELETE_SELECTED_MOD)
                    .and_then(|info| info.take())
                    .expect("command type matched but didn't contain the expected value");
                if self
                    .sender
                    .send(AsyncAction::DeleteMod((state.clone(), info)))
                    .is_err()
                {
                    tracing::error!("Failed to queue action to deploy mods");
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_DELETE_SELECTED_MOD) => {
                let info = cmd
                    .get(ACTION_FINISH_DELETE_SELECTED_MOD)
                    .and_then(|info| info.take())
                    .expect("command type matched but didn't contain the expected value");
                let found = state.mods.iter().enumerate().find(|(_, i)| i.id == info.id);
                let Some((index, _)) = found else {
                    return Handled::No;
                };

                state.mods.remove(index);
                ctx.submit_command(ACTION_START_SAVE_SETTINGS);

                Handled::Yes
            }
            cmd if cmd.is(ACTION_ADD_MOD) => {
                let info = cmd
                    .get(ACTION_ADD_MOD)
                    .expect("command type matched but didn't contain the expected value");
                if self
                    .sender
                    .send(AsyncAction::AddMod((state.clone(), info.clone())))
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
                    ctx.submit_command(ACTION_START_SAVE_SETTINGS);
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
                    .send(AsyncAction::SaveSettings(state.clone()))
                    .is_ok()
                {
                    state.is_save_in_progress = true;
                } else {
                    tracing::error!("Failed to queue action to save settings");
                }

                Handled::Yes
            }
            cmd if cmd.is(ACTION_FINISH_SAVE_SETTINGS) => {
                state.is_save_in_progress = false;

                if state.is_next_save_pending {
                    state.is_next_save_pending = false;
                    ctx.submit_command(ACTION_START_SAVE_SETTINGS);
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
