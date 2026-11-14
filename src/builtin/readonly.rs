//! `readonly` - set the readonly attribute for variables (special built-in).
//!
//! ```text
//! readonly name[=word]...
//! readonly -p
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Set the readonly attribute for variables.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "readonly name[=word]...\n       readonly -p"
)]
pub(super) struct Args {}

pub fn readonly(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
