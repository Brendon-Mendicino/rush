//! `wait` - await process completion (regular built-in).
//!
//! ```text
//! wait [pid...]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/wait.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Await process completion.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "wait [pid...]")]
pub(super) struct Args {}

pub fn wait(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
