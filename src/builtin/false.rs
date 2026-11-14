//! `false` - return false value (regular built-in).
//!
//! ```text
//! false
//! ```
//!
//! Spec: `docs/utilities/false.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Return a non-zero status.
///
/// Exit with a non-zero status. There are no options nor operands: the
/// arguments are ignored, as in other shells.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "false")]
pub(super) struct Args {}

/// The arguments are ignored, see [`Args`].
pub fn r#false(_bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::FAILURE)
}
