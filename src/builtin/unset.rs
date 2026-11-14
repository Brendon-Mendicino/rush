//! `unset` - unset values and attributes of variables and functions (special built-in).
//!
//! ```text
//! unset [-fv] name...
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status, check_name};

/// Unset values and attributes of variables and functions.
///
/// Unset each variable NAME: its value and its attributes go away, so it is no
/// longer exported. A readonly variable can't be unset. Unsetting a variable
/// that is not set is not an error.
///
/// With -f, NAME is a function instead, and with -v a variable (the default).
///
/// Exit status:
///   0   success
///   >0  a NAME is invalid, or readonly (the next ones are skipped)
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "unset [-fv] name...")]
pub(super) struct Args {
    /// NAME is a function.
    #[arg(short = 'f', conflicts_with = "variables")]
    functions: bool,
    /// NAME is a variable (default).
    #[arg(short = 'v')]
    variables: bool,
    /// The variables or functions to unset.
    names: Vec<String>,
}

pub fn unset(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;

    // The first error stops: a special built-in's error exits a script anyway.
    for name in &args.names {
        check_name(name)?;

        if args.functions {
            // TODO: remove the function once there are functions. Until then
            // there is none to unset, which is not an error.
            continue;
        }

        bi.shell.env.unset(name)?;
    }

    Ok(Status::SUCCESS)
}
