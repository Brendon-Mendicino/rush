//! `times` - write process times (special built-in).
//!
//! ```text
//! times
//! ```
//!
//! Not implemented yet. Spec: §2.14 Special Built-In Utilities in
//! `docs/The Open Group Base Specifications Issue 7, 2018 edition_files/mindex.html`.

use crate::builtin::{Builtin, BuiltinResult, not_implemented};

pub fn times(bi: &mut Builtin) -> BuiltinResult {
    not_implemented(bi)
}
