//! `break` - exit from for, while, or until loop (special built-in).
//!
//! ```text
//! break [n]
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Exit from for, while, or until loop.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "break [n]")]
pub(super) struct Args {}

pub fn r#break(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
