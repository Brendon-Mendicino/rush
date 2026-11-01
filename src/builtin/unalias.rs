//! `unalias` - remove alias definitions (regular built-in).
//!
//! ```text
//! unalias alias-name...
//! unalias -a
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/unalias.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn unalias(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
