//! `eval` - construct command by concatenating arguments (special built-in).
//!
//! ```text
//! eval [argument...]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Run the arguments as shell code.
///
/// Join the arguments with single spaces and let the shell read and run the
/// result in the current environment. There are no options: an argument
/// starting with - is part of the command.
///
/// Exit status:
///   the status of the command, or 0 without arguments or with only empty ones
///   >0  the result cannot be parsed
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "eval [argument...]")]
pub(super) struct Args {}

/// Join the arguments with single spaces and let the shell read and run the result
/// in the current environment. Its status is the status of `eval`: zero without
/// arguments, or with only empty ones, and non-zero when the result cannot be
/// parsed. There are no options, an argument starting with `-` is part of the
/// command.
pub fn eval(bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::Eval(bi.args[1..].join(" ")))
}
