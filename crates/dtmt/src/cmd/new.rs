use std::path::PathBuf;

use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::Help;
use futures::{StreamExt, TryStreamExt};
use minijinja::Environment;
use tokio::fs::{self, DirBuilder};

const TEMPLATES: [(&str, &str); 5] = [
    (
        "dtmt.cfg",
        r#"id = "{{id}}"
name = "{{name}}"
version = "0.1.0"
// author = ""

// A one- or two-line short description.
summary = "This is my new mod '{{name}}'!"
// description = ""
// image = "assets/logo.png"

// Can contain arbitrary strings. But to keep things consistent and useful,
// capitalize names and check existing mods for matching categories.
categories = [
    Misc
    // UI
    // QoL
    // Tools
]

// A list of mod IDs that this mod depends on. You can find
// those IDs by downloading the mod and extracting their `dtmt.cfg`.
depends = [
    DMF
]

resources = {
    init = "scripts/mods/{{id}}/init"
    data = "scripts/mods/{{id}}/data"
    localization = "scripts/mods/{{id}}/localization"
}

packages = [
    "packages/mods/{{id}}"
]
"#,
    ),
    (
        "packages/mods/{{id}}.package",
        r#"lua = [
    "scripts/mods/{{id}}/*"
]
"#,
    ),
    (
        "scripts/mods/{{id}}/init.lua",
        r#"local mod = get_mod("{{id}}")

-- Your mod code goes here.
-- https://vmf-docs.verminti.de
"#,
    ),
    (
        "scripts/mods/{{id}}/data.lua",
        r#"local mod = get_mod("{{id}}")

return {
	name = "{{name}}",
	description = mod:localize("mod_description"),
	is_togglable = true,
}"#,
    ),
    (
        "scripts/mods/{{id}}/localization.lua",
        r#"return {
	mod_description = {
		en = "This is my new mod '{{name}}'!",
	},
}"#,
    ),
];

pub(crate) fn command_definition() -> Command {
    Command::new("new")
        .about("Create a new project")
        .arg(
            Arg::new("name")
                .long("name")
                .help("The display name of the new mod."),
        )
        .arg(Arg::new("root").help(
            "The directory where to initialize the new project. This directory must be empty \
                or must not exist. If omitted or `.` is given, the current directory \
                will be used.",
        ))
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let root = if let Some(dir) = matches.get_one::<String>("root") {
        if dir == "." {
            std::env::current_dir()
                .wrap_err("The current working dir is invalid")
                .with_suggestion(|| "Change to a different directory.")?
        } else {
            PathBuf::from(dir)
        }
    } else {
        let prompt = "The mod directory";
        match std::env::current_dir() {
            Ok(default) => promptly::prompt_default(prompt, default)?,
            Err(_) => promptly::prompt(prompt)?,
        }
    };

    let name = if let Some(name) = matches.get_one::<String>("name") {
        name.clone()
    } else {
        promptly::prompt("The display name")?
    };

    let id = {
        let default = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        promptly::prompt_default("The unique mod ID", default)?
    };

    tracing::debug!(root = %root.display(), name, id);

    let render_ctx = minijinja::context!(name => name.as_str(), id => id.as_str());
    let env = Environment::new();

    let templates = TEMPLATES
        .iter()
        .map(|(path_tmpl, content_tmpl)| {
            env.render_str(path_tmpl, &render_ctx)
                .wrap_err_with(|| format!("Failed to render template: {}", path_tmpl))
                .and_then(|path| {
                    env.render_named_str(&path, content_tmpl, &render_ctx)
                        .wrap_err_with(|| format!("Failed to render template '{}'", &path))
                        .map(|content| (root.join(path), content))
                })
        })
        .map(|res| async move {
            match res {
                Ok((path, content)) => {
                    let dir = path
                        .parent()
                        .ok_or_else(|| eyre::eyre!("invalid root path"))?;

                    DirBuilder::new()
                        .recursive(true)
                        .create(&dir)
                        .await
                        .wrap_err_with(|| {
                            format!("Failed to create directory {}", dir.display())
                        })?;

                    tracing::trace!("Writing file {}", path.display());

                    fs::write(&path, content.as_bytes())
                        .await
                        .wrap_err_with(|| {
                            format!("Failed to write content to path {}", path.display())
                        })
                }
                Err(e) => Err(e),
            }
        });

    futures::stream::iter(templates)
        .buffer_unordered(10)
        .try_fold((), |_, _| async { Ok(()) })
        .await?;

    tracing::info!(
        "Created {} files for mod '{}' in '{}'.",
        TEMPLATES.len(),
        name,
        root.display()
    );

    Ok(())
}
