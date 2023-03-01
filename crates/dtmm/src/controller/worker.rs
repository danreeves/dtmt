use std::sync::Arc;

use color_eyre::Result;
use druid::{ExtEventSink, SingleUse, Target};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;

use crate::controller::app::*;
use crate::controller::game::*;
use crate::state::AsyncAction;
use crate::state::{
    ACTION_FINISH_ADD_MOD, ACTION_FINISH_DELETE_SELECTED_MOD, ACTION_FINISH_DEPLOY,
    ACTION_FINISH_RESET_DEPLOYMENT, ACTION_LOG,
};

async fn handle_action(
    event_sink: Arc<RwLock<ExtEventSink>>,
    action_queue: Arc<RwLock<UnboundedReceiver<AsyncAction>>>,
) {
    while let Some(action) = action_queue.write().await.recv().await {
        let event_sink = event_sink.clone();
        match action {
            AsyncAction::DeployMods(state) => tokio::spawn(async move {
                if let Err(err) = deploy_mods(state).await {
                    tracing::error!("Failed to deploy mods: {:?}", err);
                }

                event_sink
                    .write()
                    .await
                    .submit_command(ACTION_FINISH_DEPLOY, (), Target::Auto)
                    .expect("failed to send command");
            }),
            AsyncAction::AddMod((state, info)) => tokio::spawn(async move {
                match import_mod(state, info).await {
                    Ok(mod_info) => {
                        event_sink
                            .write()
                            .await
                            .submit_command(
                                ACTION_FINISH_ADD_MOD,
                                SingleUse::new(mod_info),
                                Target::Auto,
                            )
                            .expect("failed to send command");
                    }
                    Err(err) => {
                        tracing::error!("Failed to import mod: {:?}", err);
                    }
                }
            }),
            AsyncAction::DeleteMod((state, info)) => tokio::spawn(async move {
                if let Err(err) = delete_mod(state, &info).await {
                    tracing::error!(
                        "Failed to delete mod files. \
                                You might want to clean up the data directory manually. \
                                Reason: {:?}",
                        err
                    );
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
                if let Err(err) = reset_mod_deployment(state).await {
                    tracing::error!("Failed to reset mod deployment: {:?}", err);
                }

                event_sink
                    .write()
                    .await
                    .submit_command(ACTION_FINISH_RESET_DEPLOYMENT, (), Target::Auto)
                    .expect("failed to send command");
            }),
        };
    }
}

async fn handle_log(
    event_sink: Arc<RwLock<ExtEventSink>>,
    log_queue: Arc<RwLock<UnboundedReceiver<String>>>,
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
    log_queue: Arc<RwLock<UnboundedReceiver<String>>>,
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
