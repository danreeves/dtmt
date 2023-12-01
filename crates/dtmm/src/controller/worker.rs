use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::Help;
use color_eyre::Report;
use color_eyre::Result;
use druid::{ExtEventSink, SingleUse, Target};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;

use crate::controller::app::*;
use crate::controller::deploy::deploy_mods;
use crate::controller::game::*;
use crate::controller::import::*;
use crate::state::AsyncAction;
use crate::state::ACTION_FINISH_CHECK_UPDATE;
use crate::state::ACTION_FINISH_LOAD_INITIAL;
use crate::state::ACTION_FINISH_SAVE_SETTINGS;
use crate::state::ACTION_SHOW_ERROR_DIALOG;
use crate::state::{
    ACTION_FINISH_ADD_MOD, ACTION_FINISH_DELETE_SELECTED_MOD, ACTION_FINISH_DEPLOY,
    ACTION_FINISH_RESET_DEPLOYMENT, ACTION_LOG,
};

async fn send_error(sink: Arc<RwLock<ExtEventSink>>, err: Report) {
    sink.write()
        .await
        .submit_command(ACTION_SHOW_ERROR_DIALOG, SingleUse::new(err), Target::Auto)
        .expect("failed to send command");
}

async fn handle_action(
    event_sink: Arc<RwLock<ExtEventSink>>,
    action_queue: Arc<RwLock<UnboundedReceiver<AsyncAction>>>,
) {
    while let Some(action) = action_queue.write().await.recv().await {
        if cfg!(debug_assertions) && !matches!(action, AsyncAction::Log(_)) {
            tracing::debug!(?action);
        }

        let event_sink = event_sink.clone();
        match action {
            AsyncAction::DeployMods(state) => tokio::spawn(async move {
                if let Err(err) = deploy_mods(state).await.wrap_err("Failed to deploy mods") {
                    tracing::error!("{:?}", err);
                    send_error(event_sink.clone(), err).await;
                }

                event_sink
                    .write()
                    .await
                    .submit_command(ACTION_FINISH_DEPLOY, (), Target::Auto)
                    .expect("failed to send command");
            }),
            AsyncAction::AddMod(state, info) => tokio::spawn(async move {
                match import_from_file(state, info)
                    .await
                    .wrap_err("Failed to import mod")
                {
                    Ok(mod_info) => {
                        event_sink
                            .write()
                            .await
                            .submit_command(
                                ACTION_FINISH_ADD_MOD,
                                SingleUse::new(Arc::new(mod_info)),
                                Target::Auto,
                            )
                            .expect("failed to send command");
                    }
                    Err(err) => {
                        tracing::error!("{:?}", err);
                        send_error(event_sink.clone(), err).await;
                    }
                }
            }),
            AsyncAction::DeleteMod(state, info) => tokio::spawn(async move {
                let mod_dir = state.mod_dir.join(&info.id);
                if let Err(err) = delete_mod(state, &info)
                    .await
                    .wrap_err("Failed to delete mod files")
                    .with_suggestion(|| {
                        format!("Clean the folder '{}' manually", mod_dir.display())
                    })
                {
                    tracing::error!("{:?}", err);
                    send_error(event_sink.clone(), err).await;
                }

                event_sink
                    .write()
                    .await
                    .submit_command(
                        ACTION_FINISH_DELETE_SELECTED_MOD,
                        SingleUse::new(info),
                        Target::Auto,
                    )
                    .expect("failed to send command");
            }),
            AsyncAction::ResetDeployment(state) => tokio::spawn(async move {
                if let Err(err) = reset_mod_deployment(state)
                    .await
                    .wrap_err("Failed to reset mod deployment")
                {
                    tracing::error!("{:?}", err);
                    send_error(event_sink.clone(), err).await;
                }

                event_sink
                    .write()
                    .await
                    .submit_command(ACTION_FINISH_RESET_DEPLOYMENT, (), Target::Auto)
                    .expect("failed to send command");
            }),
            AsyncAction::SaveSettings(state) => tokio::spawn(async move {
                if let Err(err) = save_settings(state)
                    .await
                    .wrap_err("Failed to save settings")
                {
                    tracing::error!("{:?}", err);
                    send_error(event_sink.clone(), err).await;
                }

                event_sink
                    .write()
                    .await
                    .submit_command(ACTION_FINISH_SAVE_SETTINGS, (), Target::Auto)
                    .expect("failed to send command");
            }),
            AsyncAction::CheckUpdates(state) => tokio::spawn(async move {
                let updates = match check_updates(state)
                    .await
                    .wrap_err("Failed to check for updates")
                {
                    Ok(updates) => updates,
                    Err(err) => {
                        tracing::error!("{:?}", err);
                        send_error(event_sink.clone(), err).await;
                        vec![]
                    }
                };

                event_sink
                    .write()
                    .await
                    .submit_command(
                        ACTION_FINISH_CHECK_UPDATE,
                        SingleUse::new(updates),
                        Target::Auto,
                    )
                    .expect("failed to send command");
            }),
            AsyncAction::LoadInitial((path, is_default)) => tokio::spawn(async move {
                let data = match load_initial(path, is_default)
                    .await
                    .wrap_err("Failed to load initial application data")
                {
                    Ok(data) => Some(data),
                    Err(err) => {
                        tracing::error!("{:?}", err);
                        send_error(event_sink.clone(), err).await;
                        None
                    }
                };

                event_sink
                    .write()
                    .await
                    .submit_command(
                        ACTION_FINISH_LOAD_INITIAL,
                        SingleUse::new(data),
                        Target::Auto,
                    )
                    .expect("failed to send command");
            }),
            AsyncAction::Log((state, line)) => tokio::spawn(async move {
                if let Ok(mut f) = OpenOptions::new()
                    .append(true)
                    .open(state.data_dir.join("dtmm.log"))
                    .await
                {
                    let _ = f.write_all(&line).await;
                }
            }),
            AsyncAction::NxmDownload(state, uri) => tokio::spawn(async move {
                match import_from_nxm(state, uri)
                    .await
                    .wrap_err("Failed to handle NXM URI")
                {
                    Ok(mod_info) => {
                        event_sink
                            .write()
                            .await
                            .submit_command(
                                ACTION_FINISH_ADD_MOD,
                                SingleUse::new(Arc::new(mod_info)),
                                Target::Auto,
                            )
                            .expect("failed to send command");
                    }
                    Err(err) => {
                        tracing::error!("{:?}", err);
                        send_error(event_sink.clone(), err).await;
                    }
                }
            }),
        };
    }
}

async fn handle_log(
    event_sink: Arc<RwLock<ExtEventSink>>,
    log_queue: Arc<RwLock<UnboundedReceiver<Vec<u8>>>>,
) {
    while let Some(line) = log_queue.write().await.recv().await {
        let event_sink = event_sink.clone();
        event_sink
            .write()
            .await
            .submit_command(ACTION_LOG, SingleUse::new(line), Target::Auto)
            .expect("failed to send command");
    }
}

pub(crate) fn work_thread(
    event_sink: Arc<RwLock<ExtEventSink>>,
    action_queue: Arc<RwLock<UnboundedReceiver<AsyncAction>>>,
    log_queue: Arc<RwLock<UnboundedReceiver<Vec<u8>>>>,
) -> Result<()> {
    let rt = Runtime::new()?;

    rt.block_on(async {
        loop {
            tokio::select! {
                _ = handle_action(event_sink.clone(), action_queue.clone()) => {},
                _ = handle_log(event_sink.clone(), log_queue.clone()) => {},
            }
        }
    });

    Ok(())
}
