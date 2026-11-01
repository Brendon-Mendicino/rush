use crate::builtin::{Builtin, BuiltinResult};
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

    // TODO: return `Status::Exit(code)` once the executor unwinds on it.
    std::process::exit(code);
}
