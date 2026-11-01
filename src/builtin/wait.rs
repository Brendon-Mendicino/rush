//! `wait` - await process completion (regular built-in).
//!
//! ```text
//! wait [pid...]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/wait.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn wait(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
