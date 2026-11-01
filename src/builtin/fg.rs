//! `fg` - run jobs in the foreground (regular built-in).
//!
//! ```text
//! fg [job_id]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/fg.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn fg(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
