use std::ffi::OsString;
use std::io::{Cursor, Write};
use std::path::PathBuf;

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::{Context, Result};
use color_eyre::Help;
use path_slash::PathBufExt;
use tokio::fs::{self, DirEntry};
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;
use zip::ZipWriter;

use crate::cmd::build::read_project_config;

pub(crate) fn command_definition() -> Command {
    Command::new("package")
        .about("Package compiled bundles for distribution")
        .arg(
            Arg::new("project")
                .required(false)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to the project to build. \
                        If omitted, dtmt will search from the current working directory upward.",
                ),
        )
        .arg(
            Arg::new("directory")
                .long("directory")
                .short('d')
                .default_value("out")
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to the directory were the compiled bundles were written to. \
                        This is the same directory as `dtmt build -o`",
                ),
        )
        .arg(
            Arg::new("out")
                .long("out")
                .short('o')
                .default_value(".")
                .value_parser(value_parser!(PathBuf))
                .help("The path to write the packaged file to. May be a directory or a file name."),
        )
}

async fn process_dir_entry(res: Result<DirEntry>) -> Result<(OsString, Vec<u8>)> {
    let entry = res?;
    let path = entry.path();

    let data = fs::read(&path)
        .await
        .wrap_err_with(|| format!("failed to read '{}'", path.display()))?;

    Ok((entry.file_name(), data))
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let cfg = read_project_config(matches.get_one::<PathBuf>("project").cloned()).await?;

    let dest = {
        let mut path = matches
            .get_one::<PathBuf>("out")
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."));

        if path.extension().is_none() {
            path.push(format!("{}.zip", cfg.id))
        }

        path
    };

    let data = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(data);

    zip.add_directory(&cfg.id, Default::default())?;

    let base_path = PathBuf::from(cfg.id);

    {
        let name = base_path.join("dtmt.cfg");
        let path = cfg.dir.join("dtmt.cfg");

        let data = fs::read(&path)
            .await
            .wrap_err_with(|| format!("failed to read mod config at {}", path.display()))?;

        zip.start_file(name.to_slash_lossy(), Default::default())?;
        zip.write_all(&data)?;
    }

    {
        let path = cfg.dir.join(
            matches
                .get_one::<PathBuf>("directory")
                .expect("parameter has default value"),
        );
        let read_dir = fs::read_dir(&path)
            .await
            .wrap_err_with(|| format!("failed to read directory '{}'", path.display()))?;

        let stream = ReadDirStream::new(read_dir)
            .map(|res| res.wrap_err("failed to read dir entry"))
            .then(process_dir_entry);
        tokio::pin!(stream);

        while let Some(res) = stream.next().await {
            let (name, data) = res?;

            let name = base_path.join(name);
            zip.start_file(name.to_slash_lossy(), Default::default())?;
            zip.write_all(&data)?;
        }
    };

    let data = zip.finish()?;

    fs::write(&dest, data.into_inner())
        .await
        .wrap_err_with(|| format!("failed to write mod archive to '{}'", dest.display()))
        .with_suggestion(|| "Make sure that parent directories exist.".to_string())?;

    tracing::info!("Mod archive written to {}", dest.display());
    Ok(())
}
