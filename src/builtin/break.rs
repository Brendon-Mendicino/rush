//! `break` - exit from for, while, or until loop (special built-in).
//!
//! ```text
//! break [n]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status, parse_loop_count};

/// Exit from for, while, or until loop.
///
/// Leave the Nth enclosing loop, 1 without N: the shell goes on after its
/// `done`. With N greater than the number of enclosing loops, the outermost one
/// is left. Outside a loop it does nothing.
///
/// Exit status:
///   0   always, unless N is not a positive integer
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "break [n]")]
pub(super) struct Args {
    /// How many enclosing loops to leave.
    #[arg(value_parser = parse_loop_count)]
    n: Option<usize>,
}

pub fn r#break(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;
    Ok(Status::Break(args.n.unwrap_or(1)))
}
