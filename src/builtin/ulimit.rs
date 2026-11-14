//! `ulimit` - set or report file size limit (regular built-in).
//!
//! ```text
//! ulimit [-f] [blocks]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/ulimit.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Set or report file size limit.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "ulimit [-f] [blocks]")]
pub(super) struct Args {}

pub fn ulimit(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
