#![recursion_limit = "256"]
#![feature(let_chains)]
#![feature(iterator_try_collect)]
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::Arc;

use clap::parser::ValueSource;
use clap::{command, value_parser, Arg};
use color_eyre::eyre::{self, Context};
use color_eyre::{Report, Result, Section};
use druid::AppLauncher;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use tokio::sync::RwLock;

use crate::controller::worker::work_thread;
use crate::state::{AsyncAction, ACTION_HANDLE_NXM};
use crate::state::{Delegate, State};
use crate::ui::theme;
use crate::util::log::LogLevel;

mod controller;
mod state;
mod util {
    pub mod ansi;
    pub mod config;
    pub mod log;
}
mod ui;

// As explained in https://docs.rs/interprocess/2.1.0/interprocess/local_socket/struct.Name.html
// namespaces are supported on both platforms we care about: Windows and Linux.
const IPC_ADDRESS: &str = "dtmm.sock";

#[tracing::instrument]
fn notify_nxm_download(
    uri: impl AsRef<str> + std::fmt::Debug,
    level: Option<LogLevel>,
) -> Result<()> {
    util::log::create_tracing_subscriber(level, None);

    tracing::debug!("Received Uri '{}', sending to main process.", uri.as_ref());

    let mut stream = LocalSocketStream::connect(
        IPC_ADDRESS
            .to_ns_name::<GenericNamespaced>()
            .expect("Invalid socket name"),
    )
    .wrap_err_with(|| format!("Failed to connect to '{}'", IPC_ADDRESS))
    .suggestion("Make sure the main window is open.")?;

    tracing::debug!("Connected to main process at '{}'", IPC_ADDRESS);

    let bincode_config = bincode::config::standard();

    bincode::encode_into_std_write(uri.as_ref(), &mut stream, bincode_config)
        .wrap_err("Failed to send URI")?;

    // We don't really care what the message is, we just need an acknowledgement.
    let _: String = bincode::decode_from_std_read(&mut stream, bincode_config)
        .wrap_err("Failed to receive reply")?;

    tracing::info!(
        "Notified DTMM with uri '{}'. Check the main window.",
        uri.as_ref()
    );
    Ok(())
}

#[tracing::instrument]
fn main() -> Result<()> {
    color_eyre::install()?;

    let default_config_path = util::config::get_default_config_path();

    tracing::trace!(default_config_path = %default_config_path.display());

    let matches = command!()
        .arg(
            Arg::new("config")
                .long("config")
                .short('c')
                .help("Path to the config file")
                .value_parser(value_parser!(PathBuf))
                .default_value(default_config_path.to_string_lossy().to_string()),
        )
        .arg(
            Arg::new("log-level")
                .long("log-level")
                .help("The maximum level of log events to print")
                .value_parser(value_parser!(LogLevel))
                .default_value("info"),
        )
        .arg(
            Arg::new("nxm")
                .help("An `nxm://` URI to download")
                .required(false),
        )
        .get_matches();

    let level = if matches.value_source("log-level") == Some(ValueSource::DefaultValue) {
        None
    } else {
        matches.get_one::<LogLevel>("log-level").cloned()
    };

    if let Some(uri) = matches.get_one::<String>("nxm") {
        return notify_nxm_download(uri, level).wrap_err("Failed to send NXM Uri to main window.");
    }

    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    util::log::create_tracing_subscriber(level, Some(log_tx));

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();

    let config_path = matches
        .get_one::<PathBuf>("config")
        .cloned()
        .expect("argument has default value");
    let is_config_default = matches.value_source("config") == Some(ValueSource::DefaultValue);
    if action_tx
        .send(AsyncAction::LoadInitial((config_path, is_config_default)))
        .is_err()
    {
        let err = eyre::eyre!("Failed to send action");
        return Err(err);
    }

    let launcher = AppLauncher::with_window(ui::window::main::new())
        .delegate(Delegate::new(action_tx))
        .configure_env(theme::set_theme_env);

    let event_sink = launcher.get_external_handle();

    {
        let span = tracing::info_span!(IPC_ADDRESS, "nxm-socket");
        let _guard = span.enter();

        let event_sink = event_sink.clone();
        let server = ListenerOptions::new()
            .name(
                IPC_ADDRESS
                    .to_ns_name::<GenericNamespaced>()
                    .expect("Invalid socket name"),
            )
            .create_sync()
            .wrap_err("Failed to create IPC listener")?;

        tracing::debug!("IPC server listening on '{}'", IPC_ADDRESS);

        // Drop the guard here, so that we can re-enter the same span in the thread.
        drop(_guard);

        std::thread::Builder::new()
            .name("nxm-socket".into())
            .spawn(move || {
                let _guard = span.enter();

                loop {
                    let res = server.accept().wrap_err_with(|| {
                        format!("IPC server failed to listen on '{}'", IPC_ADDRESS)
                    });

                    match res {
                        Ok(mut stream) => {
                            let res = bincode::decode_from_std_read(
                                &mut stream,
                                bincode::config::standard(),
                            )
                            .wrap_err("Failed to read message")
                            .and_then(|uri: String| {
                                tracing::trace!(uri, "Received NXM uri");

                                event_sink
                                    .submit_command(ACTION_HANDLE_NXM, uri, druid::Target::Auto)
                                    .wrap_err("Failed to start NXM download")
                            });
                            match res {
                                Ok(()) => {
                                    let _ = bincode::encode_into_std_write(
                                        "Ok",
                                        &mut stream,
                                        bincode::config::standard(),
                                    );
                                }
                                Err(err) => {
                                    tracing::error!("{:?}", err);
                                    let _ = bincode::encode_into_std_write(
                                        "Error",
                                        &mut stream,
                                        bincode::config::standard(),
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!("Failed to receive client connection: {:?}", err)
                        }
                    }
                }
            })
            .wrap_err("Failed to create thread")?;
    }

    std::thread::Builder::new()
        .name("work-thread".into())
        .spawn(move || {
            let event_sink = Arc::new(RwLock::new(event_sink));
            let action_rx = Arc::new(RwLock::new(action_rx));
            let log_rx = Arc::new(RwLock::new(log_rx));
            loop {
                if let Err(err) = work_thread(event_sink.clone(), action_rx.clone(), log_rx.clone())
                {
                    tracing::error!("Work thread failed, restarting: {:?}", err);
                }
            }
        })
        .wrap_err("Failed to create thread")?;

    launcher.launch(State::new()).map_err(Report::new)
}
