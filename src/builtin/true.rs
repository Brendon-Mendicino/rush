//! `true` - return true value (regular built-in).
//!
//! ```text
//! true
//! ```
//!
//! Spec: `docs/utilities/true.html`.

use crate::builtin::{Builtin, BuiltinResult, Status};

/// Exit with a zero status. There are no options nor operands: the arguments are
/// ignored, as in other shells.
pub fn r#true(_bi: &mut Builtin) -> BuiltinResult {
    Ok(Status::SUCCESS)
}
