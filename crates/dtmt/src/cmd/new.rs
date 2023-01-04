use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;

pub(crate) fn _command_definition() -> Command {
    Command::new("new")
        .about("Create a new project")
        .arg(Arg::new("name").help(
            "The name of the new project. Will default to the name of the project's directory",
        ))
        .arg(Arg::new("directory").required(true).help(
            "The directory where to initialize the new project. This directory must be empty\
                        or must not exist. If `.` is given, the current directory will be used.",
        ))
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, _matches: &ArgMatches) -> Result<()> {
    unimplemented!()
}
