extern crate failure;
extern crate promptly;
extern crate quicli;
extern crate serde;
extern crate serde_derive;
extern crate serde_yaml;
extern crate void;
use quicli::prelude::*;

mod cli_args;
mod project;
pub use cli_args::*;
use project::*;

pub fn process(args: &CliArgs) -> Result<()> {
    debug!("Processed args: {:#?}", &args);
    let load = || -> Result<Project> {
        let project = Project::load(&args.project_file)?;
        debug!("Project: {:#?}", &project);
        let merged = project.merge()?;
        debug!("Merged: {:#?}", &merged);
        Ok(merged)
    };

    match args.cmd {
        CliCommand::Init { ref name } => {
            init(&args.project_file, &name)?;
        }
        CliCommand::Run { ref entries } => {
            let project = load()?;
            let to_run = project.runner.entries_to_run(entries)?;
            debug!("To run {:?}", to_run);
        }
    }

    Ok(())
}
