//! `exec` - execute commands and open, close, or copy file descriptors (special
//! built-in).
//!
//! ```text
//! exec [command [argument...]]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Replace the shell with a command, or make redirections permanent.
///
/// Replace the shell with COMMAND, without creating a new process. Without
/// COMMAND, the redirections of exec stay in the shell: exec 3<file,
/// exec >log, exec 3>&-. There are no options; a leading -- is skipped, for a
/// command starting with -.
///
/// Exit status:
///   0    without COMMAND, the redirections were performed
///   126  COMMAND was found but could not be run
///   127  COMMAND was not found
///   >0   a redirection error
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "exec [command [argument...]]")]
pub(super) struct Args {}

/// Replace the shell with `command`, without creating a new process. Without a
/// command, the redirections of `exec` are made permanent in the shell:
/// `exec 3<file`, `exec >log`, `exec 3>&-`. Both are carried out by the
/// executor, which has the redirections and the `VAR=x` prefixes.
///
/// There are no options. A leading `--` is skipped, like in other shells, for a
/// command starting with `-`.
pub fn exec(bi: &mut Builtin) -> BuiltinResult {
    let argv = match bi.args.get(1) {
        Some(&"--") => &bi.args[2..],
        _ => &bi.args[1..],
    };

    Ok(Status::Exec(
        argv.iter().map(|arg| arg.to_string()).collect(),
    ))
}
