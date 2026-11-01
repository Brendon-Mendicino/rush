//! `readonly` - set the readonly attribute for variables (special built-in).
//!
//! ```text
//! readonly name[=word]...
//! readonly -p
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn readonly(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
