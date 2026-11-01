//! `exec` - execute commands and open, close, or copy file descriptors (special
//! built-in).
//!
//! ```text
//! exec [command [argument...]]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use crate::builtin::{Builtin, BuiltinResult, Status};

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
