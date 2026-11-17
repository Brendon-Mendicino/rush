//! `exit` - cause the shell to exit (special built-in).
//!
//! ```text
//! exit [n]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Exit the shell.
///
/// Exit the shell with status N, or with the status of the last command ($?)
/// without it. In a subshell, a command of a pipeline or a command
/// substitution, only that subshell exits.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "exit [n]")]
pub(super) struct Args {
    /// The exit status.
    n: Option<i32>,
}

pub fn exit(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    let code = args.n.unwrap_or(bi.shell.last_status);

    // The executor leaves the process: the shell, or the child a pipeline
    // command or a subshell runs in.
    Ok(Status::Exit(code))
}
