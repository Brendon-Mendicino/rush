//! `set` - set or unset options and positional parameters (special built-in).
//!
//! ```text
//! set [-abCefhmnuvx] [-o option] [argument...]
//! set [+abCefhmnuvx] [+o option] [argument...]
//! set -- [argument...]
//! set -o
//! set +o
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Set or unset options and positional parameters.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "set [-abCefhmnuvx] [-o option] [argument...]\n       set [+abCefhmnuvx] [+o option] [argument...]\n       set -- [argument...]\n       set -o\n       set +o"
)]
pub(super) struct Args {}

pub fn set(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
