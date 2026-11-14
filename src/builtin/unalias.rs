//! `unalias` - remove alias definitions (regular built-in).
//!
//! ```text
//! unalias alias-name...
//! unalias -a
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/unalias.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Remove alias definitions.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "unalias alias-name...\n       unalias -a"
)]
pub(super) struct Args {}

pub fn unalias(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
