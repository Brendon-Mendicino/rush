//! `read` - read from standard input into shell variables (regular built-in).
//!
//! ```text
//! read [-r] var...
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/read.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Read from standard input into shell variables.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "read [-r] var...")]
pub(super) struct Args {}

pub fn read(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
