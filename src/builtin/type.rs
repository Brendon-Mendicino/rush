//! `type` - write a description of command type (regular built-in).
//!
//! ```text
//! type name...
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/type.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Write a description of command type.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "type name...")]
pub(super) struct Args {}

pub fn r#type(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
