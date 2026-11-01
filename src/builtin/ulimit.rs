//! `ulimit` - set or report file size limit (regular built-in).
//!
//! ```text
//! ulimit [-f] [blocks]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/ulimit.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn ulimit(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
