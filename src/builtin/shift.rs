//! `shift` - shift positional parameters (special built-in).
//!
//! ```text
//! shift [n]
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Shift positional parameters.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "shift [n]")]
pub(super) struct Args {}

pub fn shift(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
