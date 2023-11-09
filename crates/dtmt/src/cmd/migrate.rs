use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::{self, Context};
use color_eyre::{Help, Report, Result};
use dtmt_shared::{ModConfig, ModConfigResources, ModDependency};
use futures::FutureExt;
use luajit2_sys as lua;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;

pub(crate) fn command_definition() -> Command {
    Command::new("migrate")
        .about("Migrate a mod project from the loose file structure to DTMT.")
        .arg(
            Arg::new("mod-file")
                .required(true)
                .value_parser(value_parser!(PathBuf))
                .help("The path to the mod's '<id>.mod' file."),
        )
        .arg(
            Arg::new("directory")
                .required(true)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The directory to create the mod in. Within this directory, \
                        DTMT will create a new folder named after the mod ID and migrate files \
                        into that folder.",
                ),
        )
}

#[derive(Clone, Debug)]
struct ModFile {
    id: String,
    init: PathBuf,
    data: Option<PathBuf>,
    localization: Option<PathBuf>,
}

// This piece of Lua code stubs DMF functions and runs a mod's `.mod` file to extract
// the contained information.
static MOD_FILE_RUNNER: &str = r#"
_DATA = {}

function fassert() end

function new_mod(id, options)
    _DATA.id = id
    _DATA.init = options.mod_script
    _DATA.data = options.mod_data
    _DATA.localization = options.mod_localization
end

dmf = {
    dofile = function(self, file)
        _DATA.init = file
    end
}

_MOD().run()
"#;

#[tracing::instrument]
async fn evaluate_mod_file(path: impl AsRef<Path> + std::fmt::Debug) -> Result<ModFile> {
    let path = path.as_ref();
    let code = fs::read(path)
        .await
        .wrap_err_with(|| format!("Failed to read file '{}'", path.display()))?;

    tokio::task::spawn_blocking(move || unsafe {
        let state = lua::luaL_newstate();
        lua::luaL_openlibs(state);

        let code = CString::new(code).expect("Cannot build CString");
        let name = CString::new("_MOD").expect("Cannot build CString");

        match lua::luaL_loadstring(state, code.as_ptr()) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRSYNTAX => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Invalid syntax: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory")
            }
            _ => unreachable!(),
        }

        tracing::trace!("Loaded '.mod' code");

        lua::lua_setglobal(state, name.as_ptr());

        let code = CString::new(MOD_FILE_RUNNER).expect("Cannot build CString");
        match lua::luaL_loadstring(state, code.as_ptr()) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRSYNTAX => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Invalid syntax: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory")
            }
            _ => unreachable!(),
        }

        match lua::lua_pcall(state, 0, 1, 0) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRRUN => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Failed to evaluate '.mod' file: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory")
            }
            // We don't use an error handler function, so this should be unreachable
            lua::LUA_ERRERR => unreachable!(),
            _ => unreachable!(),
        }

        tracing::trace!("Loaded file runner code");

        let name = CString::new("_DATA").expect("Cannot build CString");
        lua::lua_getglobal(state, name.as_ptr());

        let id = {
            let name = CString::new("id").expect("Cannot build CString");
            lua::lua_getfield(state, -1, name.as_ptr());
            let val = {
                let ptr = lua::lua_tostring(state, -1);
                let str = CStr::from_ptr(ptr);
                str.to_str()
                    .expect("ID value is not a valid string")
                    .to_string()
            };
            lua::lua_pop(state, 1);
            val
        };

        let path_prefix = format!("{id}/");

        let init = {
            let name = CString::new("init").expect("Cannot build CString");
            lua::lua_getfield(state, -1, name.as_ptr());
            let val = {
                let ptr = lua::lua_tostring(state, -1);
                let str = CStr::from_ptr(ptr);
                str.to_str().expect("ID value is not a valid string")
            };
            lua::lua_pop(state, 1);
            PathBuf::from(val.strip_prefix(&path_prefix).unwrap_or(val))
        };

        let data = {
            let name = CString::new("data").expect("Cannot build CString");
            lua::lua_getfield(state, -1, name.as_ptr());

            if lua::lua_isnil(state, -1) > 0 {
                None
            } else {
                let val = {
                    let ptr = lua::lua_tostring(state, -1);
                    let str = CStr::from_ptr(ptr);
                    str.to_str().expect("ID value is not a valid string")
                };
                lua::lua_pop(state, 1);
                Some(PathBuf::from(val.strip_prefix(&path_prefix).unwrap_or(val)))
            }
        };

        let localization = {
            let name = CString::new("localization").expect("Cannot build CString");
            lua::lua_getfield(state, -1, name.as_ptr());

            if lua::lua_isnil(state, -1) > 0 {
                None
            } else {
                let val = {
                    let ptr = lua::lua_tostring(state, -1);
                    let str = CStr::from_ptr(ptr);
                    str.to_str().expect("ID value is not a valid string")
                };
                lua::lua_pop(state, 1);
                Some(PathBuf::from(val.strip_prefix(&path_prefix).unwrap_or(val)))
            }
        };

        lua::lua_close(state);

        let mod_file = ModFile {
            id,
            init,
            data,
            localization,
        };

        tracing::trace!(?mod_file);

        Ok(mod_file)
    })
    .await
    .map_err(Report::new)
    .flatten()
    .wrap_err("Failed to run mod file handler")
}

#[async_recursion::async_recursion]
#[tracing::instrument]
async fn process_directory<P1, P2>(path: P1, prefix: P2) -> Result<()>
where
    P1: AsRef<Path> + std::fmt::Debug + std::marker::Send,
    P2: AsRef<Path> + std::fmt::Debug + std::marker::Send,
{
    let path = path.as_ref();
    let prefix = prefix.as_ref();

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

        if t.is_dir() {
            process_directory(in_path, out_path).await?;
        } else {
            tracing::trace!(
                "Copying file '{}' -> '{}'",
                in_path.display(),
                out_path.display()
            );
            let res = fs::create_dir_all(prefix)
                .then(|_| fs::copy(&in_path, &out_path))
                .await
                .wrap_err_with(|| {
                    format!(
                        "Failed to copy '{}' -> '{}'",
                        in_path.display(),
                        out_path.display()
                    )
                });
            if let Err(err) = res {
                tracing::error!("{:?}", err);
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let (mod_file, in_dir) = {
        let path = matches
            .get_one::<PathBuf>("mod-file")
            .expect("Parameter is required");

        let mod_file = evaluate_mod_file(&path)
            .await
            .wrap_err("Failed to evaluate '.mod' file")?;

        (
            mod_file,
            path.parent().expect("A file path always has a parent"),
        )
    };

    let out_dir = matches
        .get_one::<PathBuf>("directory")
        .expect("Parameter is required");

    {
        let is_dir = fs::metadata(out_dir)
            .await
            .map(|meta| meta.is_dir())
            .unwrap_or(false);

        if !is_dir {
            let err = eyre::eyre!("Invalid output directory '{}'", out_dir.display());
            return Err(err)
                .with_suggestion(|| "Make sure the directory exists and is writable.".to_string());
        }
    }

    let out_dir = out_dir.join(&mod_file.id);

    fs::create_dir(&out_dir)
        .await
        .wrap_err_with(|| format!("Failed to create mod directory '{}'", out_dir.display()))?;

    tracing::info!("Created mod directory '{}'", out_dir.display());

    println!(
        "Enter additional information about your mod '{}'!",
        &mod_file.id
    );

    let name = promptly::prompt_default("Display name", mod_file.id.clone())
        .map(|s: String| s.trim().to_string())?;
    let summary = promptly::prompt("Short summary").map(|s: String| s.trim().to_string())?;
    let author =
        promptly::prompt_opt("Author").map(|opt| opt.map(|s: String| s.trim().to_string()))?;
    let version = promptly::prompt_default("Version", String::from("0.1.0"))
        .map(|s: String| s.trim().to_string())?;
    let categories = promptly::prompt("Categories (comma separated list)")
        .map(|s: String| s.trim().to_string())
        .map(|s: String| s.split(',').map(|s| s.trim().to_string()).collect())?;

    let packages = vec![PathBuf::from("packages/mods").join(&mod_file.id)];

    let dtmt_cfg = ModConfig {
        dir: out_dir,
        id: mod_file.id,
        name,
        summary,
        author,
        version,
        description: None,
        image: None,
        categories,
        packages,
        resources: ModConfigResources {
            init: mod_file.init,
            data: mod_file.data,
            localization: mod_file.localization,
        },
        depends: vec![ModDependency::ID(String::from("DMF"))],
        bundled: true,
    };

    tracing::debug!(?dtmt_cfg);

    {
        let path = dtmt_cfg.dir.join("dtmt.cfg");
        let data = serde_sjson::to_string(&dtmt_cfg).wrap_err("Failed to serialize dtmt.cfg")?;
        fs::write(&path, &data)
            .await
            .wrap_err_with(|| format!("Failed to write '{}'", path.display()))?;

        tracing::info!("Created mod configuration at '{}'", path.display());
    }

    {
        let path = dtmt_cfg
            .dir
            .join(&dtmt_cfg.packages[0])
            .with_extension("package");

        let data = {
            let mut map = HashMap::new();
            map.insert("lua", vec![format!("scripts/mods/{}/*", dtmt_cfg.id)]);
            map
        };
        let data = serde_sjson::to_string(&data).wrap_err("Failed to serialize package file")?;

        fs::create_dir_all(path.parent().unwrap())
            .then(|_| fs::write(&path, &data))
            .await
            .wrap_err_with(|| format!("Failed to write '{}'", path.display()))?;

        tracing::info!("Created package file at '{}'", path.display());
    }

    {
        let path = in_dir.join("scripts");
        let scripts_dir = dtmt_cfg.dir.join("scripts");
        process_directory(&path, &scripts_dir)
            .await
            .wrap_err_with(|| {
                format!(
                    "Failed to copy files from '{}' to '{}'",
                    path.display(),
                    scripts_dir.display()
                )
            })?;

        tracing::info!("Copied script files to '{}'", scripts_dir.display());
    }

    Ok(())
}
