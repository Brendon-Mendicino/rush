//! `fc` - process the command history list (regular built-in).
//!
//! ```text
//! fc [-r] [-e editor] [first [last]]
//! fc -l [-nr] [first [last]]
//! fc -s [old=new] [first]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/fc.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Process the command history list.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(
    verbatim_doc_comment,
    override_usage = "fc [-r] [-e editor] [first [last]]\n       fc -l [-nr] [first [last]]\n       fc -s [old=new] [first]"
)]
pub(super) struct Args {}

pub fn fc(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
