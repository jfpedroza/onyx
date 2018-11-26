extern crate quicli;
use quicli::prelude::*;
use std::path::PathBuf;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "Onix CLI",
    about = "CLI tool for configuring and running projects"
)]
pub struct CliArgs {
    #[structopt(flatten)]
    pub verbosity: Verbosity,

    /// The project file
    #[structopt(
        short = "p",
        long = "project",
        default_value = "onyx.yml",
        parse(from_os_str)
    )]
    pub project_file: PathBuf,

    #[structopt(subcommand)]
    pub cmd: CliCommand,
}

#[derive(Debug, StructOpt)]
pub enum CliCommand {
    /// Initialize an existing project with Onyx by creating an onyx.yml file
    #[structopt(name = "init")]
    Init {
        /// Name of the project. If passed, the file will be generated without prompting anything
        name: Option<String>,
    },

    /// Run the project
    #[structopt(name = "run")]
    Run { entries: Vec<String> },
}
