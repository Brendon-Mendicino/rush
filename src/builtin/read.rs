//! `read` - read from standard input into shell variables (regular built-in).
//!
//! ```text
//! read [-r] var...
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/read.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn read(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
