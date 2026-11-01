//! `type` - write a description of command type (regular built-in).
//!
//! ```text
//! type name...
//! ```
//!
//! Not implemented yet. Spec: `docs/utilities/type.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn r#type(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
