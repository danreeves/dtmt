use std::collections::HashMap;
use std::path::PathBuf;

use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::Help;
use futures::{StreamExt, TryStreamExt};
use string_template::Template;
use tokio::fs::{self, DirBuilder};

const TEMPLATES: [(&str, &str); 5] = [
    (
        "dtmt.cfg",
        r#"name = "{{name}}"
description = "An elaborate description of my cool game mod!"
version = "0.1.0"

resources = {
    script = "scripts/mods/{{name}}/init"
    data = "scripts/mods/{{name}}/data"
    localization = "scripts/mods/{{name}}/locationzation"
}

packages = [
    "packages/{{name}}"
]

depends = [
    "dmf"
]
"#,
    ),
    (
        "packages/{{name}}.package",
        r#"lua = [
    "scripts/mods/{{name}}/*"
]
"#,
    ),
    (
        "scripts/mods/{{name}}/init.lua",
        r#"local mod = get_mod("{{name}}")

-- Your mod code goes here.
-- https://vmf-docs.verminti.de
"#,
    ),
    (
        "scripts/mods/{{name}}/data.lua",
        r#"local mod = get_mod("{{name}}")

return {
	name = "{{title}}",
	description = mod:localize("mod_description"),
	is_togglable = true,
}"#,
    ),
    (
        "scripts/mods/{{name}}/localization.lua",
        r#"return {
	mod_description = {
		en = "An elaborate description of my cool game mod!",
	},
}"#,
    ),
];

pub(crate) fn command_definition() -> Command {
    Command::new("new")
        .about("Create a new project")
        .arg(
            Arg::new("title")
                .long("title")
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
                .wrap_err("the current working dir is invalid")
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

    let title = if let Some(title) = matches.get_one::<String>("title") {
        title.clone()
    } else {
        promptly::prompt("The mod display name")?
    };

    let name = {
        let default = title
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        promptly::prompt_default("The mod identifier name", default)?
    };

    tracing::debug!(root = %root.display(), title, name);

    let mut data = HashMap::new();
    data.insert("name", name.as_str());
    data.insert("title", title.as_str());

    let templates = TEMPLATES
        .iter()
        .map(|(path_tmpl, content_tmpl)| {
            let path = Template::new(path_tmpl).render(&data);
            let content = Template::new(content_tmpl).render(&data);

            (root.join(path), content)
        })
        .map(|(path, content)| async move {
            let dir = path
                .parent()
                .ok_or_else(|| eyre::eyre!("invalid root path"))?;

            DirBuilder::new()
                .recursive(true)
                .create(&dir)
                .await
                .wrap_err_with(|| format!("failed to create directory {}", dir.display()))?;

            tracing::trace!("Writing file {}", path.display());

            fs::write(&path, content.as_bytes())
                .await
                .wrap_err_with(|| format!("failed to write content to path {}", path.display()))
        });

    futures::stream::iter(templates)
        .buffer_unordered(10)
        .try_fold((), |_, _| async { Ok(()) })
        .await?;

    tracing::info!(
        "Created {} files for mod '{}' in '{}'.",
        TEMPLATES.len(),
        title,
        root.display()
    );

    Ok(())
}
