//! `:` - null utility (special built-in).
//!
//! ```text
//! : [argument...]
//! ```
//!
//! Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Do nothing, successfully.
///
/// Do nothing and exit with a zero status. The arguments are only expanded,
/// by the shell, and the redirections performed: : >file creates or truncates
/// file. There are no options, -- is not special either.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = ": [argument...]")]
pub(super) struct Args {}

/// The arguments are ignored, see [`Args`].
pub fn colon(_bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::SUCCESS)
}
