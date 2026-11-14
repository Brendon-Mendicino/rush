//! `continue` - continue for, while, or until loop (special built-in).
//!
//! ```text
//! continue [n]
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Continue for, while, or until loop.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "continue [n]")]
pub(super) struct Args {}

pub fn r#continue(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
