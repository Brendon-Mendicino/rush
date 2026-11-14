//! `export` - set the export attribute for variables (special built-in).
//!
//! ```text
//! export name[=word]...
//! export -p
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Set the export attribute for variables.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "export name[=word]...\n       export -p"
)]
pub(super) struct Args {}

pub fn export(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
