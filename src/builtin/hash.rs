//! `hash` - remember or report utility locations (regular built-in).
//!
//! ```text
//! hash [utility...]
//! hash -r
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/hash.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Remember or report utility locations.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "hash [utility...]\n       hash -r"
)]
pub(super) struct Args {}

pub fn hash(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
