//! `alias` - define or display aliases (regular built-in).
//!
//! ```text
//! alias [alias-name[=string]...]
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/alias.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn alias(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
