//! `bg` - run jobs in the background (regular built-in).
//!
//! ```text
//! bg [job_id...]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/bg.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn bg(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
