//! `unset` - unset values and attributes of variables and functions (special built-in).
//!
//! ```text
//! unset [-fv] name...
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Unset values and attributes of variables and functions.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "unset [-fv] name...")]
pub(super) struct Args {}

pub fn unset(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
