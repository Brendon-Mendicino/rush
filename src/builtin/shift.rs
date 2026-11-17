//! `shift` - shift positional parameters (special built-in).
//!
//! ```text
//! shift [n]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Shift positional parameters.
///
/// Drop the first N positional parameters, 1 without N: $N+1 becomes $1, and
/// $# goes down by N. With N greater than $#, nothing changes and it fails.
///
/// Exit status:
///   0   success
///   >0  N is greater than $#
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "shift [n]")]
pub(super) struct Args {
    /// How many to drop.
    n: Option<usize>,
}

pub fn shift(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;
    let n = args.n.unwrap_or(1);

    // An error of a special built-in: it exits a script, like in dash.
    let positional = &mut bi.shell.positional;
    if n > positional.len() {
        return Err(format!("{n}: shift count out of range").into());
    }

    positional.drain(..n);
    Ok(Status::SUCCESS)
}
