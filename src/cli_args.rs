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
    /// Initializes an existing project with Onyx by creating an onyx.yml file
    #[structopt(name = "init")]
    Init {
        /// Name of the project. If passed, the file will be generated without prompting anything
        name: Option<String>,
    },

    /// Runs the project
    #[structopt(name = "run")]
    Run { entries: Vec<String> },

    /// Reads configuration entries and prints them
    #[structopt(name = "config")]
    Config {
        /// Application to read config from. Ingored if umbrella = false
        /// If not provided, all apps will be taken into a account and `app: value` pairs will be returned
        #[structopt(short = "a", long = "app")]
        app: Option<String>,

        /// The key to search
        key: String,

        #[structopt(name = "sub-key")]
        /// The subkey to search. Raises error if `key` doesn't contain key-value pairs
        sub_key: Option<String>,
    },
}
