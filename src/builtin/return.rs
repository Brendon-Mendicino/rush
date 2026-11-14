//! `return` - return from a function or dot script (special built-in).
//!
//! ```text
//! return [n]
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Return from a function or dot script.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "return [n]")]
pub(super) struct Args {}

pub fn r#return(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
