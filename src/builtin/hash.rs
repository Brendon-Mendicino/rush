//! `hash` - remember or report utility locations (regular built-in).
//!
//! ```text
//! hash [utility...]
//! hash -r
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/hash.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn hash(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
