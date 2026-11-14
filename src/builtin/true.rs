//! `true` - return true value (regular built-in).
//!
//! ```text
//! true
//! ```
//!
//! Spec: `docs/utilities/true.html`.

use clap::Parser;

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Return a zero status.
///
/// Exit with a zero status. There are no options nor operands: the arguments
/// are ignored, as in other shells.
#[derive(Debug, Parser)]
#[command(verbatim_doc_comment, override_usage = "true")]
pub(super) struct Args {}

/// The arguments are ignored, see [`Args`].
pub fn r#true(_bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::SUCCESS)
}
