//! `trap` - trap signals (special built-in).
//!
//! ```text
//! trap n [condition...]
//! trap [action condition...]
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Trap signals.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "trap n [condition...]\n       trap [action condition...]"
)]
pub(super) struct Args {}

pub fn trap(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
