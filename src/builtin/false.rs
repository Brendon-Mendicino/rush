//! `false` - return false value (regular built-in).
//!
//! ```text
//! false
//! ```
//!
//! Spec: `docs/utilities/false.html`.

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Exit with a non-zero status. There are no options nor operands: the arguments are
/// ignored, as in other shells.
pub fn r#false(_bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::FAILURE)
}
