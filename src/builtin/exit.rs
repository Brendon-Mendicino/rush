use crate::builtin::{Builtin, BuiltinResult, Status};
use clap::Parser;

/// Ma io che ne so
#[derive(Debug, Parser)]
struct Args {
    /// The code
    code: Option<i32>,
}

pub fn exit(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    let code = args.code.unwrap_or(0);

    // The executor leaves the process: the shell, or the child a pipeline
    // command or a subshell runs in.
    Ok(Status::Exit(code))
}
