//! `continue` - continue for, while, or until loop (special built-in).
//!
//! ```text
//! continue [n]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status, parse_loop_count};

/// Continue for, while, or until loop.
///
/// Go to the next iteration of the Nth enclosing loop, 1 without N: the rest
/// of the body is skipped. With N greater than the number of enclosing loops,
/// the outermost one goes on. Outside a loop it does nothing.
///
/// Exit status:
///   0   always, unless N is not a positive integer
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "continue [n]")]
pub(super) struct Args {
    /// Which enclosing loop goes on.
    #[arg(value_parser = parse_loop_count)]
    n: Option<usize>,
}

pub fn r#continue(bi: &mut Builtin) -> BuiltinResult {
    let args: Args = bi.parse()?;
    Ok(Status::Continue(args.n.unwrap_or(1)))
}
