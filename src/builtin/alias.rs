//! `alias` - define or display aliases (regular built-in).
//!
//! ```text
//! alias [alias-name[=string]...]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/alias.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Define or display aliases.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "alias [alias-name[=string]...]"
)]
pub(super) struct Args {}

pub fn alias(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
