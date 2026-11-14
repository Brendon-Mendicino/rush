//! `umask` - get or set the file mode creation mask (regular built-in).
//!
//! ```text
//! umask [-S] [mask]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/umask.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

/// Get or set the file mode creation mask.
///
/// Not implemented yet.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "umask [-S] [mask]")]
pub(super) struct Args {}

pub fn umask(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
