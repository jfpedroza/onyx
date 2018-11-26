#[macro_use]
extern crate quicli;
extern crate onyx;
use onyx::*;
use quicli::prelude::*;

main!(|args: CliArgs, log_level: verbosity| {
    process(&args)?;
});
