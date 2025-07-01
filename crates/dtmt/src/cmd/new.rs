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
        r#"//
// This is your mod's main configuration file. It tells DTMT how to build the mod,
// and DTMM what to display to your users.
// Certain files have been pre-filled by the template, the ones commented out (`//`)
// are optional.
//
// A unique identifier (preferably lower case, alphanumeric)
id = "{{id}}"
// The display name that your users will see.
// This doesn't have to be unique, but you still want to avoid being confused with other
// mods.
name = "{{name}}"
// It's good practice to increase this number whenever you publish changes.
// It's up to you if you use SemVer or something simpler like `1970-12-24`. It should sort and
// compare well, though.
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
// To make your fellow modders' lives easier, publish your own mods' IDs
// somewhere visible, such as the Nexusmods page.
depends = [
    DMF
]

// The primary resources that serve as the entry point to your
// mod's code. Unless for very specific use cases, the generated
// values shouldn't be changed.
resources = {
    init = "scripts/mods/{{id}}/init"
    data = "scripts/mods/{{id}}/data"
    localization = "scripts/mods/{{id}}/localization"
}

// The list of packages, or bundles, to build.
// Each one corresponds to a package definition in the named folder.
// For mods that contain only code and/or a few small assets, a single
// package will suffice.
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
                .wrap_err_with(|| format!("Failed to render template: {path_tmpl}"))
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
