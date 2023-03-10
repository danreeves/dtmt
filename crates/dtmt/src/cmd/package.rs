use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::{Context, Result};
use color_eyre::Help;
use dtmt_shared::ModConfig;
use path_slash::{PathBufExt, PathExt};
use tokio::fs;
use tokio::sync::Mutex;
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
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to write the packaged file to. Will default to a file in the \
                        current working directory",
                ),
        )
}

#[async_recursion::async_recursion]
async fn process_directory<P1, P2, W>(
    zip: Arc<Mutex<ZipWriter<W>>>,
    path: P1,
    prefix: P2,
) -> Result<()>
where
    P1: AsRef<Path> + std::marker::Send,
    P2: AsRef<Path> + std::marker::Send,
    W: std::io::Write + std::io::Seek + std::marker::Send,
{
    let path = path.as_ref();
    let prefix = prefix.as_ref();

    zip.lock()
        .await
        .add_directory(prefix.to_slash_lossy(), Default::default())?;

    let read_dir = fs::read_dir(&path)
        .await
        .wrap_err_with(|| format!("Failed to read directory '{}'", path.display()))?;

    let stream = ReadDirStream::new(read_dir).map(|res| res.wrap_err("Failed to read dir entry"));
    tokio::pin!(stream);

    while let Some(res) = stream.next().await {
        let entry = res?;
        let in_path = entry.path();
        let out_path = prefix.join(entry.file_name());

        let t = entry.file_type().await?;

        if t.is_file() || t.is_symlink() {
            let data = fs::read(&in_path)
                .await
                .wrap_err_with(|| format!("Failed to read '{}'", in_path.display()))?;
            {
                let mut zip = zip.lock().await;
                zip.start_file(out_path.to_slash_lossy(), Default::default())?;
                zip.write_all(&data)?;
            }
        } else if t.is_dir() {
            process_directory(zip.clone(), in_path, out_path).await?;
        }
    }

    Ok(())
}

pub(crate) async fn package<P1, P2>(cfg: &ModConfig, path: P1, dest: P2) -> Result<()>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    let path = path.as_ref();
    let dest = dest.as_ref();

    let data = Cursor::new(Vec::new());
    let zip = ZipWriter::new(data);
    let zip = Arc::new(Mutex::new(zip));

    process_directory(zip.clone(), path, PathBuf::from(&cfg.id))
        .await
        .wrap_err("Failed to add directory to archive")?;

    let mut zip = zip.lock().await;

    {
        let name = PathBuf::from(&cfg.id).join("dtmt.cfg");
        let path = cfg.dir.join("dtmt.cfg");

        let data = fs::read(&path)
            .await
            .wrap_err_with(|| format!("Failed to read mod config at {}", path.display()))?;

        zip.start_file(name.to_slash_lossy(), Default::default())?;
        zip.write_all(&data)?;
    }

    let data = zip.finish()?;

    fs::write(dest, data.into_inner())
        .await
        .wrap_err_with(|| format!("Failed to write mod archive to '{}'", dest.display()))
        .with_suggestion(|| "Make sure that parent directories exist.".to_string())?;

    tracing::info!("Mod archive written to {}", dest.display());
    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let cfg = read_project_config(matches.get_one::<PathBuf>("project").cloned()).await?;

    let dest = matches
        .get_one::<PathBuf>("out")
        .map(path_clean::clean)
        .unwrap_or_else(|| PathBuf::from(format!("{}.zip", cfg.id)));

    let path = cfg.dir.join(
        matches
            .get_one::<PathBuf>("directory")
            .expect("parameter has default value"),
    );

    package(&cfg, path, dest).await
}
